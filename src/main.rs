use core::str;
use std::{collections::HashSet, io::Write, net::SocketAddr, pin::Pin, str::FromStr, sync::Arc};

use axum::{http::HeaderMap, response::IntoResponse, Router};
use iprange::IpRange;
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;

/// Custom DNS resolver that validates resolved IPs against SSRF rules at connect time.
/// This eliminates the TOCTOU gap between check_url() and reqwest's actual connection.
struct SafeResolver {
	rtc: Arc<RuntimeConfig>,
}
impl reqwest::dns::Resolve for SafeResolver {
	fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
		let rtc = self.rtc.clone();
		let host = name.as_str().to_owned();
		Box::pin(async move {
			use std::net::ToSocketAddrs;
			let addrs: Vec<SocketAddr> = format!("{}:0", host)
				.to_socket_addrs()
				.map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?
				.collect();
			if addrs.is_empty() {
				return Err(Box::new(std::io::Error::new(
					std::io::ErrorKind::Other, "no addresses resolved",
				)) as Box<dyn std::error::Error + Send + Sync>);
			}
			for addr in &addrs {
				match addr {
					SocketAddr::V4(v4) => {
						if let Some(ref custom_blocked) = rtc.ipv4_custom_blocked {
							if custom_blocked.contains(v4.ip()) {
								return Err(Box::new(std::io::Error::new(
									std::io::ErrorKind::PermissionDenied, "Blocked address",
								)) as Box<dyn std::error::Error + Send + Sync>);
							}
						}
						if rtc.ipv4_blocked.contains(v4.ip()) {
							let allow = rtc.ipv4_allowed.as_ref()
								.map(|a| a.contains(v4.ip()))
								.unwrap_or(false);
							if !allow {
								return Err(Box::new(std::io::Error::new(
									std::io::ErrorKind::PermissionDenied, "Blocked address",
								)) as Box<dyn std::error::Error + Send + Sync>);
							}
						}
					},
					SocketAddr::V6(v6) => {
						if is_ipv6_blocked(v6.ip()) {
							return Err(Box::new(std::io::Error::new(
								std::io::ErrorKind::PermissionDenied, "Blocked address",
							)) as Box<dyn std::error::Error + Send + Sync>);
						}
					},
				}
			}
			Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
		})
	}
}

mod img;
mod svg;
mod browsersafe;
mod image_test;

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct ConfigFile{
	bind_addr: String,
	timeout:u64,
	user_agent:String,
	max_size:u64,
	proxy:Option<String>,
	filter_type:FilterType,
	max_pixels:u32,
	append_headers:Vec<String>,
	load_system_fonts:bool,
	webp_quality:f32,
	encode_avif:bool,
	allowed_networks:Option<Vec<String>>,
	blocked_networks:Option<Vec<String>>,
	blocked_hosts:Option<Vec<String>>,
	#[serde(default="default_max_concurrent")]
	max_concurrent:u32,
}
fn default_max_concurrent()->u32{ 64 }

/// Pre-parsed runtime config (parsed once at startup, shared via Arc)
pub struct RuntimeConfig{
	config: ConfigFile,
	ipv4_blocked: IpRange<Ipv4Net>,
	ipv4_allowed: Option<IpRange<Ipv4Net>>,
	ipv4_custom_blocked: Option<IpRange<Ipv4Net>>,
	blocked_hosts: HashSet<String>,
}

#[derive(Debug, Deserialize)]
pub struct RequestParams{
	url: String,
	//#[serde(rename = "static")]
	r#static:Option<String>,
	emoji:Option<String>,
	avatar:Option<String>,
	preview:Option<String>,
	badge:Option<String>,
	fallback:Option<String>,
}
#[derive(Clone, Copy,Debug,Serialize,Deserialize)]
enum FilterType{
	Nearest,
	Triangle,
	CatmullRom,
	Gaussian,
	Lanczos3,
}
impl From<FilterType> for image::imageops::FilterType{
	fn from(val: FilterType) -> Self {
		match val {
			FilterType::Nearest => image::imageops::Nearest,
			FilterType::Triangle => image::imageops::Triangle,
			FilterType::CatmullRom => image::imageops::CatmullRom,
			FilterType::Gaussian => image::imageops::Gaussian,
			FilterType::Lanczos3 => image::imageops::Lanczos3,
		}
	}
}
impl From<FilterType> for fast_image_resize::FilterType{
	fn from(val: FilterType) -> Self {
		match val {
			FilterType::Nearest => fast_image_resize::FilterType::Box,
			FilterType::Triangle => fast_image_resize::FilterType::Bilinear,
			FilterType::CatmullRom => fast_image_resize::FilterType::CatmullRom,
			FilterType::Gaussian => fast_image_resize::FilterType::Mitchell,
			FilterType::Lanczos3 => fast_image_resize::FilterType::Lanczos3,
		}
	}
}
async fn shutdown_signal() {
	use tokio::signal;
	use futures::{future::FutureExt,pin_mut};
	let ctrl_c = async {
		signal::ctrl_c()
			.await
			.expect("failed to install Ctrl+C handler");
	}.fuse();

	#[cfg(unix)]
	let terminate = async {
		signal::unix::signal(signal::unix::SignalKind::terminate())
			.expect("failed to install signal handler")
			.recv()
			.await;
	}.fuse();
	#[cfg(not(unix))]
	let terminate = std::future::pending::<()>().fuse();
	pin_mut!(ctrl_c, terminate);
	futures::select!{
		_ = ctrl_c => {},
		_ = terminate => {},
	}
}

