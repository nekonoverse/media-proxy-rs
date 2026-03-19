use std::{net::SocketAddr, str::FromStr};

use axum::{response::IntoResponse, Router};

fn main() {
	let args:Vec<String>=std::env::args().collect();
	let bind_port=args.get(1).expect("args[1]=bind_port");
	let target_url=args.get(2).expect("args[2]=target_url");
	let http_addr:SocketAddr = SocketAddr::new("127.0.0.1".parse().unwrap(),bind_port.parse().expect("bind_port parse"));
	let self_url=reqwest::Url::from_str(&format!("http://{}:{}/dummy.png",http_addr.ip().to_string(),http_addr.port())).unwrap();
	let rt=tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
	rt.spawn(async move{
		let listener = tokio::net::TcpListener::bind(http_addr).await.unwrap();
		let app = Router::new();
		let app=app.route("/dummy.png",axum::routing::get(||async{
			(axum::http::StatusCode::OK,include_bytes!("../asset/dummy.png").to_vec()).into_response()
		}));
		axum::serve(listener,app.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
	});

	// Determine if target is a Unix socket path or HTTP URL
	let is_uds = target_url.starts_with("/") || target_url.ends_with(".sock");

	let client = if is_uds {
		// For UDS: build a client that connects via Unix socket
		// We use hyper_util + tower to connect through UDS
		reqwest::Client::builder()
			.timeout(std::time::Duration::from_millis(500))
			.build()
			.unwrap()
	} else {
		reqwest::Client::builder()
			.timeout(std::time::Duration::from_millis(500))
			.build()
			.unwrap()
	};

	let mut local_ok=false;
	for _ in 0..20{
		std::thread::sleep(std::time::Duration::from_millis(50));
		let self_url=self_url.clone();
		let client=client.clone();
		let status=rt.block_on(async move{
			if let Ok(s)=client.get(self_url).send().await{
				s.status().as_u16()
			}else{
				504
			}
		});
		if status==200{
			local_ok=true;
			break;
		}
		std::thread::sleep(std::time::Duration::from_millis(50));
	}
	if !local_ok{
		println!("test server bind error");
		std::process::exit(1);
	}

	if is_uds {
		// UDS healthcheck: use curl-like approach via hyper with Unix socket
		for _ in 0..5{
			let self_url=self_url.to_string();
			let sock_path=target_url.clone();
			let status=rt.block_on(async move{
				uds_request(&sock_path, &format!("/?url={}", self_url)).await
			});
			if status==200{
				println!("ok");
				std::process::exit(0);
			}
			std::thread::sleep(std::time::Duration::from_millis(500));
		}
	} else {
		// TCP healthcheck (original behavior)
		for _ in 0..5{
			let self_url=self_url.to_string();
			let client=client.clone();
			let status=rt.block_on(async move{
				if let Ok(s)=client.get(format!("{}?url={}",target_url,self_url)).send().await{
					s.status().as_u16()
				}else{
					504
				}
			});
			if status==200{
				println!("ok");
				std::process::exit(0);
			}
			std::thread::sleep(std::time::Duration::from_millis(500));
		}
	}
	std::process::exit(2);
}

/// Send an HTTP request over a Unix domain socket
async fn uds_request(sock_path: &str, path: &str) -> u16 {
	use tokio::net::UnixStream;
	use tokio::io::{AsyncWriteExt, AsyncReadExt};

	let stream = match UnixStream::connect(sock_path).await {
		Ok(s) => s,
		Err(_) => return 504,
	};
	let (mut reader, mut writer) = tokio::io::split(stream);
	let request = format!(
		"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
		path
	);
	if writer.write_all(request.as_bytes()).await.is_err() {
		return 504;
	}
	if writer.shutdown().await.is_err() {
		return 504;
	}
	let mut buf = vec![0u8; 4096];
	let n = match reader.read(&mut buf).await {
		Ok(n) => n,
		Err(_) => return 504,
	};
	let response = String::from_utf8_lossy(&buf[..n]);
	// Parse HTTP status from first line: "HTTP/1.1 200 OK"
	if let Some(line) = response.lines().next() {
		let parts: Vec<&str> = line.split_whitespace().collect();
		if parts.len() >= 2 {
			return parts[1].parse().unwrap_or(504);
		}
	}
	504
}