/// Build RuntimeConfig: parse CIDR ranges and normalize blocked hosts at startup
fn build_runtime_config(config: ConfigFile) -> RuntimeConfig {
	// Private + loopback + link-local + CGNAT + unspecified
	let mut ipv4_blocked: IpRange<Ipv4Net> = [
		"10.0.0.0/8",
		"172.16.0.0/12",
		"192.168.0.0/16",
		"127.0.0.0/8",
		"169.254.0.0/16",
		"0.0.0.0/8",
		"100.64.0.0/10",
	]
		.iter()
		.map(|s| s.parse().expect("invalid built-in CIDR"))
		.collect();

	// Merge user-configured blocked networks
	let ipv4_custom_blocked = config.blocked_networks.as_ref().map(|nets| {
		nets.iter()
			.filter_map(|s| s.parse::<Ipv4Net>().ok())
			.collect::<IpRange<Ipv4Net>>()
	});
	if let Some(ref custom) = ipv4_custom_blocked {
		for net in custom.iter() {
			ipv4_blocked.add(net);
		}
	}

	let ipv4_allowed = config.allowed_networks.as_ref().map(|nets| {
		nets.iter()
			.filter_map(|s| s.parse::<Ipv4Net>().ok())
			.collect::<IpRange<Ipv4Net>>()
	});

	// Normalize blocked hosts to lowercase
	let blocked_hosts: HashSet<String> = config.blocked_hosts.as_ref()
		.map(|hosts| hosts.iter().map(|h| h.to_lowercase()).collect())
		.unwrap_or_default();

	RuntimeConfig {
		config,
		ipv4_blocked,
		ipv4_allowed,
		ipv4_custom_blocked,
		blocked_hosts,
	}
}

fn main() {
	let config_path=match std::env::var("MEDIA_PROXY_CONFIG_PATH"){
		Ok(path)=>{
			if path.is_empty(){
				"config.json".to_owned()
			}else{
				path
			}
		},
		Err(_)=>"config.json".to_owned()
	};
	if !std::path::Path::new(&config_path).exists(){
		let default_config=ConfigFile{
			bind_addr: "0.0.0.0:12766".to_owned(),
			timeout:10000,
			user_agent: "https://github.com/yojo-art/media-proxy-rs".to_owned(),
			max_size:256*1024*1024,
			proxy:None,
			filter_type:FilterType::Triangle,
			max_pixels:2048,
			append_headers:[
				"Content-Security-Policy:default-src 'none'; img-src 'self'; media-src 'self'; style-src 'unsafe-inline'".to_owned(),
				"Access-Control-Allow-Origin:*".to_owned(),
				"X-Content-Type-Options:nosniff".to_owned(),
			].to_vec(),
			load_system_fonts:true,
			webp_quality: 75f32,
			encode_avif:false,
			allowed_networks:None,
			blocked_networks:None,
			blocked_hosts:None,
			max_concurrent:64,
		};
		let default_config=serde_json::to_string_pretty(&default_config).expect("serialize default config");
		std::fs::File::create(&config_path).expect("create default config.json").write_all(default_config.as_bytes()).expect("write default config");
	}
	let mut config:ConfigFile=serde_json::from_reader(
		std::fs::File::open(&config_path).expect("open config.json")
	).expect("parse config.json");
	if let Ok(networks)=std::env::var("MEDIA_PROXY_ALLOWED_NETWORKS"){
		let mut allowed_networks=config.allowed_networks.take().unwrap_or_default();
		for networks in networks.split(","){
			allowed_networks.push(networks.to_owned());
		}
		config.allowed_networks.replace(allowed_networks);
	}
	if let Ok(networks)=std::env::var("MEDIA_PROXY_BLOCKED_NETWORKS"){
		let mut blocked_networks=config.blocked_networks.take().unwrap_or_default();
		for networks in networks.split(","){
			blocked_networks.push(networks.to_owned());
		}
		config.blocked_networks.replace(blocked_networks);
	}
	if let Ok(networks)=std::env::var("MEDIA_PROXY_BLOCKED_HOSTS"){
		let mut blocked_hosts=config.blocked_hosts.take().unwrap_or_default();
		for networks in networks.split(","){
			blocked_hosts.push(networks.to_owned());
		}
		config.blocked_hosts.replace(blocked_hosts);
	}
	let runtime_config = build_runtime_config(config);
	let dummy_png=Arc::new(include_bytes!("../asset/dummy.png").to_vec());
	let runtime_config=Arc::new(runtime_config);
	let rt=tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("build tokio runtime");
	let client=reqwest::ClientBuilder::new()
		.redirect(reqwest::redirect::Policy::none()) // Disable auto-redirect for SSRF protection
		.dns_resolver(Arc::new(SafeResolver{ rtc: runtime_config.clone() }));
	let client=match &runtime_config.config.proxy{
		Some(url)=>client.proxy(reqwest::Proxy::http(url).expect("invalid proxy URL")),
		None=>client,
	};
	let client=client.build().expect("build reqwest client");
	let mut fontdb=resvg::usvg::fontdb::Database::new();
	if runtime_config.config.load_system_fonts{
		fontdb.load_system_fonts();
	}
	if std::path::Path::new("asset/font/").exists(){
		fontdb.load_fonts_dir("asset/font/");
	}
	fontdb.load_font_source(resvg::usvg::fontdb::Source::Binary(Arc::new(include_bytes!("../asset/font/Aileron-Light.otf"))));
	let fontdb=Arc::new(fontdb);
	let bind_addr=runtime_config.config.bind_addr.clone();
	let semaphore=Arc::new(tokio::sync::Semaphore::new(runtime_config.config.max_concurrent as usize));
	let arg_tup=(client,runtime_config,dummy_png,fontdb,semaphore);
	rt.block_on(async{
		let app = Router::new();
		let arg_tup0=arg_tup.clone();
		let arg_tup_transform=arg_tup.clone();
		let app=app.route("/",axum::routing::get(move|headers,parms|get_file(None,headers,arg_tup0.clone(),parms)));
		let app=app.route("/transform",axum::routing::post(move|headers,multipart|post_transform(headers,arg_tup_transform.clone(),multipart)));
		let app=app.route("/{*path}",axum::routing::get(move|path,headers,parms|get_file(Some(path),headers,arg_tup.clone(),parms)));
		let bind_addr=&bind_addr;
		if bind_addr.starts_with("/") || bind_addr.ends_with(".sock") {
			// Unix domain socket mode
			let path=std::path::Path::new(bind_addr);
			if path.exists() {
				std::fs::remove_file(path).expect("failed to remove existing socket file");
			}
			let listener=tokio::net::UnixListener::bind(path).expect("failed to bind unix socket");
			println!("Listening on unix:{}",bind_addr);
			axum::serve(listener,app.into_make_service()).with_graceful_shutdown(shutdown_signal()).await.expect("serve failed");
			// Clean up socket on shutdown
			let _=std::fs::remove_file(path);
		} else {
			// TCP mode
			let http_addr:SocketAddr=bind_addr.parse().expect("invalid bind_addr");
			let listener=tokio::net::TcpListener::bind(http_addr).await.expect("failed to bind TCP");
			println!("Listening on tcp://{}",http_addr);
			axum::serve(listener,app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown_signal()).await.expect("serve failed");
		}
	});
}

/// Check if an IPv6 address should be blocked (loopback, mapped IPv4, ULA, link-local, etc.)
fn is_ipv6_blocked(ip: &std::net::Ipv6Addr) -> bool {
	if ip.is_loopback() || ip.is_multicast() || ip.is_unspecified() {
		return true;
	}
	// Link-local fe80::/10
	if (ip.segments()[0] & 0xffc0) == 0xfe80 {
		return true;
	}
	// Unique local fc00::/7
	if (ip.segments()[0] & 0xfe00) == 0xfc00 {
		return true;
	}
	// IPv4-mapped ::ffff:x.x.x.x — delegate to IPv4 check
	if let Some(v4) = ip.to_ipv4_mapped() {
		// Will be checked by IPv4 logic at call site
		return v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified();
	}
	false
}

fn check_url(rtc:&RuntimeConfig,url:impl AsRef<str>)->Result<(),String>{
	let u=reqwest::Url::from_str(url.as_ref()).map_err(|e|format!("{:?}",e))?;
	match u.scheme().to_lowercase().as_str(){
		"http"|"https"=>{},
		scheme=>return Err(format!("scheme: {}",scheme))
	}
	let host=u.host_str().ok_or_else(||"no host".to_owned())?;
	if rtc.blocked_hosts.contains(&host.to_lowercase()){
		return Err("Blocked address".to_owned());
	}
	use std::net::{SocketAddr, ToSocketAddrs};
	let ips=format!("{}:{}",host,u.port_or_known_default().unwrap_or(80)).to_socket_addrs().map_err(|e|format!("{:?} {}",e,host))?;
	for ip in ips{
		match ip{
			SocketAddr::V4(v4) => {
				if let Some(ref custom_blocked)=rtc.ipv4_custom_blocked{
					if custom_blocked.contains(v4.ip()){
						return Err("Blocked address".to_owned());
					}
				}
				if rtc.ipv4_blocked.contains(v4.ip()){
					let allow=if let Some(ref allow_ips)=rtc.ipv4_allowed{
						allow_ips.contains(v4.ip())
					}else{
						false
					};
					if !allow{
						return Err("Blocked address".to_owned());
					}
				}
			},
			SocketAddr::V6(v6) => {
				if is_ipv6_blocked(v6.ip()){
					return Err("Blocked address".to_owned());
				}
			},
		}
	}
	Ok(())
}

/// Truncate a URL for safe logging (no tokens/secrets in logs)
fn truncate_url(url: &str, max_len: usize) -> String {
	if url.len() <= max_len {
		url.to_owned()
	} else {
		format!("{}...", &url[..max_len])
	}
}

/// Maximum number of redirects to follow manually
const MAX_REDIRECTS: usize = 5;

async fn post_transform(
	client_headers:axum::http::HeaderMap,
	(_client,rtc,dummy_img,fontdb,semaphore):(reqwest::Client,Arc<RuntimeConfig>,Arc<Vec<u8>>,Arc<resvg::usvg::fontdb::Database>,Arc<tokio::sync::Semaphore>),
	mut multipart:axum::extract::Multipart,
)->Result<(axum::http::StatusCode,HeaderMap,axum::body::Body),axum::response::Response>{
	let _permit = semaphore.try_acquire().map_err(|_| {
		(axum::http::StatusCode::SERVICE_UNAVAILABLE, HeaderMap::new()).into_response()
	})?;
	let max_size=rtc.config.max_size;
	let mut file_bytes:Option<Vec<u8>>=None;
	let mut avatar:Option<String>=None;
	let mut emoji:Option<String>=None;
	let mut preview:Option<String>=None;
	let mut r#static:Option<String>=None;
	let mut badge:Option<String>=None;
	while let Ok(Some(field))=multipart.next_field().await{
		let name=field.name().unwrap_or("").to_owned();
		match name.as_str(){
			"file"=>{
				let bytes=field.bytes().await.map_err(|_|{
					(axum::http::StatusCode::BAD_REQUEST,HeaderMap::new()).into_response()
				})?;
				if bytes.len() as u64>max_size{
					let mut headers=HeaderMap::new();
					headers.append("X-Proxy-Error","content-too-large".parse().unwrap());
					return Err((axum::http::StatusCode::BAD_REQUEST,headers).into_response());
				}
				file_bytes=Some(bytes.to_vec());
			},
			"avatar"=>avatar=field.text().await.ok(),
			"emoji"=>emoji=field.text().await.ok(),
			"preview"=>preview=field.text().await.ok(),
			"static"=>r#static=field.text().await.ok(),
			"badge"=>badge=field.text().await.ok(),
			_=>{}
		}
	}
	let src_bytes=match file_bytes{
		Some(b) if !b.is_empty()=>b,
		_=>return Err((axum::http::StatusCode::BAD_REQUEST,HeaderMap::new()).into_response()),
	};
	println!("{}\ttransform\tsize:{}\tavatar:{:?}\tpreview:{:?}\tbadge:{:?}\temoji:{:?}\tstatic:{:?}",
		chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
		src_bytes.len(),
		avatar,
		preview,
		badge,
		emoji,
		r#static,
	);
	let parms=RequestParams{
		url:String::new(),
		r#static,
		emoji,
		avatar,
		preview,
		badge,
		fallback:None,
	};
	// Detect format from bytes
	let is_svg=std::str::from_utf8(&src_bytes).map(|s|s.trim().starts_with("<svg")).unwrap_or(false);
	let codec=image::guess_format(&src_bytes).map_err(|e|Some(e));
	// Parse Accept header for AVIF
	let mut is_accept_avif=false;
	if rtc.config.encode_avif{
		if let Some(accept)=client_headers.get("Accept"){
			if let Ok(accept)=std::str::from_utf8(accept.as_bytes()){
				for e in accept.split(","){
					let mime=e.trim().split(';').next().unwrap_or("").trim();
					if mime=="image/avif"{
						is_accept_avif=true;
					}
				}
			}
		}
	}
	let mut headers=HeaderMap::new();
	headers.append("Cache-Control","no-cache".parse().unwrap());
	for line in rtc.config.append_headers.iter(){
		if let Some(idx)=line.find(":"){
			if idx+1>=line.len(){ continue; }
			if let Ok(k)=axum::http::HeaderName::from_str(&line[0..idx]){
				if let Ok(v)=line[idx+1..].parse(){
					headers.append(k,v);
				}
			}
		}
	}
	let mut ctx=RequestContext{
		is_accept_avif,
		headers,
		parms,
		src_bytes,
		config:Arc::new(rtc.config.clone()),
		codec,
		dummy_img,
		fontdb:fontdb.clone(),
	};
	if is_svg{
		if let Ok(img)=ctx.encode_svg(fontdb){
			ctx.headers.remove("Cache-Control");
			return Err(ctx.response_img(img));
		}else{
			return Err((axum::http::StatusCode::OK,ctx.headers.clone(),ctx.src_bytes.clone()).into_response());
		}
	}
	// Image encoding in blocking thread
	let header=ctx.headers.clone();
	let resp=if let Ok(resp)=tokio::runtime::Handle::current().spawn_blocking(move||{
		ctx.encode_img()
	}).await{
		resp
	}else{
		let mut h=header;
		h.append("X-Proxy-Error","ImageEncodeThread".parse().unwrap());
		return Err((axum::http::StatusCode::INTERNAL_SERVER_ERROR,h).into_response());
	};
	Err(resp)
}

async fn get_file(
	_path:Option<axum::extract::Path<String>>,
	client_headers:axum::http::HeaderMap,
	(client,rtc,dummy_img,fontdb,semaphore):(reqwest::Client,Arc<RuntimeConfig>,Arc<Vec<u8>>,Arc<resvg::usvg::fontdb::Database>,Arc<tokio::sync::Semaphore>),
	axum::extract::Query(q):axum::extract::Query<RequestParams>,
)->Result<(axum::http::StatusCode,HeaderMap,axum::body::Body),axum::response::Response>{
	let _permit = semaphore.try_acquire().map_err(|_| {
		(axum::http::StatusCode::SERVICE_UNAVAILABLE, HeaderMap::new()).into_response()
	})?;
	println!("{}\t{}\tavatar:{:?}\tpreview:{:?}\tbadge:{:?}\temoji:{:?}\tstatic:{:?}\tfallback:{:?}",
		chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
		truncate_url(&q.url, 200),
		q.avatar,
		q.preview,
		q.badge,
		q.emoji,
		q.r#static,
		q.fallback,
	);
	let mut headers=HeaderMap::new();
	// Sanitize URL before putting in header (truncate to safe length)
	if let Ok(url_val) = truncate_url(&q.url, 512).parse() {
		headers.append("X-Remote-Url", url_val);
	}
	if rtc.config.encode_avif{
		headers.append("Vary","Accept,Range".parse().unwrap());
	}
	let time=chrono::Utc::now();
	if let Err(_)=check_url(&rtc,&q.url){
		if q.fallback.is_some(){
			headers.append("Content-Type","image/png".parse().unwrap());
			return Err((axum::http::StatusCode::OK,headers,(*dummy_img).clone()).into_response());
		}
		return Err((axum::http::StatusCode::BAD_REQUEST,headers).into_response())
	};

	println!("check_url {}ms",(chrono::Utc::now()-time).num_milliseconds());

	// Manual redirect following with SSRF validation on each hop
	let mut current_url = q.url.clone();
	let mut resp = None;
	for _ in 0..MAX_REDIRECTS {
		let req = client.get(&current_url);
		let req = req.timeout(std::time::Duration::from_millis(rtc.config.timeout));
		let req = req.header("User-Agent", rtc.config.user_agent.clone());
		let req = if let Some(range) = client_headers.get("Range") {
			req.header("Range", range.as_bytes())
		} else {
			req
		};
		match req.send().await {
			Ok(r) => {
				let status = r.status();
				if status.is_redirection() {
					if let Some(location) = r.headers().get("location") {
						let loc_str = String::from_utf8_lossy(location.as_bytes()).to_string();
						// Resolve relative URLs
						let resolved = if loc_str.starts_with("http://") || loc_str.starts_with("https://") {
							loc_str
						} else {
							match reqwest::Url::from_str(&current_url) {
								Ok(base) => match base.join(&loc_str) {
									Ok(u) => u.to_string(),
									Err(_) => {
										return Err((axum::http::StatusCode::BAD_GATEWAY, headers).into_response());
									}
								},
								Err(_) => {
									return Err((axum::http::StatusCode::BAD_GATEWAY, headers).into_response());
								}
							}
						};
						// Validate redirect target against SSRF rules
						if let Err(_) = check_url(&rtc, &resolved) {
							return Err((axum::http::StatusCode::BAD_REQUEST, headers).into_response());
						}
						current_url = resolved;
						continue;
					} else {
						return Err((axum::http::StatusCode::BAD_GATEWAY, headers).into_response());
					}
				}
				resp = Some(r);
				break;
			},
			Err(_) => {
				if q.fallback.is_some(){
					headers.append("Content-Type","image/png".parse().unwrap());
					return Err((axum::http::StatusCode::OK,headers,(*dummy_img).clone()).into_response());
				}
				return Err((axum::http::StatusCode::BAD_GATEWAY,headers).into_response())
			}
		}
	}
	let resp = match resp {
		Some(r) => r,
		None => {
			// Too many redirects
			return Err((axum::http::StatusCode::BAD_GATEWAY, headers).into_response());
		}
	};

	fn add_remote_header(key:&'static str,headers:&mut HeaderMap,remote_headers:&reqwest::header::HeaderMap){
		for v in remote_headers.get_all(key){
			if let Ok(val) = String::from_utf8_lossy(v.as_bytes()).parse() {
				headers.append(key, val);
			}
		}
	}
	let remote_headers=resp.headers();
	add_remote_header("Content-Disposition",&mut headers,remote_headers);
	add_remote_header("Content-Type",&mut headers,remote_headers);
	let is_img=if let Some(media)=headers.get("Content-Type"){
		let s=String::from_utf8_lossy(media.as_bytes());
		s.starts_with("image/")
	}else{
		false
	};
	if !is_img{
		add_remote_header("Content-Length",&mut headers,remote_headers);
		add_remote_header("Content-Range",&mut headers,remote_headers);
		add_remote_header("Accept-Ranges",&mut headers,remote_headers);
	}
	// AVIF Accept header parsing: trim whitespace and strip quality params
	let mut is_accept_avif=false;
	if !rtc.config.encode_avif{
		//force no avif
	}else if let Some(accept)=client_headers.get("Accept"){
		if let Ok(accept)=std::str::from_utf8(accept.as_bytes()){
			for e in accept.split(","){
				let mime = e.trim().split(';').next().unwrap_or("").trim();
				if mime=="image/avif"{
					is_accept_avif=true;
				}
			}
		}
	}
	headers.append("Cache-Control","max-age=300".parse().unwrap());
	for line in rtc.config.append_headers.iter(){
		if let Some(idx)=line.find(":"){
			if idx+1>=line.len(){
				continue;
			}
			if let Ok(k)=axum::http::HeaderName::from_str(&line[0..idx]){
				if let Ok(v)=line[idx+1..].parse(){
					headers.append(k,v);
				}
			}
		}
	}
	RequestContext{
		is_accept_avif,
		headers,
		parms:q,
		src_bytes:Vec::new(),
		config:Arc::new(rtc.config.clone()),
		codec:Err(None),
		dummy_img,
		fontdb,
	}.encode(resp,is_img).await
}
struct RequestContext{
	is_accept_avif:bool,
	headers:HeaderMap,
	parms:RequestParams,
	src_bytes:Vec<u8>,
	config:Arc<ConfigFile>,
	codec:Result<image::ImageFormat,Option<image::ImageError>>,
	dummy_img:Arc<Vec<u8>>,
	fontdb:Arc<resvg::usvg::fontdb::Database>,
}
impl RequestContext{
	pub fn disposition_ext(headers:&mut HeaderMap,ext:&str){
		let k="Content-Disposition";
		if let Some(cd)=headers.get(k){
			let s=std::str::from_utf8(cd.as_bytes());
			if let Ok(s)=s{
				let cd=mailparse::parse_content_disposition(s);
				let cd_utf8=cd.params.get("filename*");
				let mut name=None;
				if let Some(cd_utf8)=cd_utf8{
					let cd_utf8=cd_utf8.to_uppercase();
					if cd_utf8.starts_with("UTF-8''")&&cd_utf8.len()>7{
						name=urlencoding::decode(&cd_utf8[7..]).map(|s|s.to_string()).ok();
					}
				}
				if name.is_none(){
					if let Some(filename)=cd.params.get("filename"){
						let m_filename=format!("_:{}",filename);
						let parsed=mailparse::parse_header(&m_filename.as_bytes());
						if let Ok((parsed,_))=&parsed{
							name=Some(parsed.get_value());
						}else if cd.params.get("name").is_none(){
							name=Some(filename.clone());
						}
					}
				}
				let name=name.unwrap_or_else(||cd.params.get("name").map(|s|s.clone()).unwrap_or_else(||"null".to_owned()));
				let mut name_arr:Vec<&str>=name.split('.').collect();
				name_arr.pop();
				let name=name_arr.join(".")+ext;
				let name=urlencoding::encode(&name);
				let content_disposition=format!("inline; filename=\"{}\";filename*=UTF-8''{};",name,name);
				headers.remove(k);
				if let Ok(val) = content_disposition.parse() {
					headers.append(k, val);
				}
			}
		}
	}
}
impl RequestContext{
	async fn encode(mut self,resp: reqwest::Response,mut is_img:bool)->Result<(axum::http::StatusCode,HeaderMap,axum::body::Body),axum::response::Response>{
		let mut is_svg=false;
		let mut content_type=None;
		if let Some(media)=self.headers.get("Content-Type"){
			let s=String::from_utf8_lossy(media.as_bytes());
			if s.as_ref()=="image/svg+xml"{
				is_svg=true;
			}else{
				content_type=Some(s);
			}
		}
		let status=resp.status();
		let resp=PreDataStream::new(resp).await;
		if let Some(Ok(head))=resp.head.as_ref(){
			//utf8にパースできて空白文字を削除した後の先頭部分が<svgの場合はsvg
			if std::str::from_utf8(&head).map(|s|s.trim().starts_with("<svg")).unwrap_or(false){
				is_svg=true;
			}else{
				self.codec=image::guess_format(head).map_err(|e|Some(e));
				if self.codec.is_err(){
					if let Some(content_type)=content_type.as_ref(){
						match content_type.as_ref(){
							"image/x-targa"|"image/x-tga"=>self.codec=Ok(image::ImageFormat::Tga),
							_=>{}
						}
					}
					if head.starts_with(&[0xFF,0x0A])||head.starts_with(&[0x00,0x00,0x00,0x0C,0x4A,0x58,0x4C,0x20,0x0D,0x0A,0x87,0x0A]){
						is_img=true;
						self.headers.remove("Content-Type");
						self.headers.append("Content-Type", "image/jxl".parse().unwrap());
					}
					if head.starts_with(&[0xFF,0x4F,0xFF,0x51])||head.starts_with(&[0x00,0x00,0x00,0x0C,0x6A,0x50,0x20,0x20,0x0D,0x0A,0x87,0x0A]){
						is_img=true;
						self.headers.remove("Content-Type");
						self.headers.append("Content-Type", "image/jp2".parse().unwrap());
					}
					if head.starts_with(&[0x49,0x49,0xBC]){
						is_img=true;
						self.headers.remove("Content-Type");
						self.headers.append("Content-Type", "image/jxr".parse().unwrap());
					}
				}
			}
		}
		if is_svg{
			self.load_all(resp).await?;
			if let Ok(img)=self.encode_svg(self.fontdb.clone()){
				self.headers.remove("Content-Length");
				self.headers.remove("Content-Range");
				self.headers.remove("Accept-Ranges");
				self.headers.remove("Cache-Control");
				self.headers.append("Cache-Control","max-age=31536000, immutable".parse().unwrap());
				return Err(self.response_img(img));
			}else{
				return Err((axum::http::StatusCode::OK,self.headers.clone(),self.src_bytes.clone()).into_response());
			}
		}else if is_img||self.codec.is_ok(){
			self.headers.remove("Content-Length");
			self.headers.remove("Content-Range");
			self.headers.remove("Accept-Ranges");
			self.load_all(resp).await?;
			let dummy_img=self.dummy_img.clone();
			let is_fallback=self.parms.fallback.is_some();
			let mut header=self.headers.clone();
			let mut handle=self;
			let resp=if let Ok(resp)=tokio::runtime::Handle::current().spawn_blocking(move ||{
				let resp=handle.encode_img();
				resp
			}).await{
				resp
			}else{
				header.append("X-Proxy-Error","ImageEncodeThread".parse().unwrap());
				return Err(if is_fallback{
					header.remove("Content-Type");
					header.append("Content-Type","image/png".parse().unwrap());
					(axum::http::StatusCode::OK,header,(*dummy_img).clone()).into_response()
				}else{
					(axum::http::StatusCode::INTERNAL_SERVER_ERROR,header).into_response()
				});
			};
			if is_fallback{
				return Err(if resp.status()==axum::http::StatusCode::OK{
					resp
				}else{
					header.remove("Content-Type");
					header.append("Content-Type","image/png".parse().unwrap());
					(axum::http::StatusCode::OK,header,(*dummy_img).clone()).into_response()
				});
			}
			return Err(resp);
		}
		if let Some(media)=self.headers.get("Content-Type"){
			let s=String::from_utf8_lossy(media.as_bytes());
			if crate::browsersafe::FILE_TYPE_BROWSERSAFE.contains(&s.as_ref()){

			}else{
				self.headers.remove("Content-Type");
				self.headers.append("Content-Type","octet-stream".parse().unwrap());
				Self::disposition_ext(&mut self.headers,".unknown");
			}
		}
		let body=axum::body::Body::from_stream(resp);
		if status.is_success(){
			self.headers.remove("Cache-Control");
			self.headers.append("Cache-Control","max-age=31536000, immutable".parse().unwrap());
			if status==reqwest::StatusCode::PARTIAL_CONTENT{
				Ok((axum::http::StatusCode::PARTIAL_CONTENT,self.headers.clone(),body))
			}else{
				Ok((axum::http::StatusCode::OK,self.headers.clone(),body))
			}
		}else{
			self.headers.append("X-Proxy-Error",format!("status:{}",status.as_u16()).parse().unwrap());
			Err(if self.parms.fallback.is_some(){
				self.headers.remove("Content-Type");
				self.headers.append("Content-Type","image/png".parse().unwrap());
				(axum::http::StatusCode::OK,self.headers.clone(),(*self.dummy_img).clone()).into_response()
			}else{
				let status=match status{
					reqwest::StatusCode::BAD_REQUEST=>axum::http::StatusCode::BAD_REQUEST,
					reqwest::StatusCode::FORBIDDEN=>axum::http::StatusCode::FORBIDDEN,
					reqwest::StatusCode::NOT_FOUND=>axum::http::StatusCode::NOT_FOUND,
					reqwest::StatusCode::REQUEST_TIMEOUT=>axum::http::StatusCode::GATEWAY_TIMEOUT,
					reqwest::StatusCode::GONE=>axum::http::StatusCode::GONE,
					reqwest::StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS=>axum::http::StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS,
					_=>axum::http::StatusCode::BAD_GATEWAY,
				};
				(status,self.headers.clone()).into_response()
			})
		}
	}
	async fn load_all(&mut self,resp: PreDataStream)->Result<(),axum::response::Response>{
		let len_hint=resp.content_length.unwrap_or(2048.min(self.config.max_size));
		if len_hint>self.config.max_size{
			self.headers.append("X-Proxy-Error","content-too-large".parse().unwrap());
			return Err((axum::http::StatusCode::BAD_GATEWAY,self.headers.clone()).into_response())
		}
		// Cap initial allocation to 4 MB to prevent malicious Content-Length from causing huge alloc
		let initial_cap = std::cmp::min(len_hint as usize, 4 * 1024 * 1024);
		let max_size = self.config.max_size;
		// Aggregate timeout: 3x the per-request timeout to prevent slow-drip attacks
		let download_timeout = std::time::Duration::from_millis(self.config.timeout * 3);
		let download_result = tokio::time::timeout(download_timeout, async move {
			let mut response_bytes=Vec::with_capacity(initial_cap);
			let mut resp = resp;
			while let Some(x) = resp.next().await{
				match x{
					Ok(b)=>{
						if response_bytes.len()+b.len()>max_size as usize{
							return Err("content-too-large");
						}
						response_bytes.extend_from_slice(&b);
					},
					Err(_)=>{
						return Err("upstream-read-error");
					}
				}
			}
			Ok(response_bytes)
		}).await;
		match download_result {
			Ok(Ok(bytes)) => {
				self.src_bytes = bytes;
				Ok(())
			},
			Ok(Err(e)) => {
				self.headers.append("X-Proxy-Error", e.parse().unwrap());
				Err((axum::http::StatusCode::BAD_GATEWAY, self.headers.clone()).into_response())
			},
			Err(_) => {
				self.headers.append("X-Proxy-Error", "download-timeout".parse().unwrap());
				Err((axum::http::StatusCode::BAD_GATEWAY, self.headers.clone()).into_response())
			}
		}
	}
}
struct PreDataStream{
	content_length:Option<u64>,
	head:Option<Result<axum::body::Bytes, reqwest::Error>>,
	last:Pin<Box<dyn futures::stream::Stream<Item=Result<axum::body::Bytes, reqwest::Error>>+Send+Sync>>,
}
impl  PreDataStream{
	async fn new(value: reqwest::Response) -> Self {
		let content_length=value.content_length();
		let mut stream=value.bytes_stream();
		let head=stream.next().await;
		Self{
			content_length,
			head,
			last: Box::pin(stream)
		}
	}
}
impl futures::stream::Stream for PreDataStream{
	type Item=Result<axum::body::Bytes, reqwest::Error>;

	fn poll_next(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
		let mut r=self.as_mut();
		if let Some(d)=r.head.take(){
			return std::task::Poll::Ready(Some(d));
		}
		r.last.as_mut().poll_next(cx)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn test_runtime_config() -> RuntimeConfig {
		let config = ConfigFile {
			bind_addr: "0.0.0.0:12766".to_owned(),
			timeout: 10000,
			user_agent: "test".to_owned(),
			max_size: 256 * 1024 * 1024,
			proxy: None,
			filter_type: FilterType::Triangle,
			max_pixels: 2048,
			append_headers: vec![],
			load_system_fonts: false,
			webp_quality: 75.0,
			encode_avif: false,
			allowed_networks: None,
			blocked_networks: None,
			blocked_hosts: Some(vec!["evil.com".to_owned(), "Evil.Net".to_owned()]),
			max_concurrent: 64,
		};
		build_runtime_config(config)
	}

	#[test]
	fn test_blocks_private_ipv4() {
		let rtc = test_runtime_config();
		// These resolve to loopback/private, should be blocked
		assert!(check_url(&rtc, "http://127.0.0.1/").is_err());
		assert!(check_url(&rtc, "http://10.0.0.1/").is_err());
		assert!(check_url(&rtc, "http://172.16.0.1/").is_err());
		assert!(check_url(&rtc, "http://192.168.1.1/").is_err());
		assert!(check_url(&rtc, "http://169.254.169.254/").is_err());
		assert!(check_url(&rtc, "http://0.0.0.0/").is_err());
	}

	#[test]
	fn test_blocks_ipv6_loopback() {
		let rtc = test_runtime_config();
		assert!(check_url(&rtc, "http://[::1]/").is_err());
	}

	#[test]
	fn test_blocks_invalid_scheme() {
		let rtc = test_runtime_config();
		assert!(check_url(&rtc, "ftp://example.com/").is_err());
		assert!(check_url(&rtc, "file:///etc/passwd").is_err());
	}

	#[test]
	fn test_blocked_hosts_case_insensitive() {
		let rtc = test_runtime_config();
		assert!(rtc.blocked_hosts.contains("evil.com"));
		assert!(rtc.blocked_hosts.contains("evil.net"));
		// Original casing should not appear
		assert!(!rtc.blocked_hosts.contains("Evil.Net"));
	}

	#[test]
	fn test_truncate_url() {
		assert_eq!(truncate_url("short", 10), "short");
		assert_eq!(truncate_url("a]long-string-here", 5), "a]lon...");
	}

	#[test]
	fn test_ipv6_blocked() {
		use std::net::Ipv6Addr;
		assert!(is_ipv6_blocked(&Ipv6Addr::LOCALHOST)); // ::1
		assert!(is_ipv6_blocked(&Ipv6Addr::UNSPECIFIED)); // ::
		// fe80::1 (link-local)
		assert!(is_ipv6_blocked(&"fe80::1".parse().unwrap()));
		// fc00::1 (ULA)
		assert!(is_ipv6_blocked(&"fc00::1".parse().unwrap()));
		// ::ffff:127.0.0.1 (mapped)
		assert!(is_ipv6_blocked(&"::ffff:127.0.0.1".parse().unwrap()));
	}
}
