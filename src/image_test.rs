#[test]
fn encode_decode_png(){
	let dummy=include_bytes!("../asset/dummy.png");
	let img=image::load_from_memory(dummy).expect("load dummy.png");
	let mut buf=vec![];
	img.write_to(&mut std::io::Cursor::new(&mut buf),image::ImageFormat::Png).expect("encode png");
}
#[test]
#[cfg(feature="avif-decoder")]
fn encode_decode_avif(){
	let dummy=include_bytes!("../asset/dummy.png");
	let img=image::load_from_memory(dummy).expect("load dummy.png");
	let mut buf=vec![];
	img.write_to(&mut std::io::Cursor::new(&mut buf),image::ImageFormat::Avif).expect("encode avif");
	//https://github.com/image-rs/image/issues/1930
	//let format=image::guess_format(&buf).expect("guess format");
	let format=image::ImageFormat::Avif;
	image::load_from_memory_with_format(&buf,format).expect("decode avif");
}
#[test]
#[cfg(not(feature="avif-decoder"))]
fn encode_avif(){
	let dummy=include_bytes!("../asset/dummy.png");
	let img=image::load_from_memory(dummy).expect("load dummy.png");
	let mut buf=vec![];
	img.write_to(&mut std::io::Cursor::new(&mut buf),image::ImageFormat::Avif).expect("encode avif");
}
#[test]
fn encode_decode_webp(){
	let dummy=include_bytes!("../asset/dummy.png");
	let img=image::load_from_memory(dummy).expect("load dummy.png");
	let img=img.into_rgba8();
	let encoer=webp::Encoder::from_rgba(img.as_raw(),img.width(),img.height());
	let mut buf=vec![];
	buf.extend_from_slice(&encoer.encode(75f32));
	webp::Decoder::new(&buf).decode().unwrap();
}

// --- Test helpers ---

#[cfg(test)]
fn test_config() -> std::sync::Arc<crate::ConfigFile> {
	std::sync::Arc::new(crate::ConfigFile {
		bind_addr: "0.0.0.0:12766".to_owned(),
		timeout: 10000,
		user_agent: "test".to_owned(),
		max_size: 256 * 1024 * 1024,
		proxy: None,
		filter_type: crate::FilterType::Triangle,
		max_pixels: 2048,
		append_headers: vec![],
		load_system_fonts: false,
		webp_quality: 75.0,
		encode_avif: false,
		allowed_networks: None,
		blocked_networks: None,
		blocked_hosts: None,
		max_concurrent: 64,
		variant_sizes: crate::VariantSizes::default(),
		enable_transform: false,
	})
}

#[cfg(test)]
fn default_parms() -> crate::RequestParams {
	crate::RequestParams {
		url: String::new(),
		r#static: None,
		emoji: None,
		avatar: None,
		preview: None,
		badge: None,
		fallback: None,
	}
}

#[cfg(test)]
fn test_request_context(parms: crate::RequestParams) -> crate::RequestContext {
	let mut fontdb = resvg::usvg::fontdb::Database::new();
	fontdb.load_font_source(resvg::usvg::fontdb::Source::Binary(std::sync::Arc::new(
		include_bytes!("../asset/font/Aileron-Light.otf"),
	)));
	crate::RequestContext {
		is_accept_avif: false,
		headers: axum::http::HeaderMap::new(),
		parms,
		src_bytes: Vec::new(),
		config: test_config(),
		codec: Err(None),
		dummy_img: std::sync::Arc::new(include_bytes!("../asset/dummy.png").to_vec()),
		fontdb: std::sync::Arc::new(fontdb),
	}
}

#[cfg(test)]
fn make_test_image(width: u32, height: u32) -> image::DynamicImage {
	image::DynamicImage::ImageRgba8(image::RgbaImage::new(width, height))
}

// --- image_size_hint() tests ---

#[test]
fn test_image_size_hint_badge() {
	let mut parms = default_parms();
	parms.badge = Some("1".to_owned());
	let ctx = test_request_context(parms);
	assert_eq!(ctx.image_size_hint(), (96, 96));
}

#[test]
fn test_image_size_hint_static() {
	let mut parms = default_parms();
	parms.r#static = Some("1".to_owned());
	let ctx = test_request_context(parms);
	assert_eq!(ctx.image_size_hint(), (498, 422));
}

#[test]
fn test_image_size_hint_emoji() {
	let mut parms = default_parms();
	parms.emoji = Some("1".to_owned());
	let ctx = test_request_context(parms);
	assert_eq!(ctx.image_size_hint(), (u32::MAX, 128));
}

#[test]
fn test_image_size_hint_preview() {
	let mut parms = default_parms();
	parms.preview = Some("1".to_owned());
	let ctx = test_request_context(parms);
	assert_eq!(ctx.image_size_hint(), (200, 200));
}

#[test]
fn test_image_size_hint_avatar() {
	let mut parms = default_parms();
	parms.avatar = Some("1".to_owned());
	let ctx = test_request_context(parms);
	assert_eq!(ctx.image_size_hint(), (u32::MAX, 320));
}

#[test]
fn test_image_size_hint_default() {
	let ctx = test_request_context(default_parms());
	assert_eq!(ctx.image_size_hint(), (2048, 2048));
}

#[test]
fn test_image_size_hint_badge_priority() {
	let mut parms = default_parms();
	parms.badge = Some("1".to_owned());
	parms.emoji = Some("1".to_owned());
	let ctx = test_request_context(parms);
	// badge is checked first
	assert_eq!(ctx.image_size_hint(), (96, 96));
}

// --- custom variant_sizes tests ---

#[cfg(test)]
fn test_config_with_variant_sizes(vs: crate::VariantSizes) -> std::sync::Arc<crate::ConfigFile> {
	let mut config = (*test_config()).clone();
	config.variant_sizes = vs;
	std::sync::Arc::new(config)
}

#[cfg(test)]
fn test_request_context_with_config(parms: crate::RequestParams, config: std::sync::Arc<crate::ConfigFile>) -> crate::RequestContext {
	let mut fontdb = resvg::usvg::fontdb::Database::new();
	fontdb.load_font_source(resvg::usvg::fontdb::Source::Binary(std::sync::Arc::new(
		include_bytes!("../asset/font/Aileron-Light.otf"),
	)));
	crate::RequestContext {
		is_accept_avif: false,
		headers: axum::http::HeaderMap::new(),
		parms,
		src_bytes: Vec::new(),
		config,
		codec: Err(None),
		dummy_img: std::sync::Arc::new(include_bytes!("../asset/dummy.png").to_vec()),
		fontdb: std::sync::Arc::new(fontdb),
	}
}

#[test]
fn test_image_size_hint_custom_badge() {
	let vs = crate::VariantSizes {
		badge: Some(crate::VariantSize { width: Some(64), height: Some(64) }),
		..Default::default()
	};
	let mut parms = default_parms();
	parms.badge = Some("1".to_owned());
	let ctx = test_request_context_with_config(parms, test_config_with_variant_sizes(vs));
	assert_eq!(ctx.image_size_hint(), (64, 64));
}

#[test]
fn test_image_size_hint_custom_avatar_null_width() {
	let vs = crate::VariantSizes {
		avatar: Some(crate::VariantSize { width: None, height: Some(256) }),
		..Default::default()
	};
	let mut parms = default_parms();
	parms.avatar = Some("1".to_owned());
	let ctx = test_request_context_with_config(parms, test_config_with_variant_sizes(vs));
	assert_eq!(ctx.image_size_hint(), (u32::MAX, 256));
}

#[test]
fn test_image_size_hint_custom_emoji() {
	let vs = crate::VariantSizes {
		emoji: Some(crate::VariantSize { width: Some(64), height: Some(64) }),
		..Default::default()
	};
	let mut parms = default_parms();
	parms.emoji = Some("1".to_owned());
	let ctx = test_request_context_with_config(parms, test_config_with_variant_sizes(vs));
	assert_eq!(ctx.image_size_hint(), (64, 64));
}

#[test]
fn test_image_size_hint_custom_preview() {
	let vs = crate::VariantSizes {
		preview: Some(crate::VariantSize { width: Some(400), height: Some(400) }),
		..Default::default()
	};
	let mut parms = default_parms();
	parms.preview = Some("1".to_owned());
	let ctx = test_request_context_with_config(parms, test_config_with_variant_sizes(vs));
	assert_eq!(ctx.image_size_hint(), (400, 400));
}

#[test]
fn test_image_size_hint_custom_static() {
	let vs = crate::VariantSizes {
		r#static: Some(crate::VariantSize { width: Some(1024), height: Some(768) }),
		..Default::default()
	};
	let mut parms = default_parms();
	parms.r#static = Some("1".to_owned());
	let ctx = test_request_context_with_config(parms, test_config_with_variant_sizes(vs));
	assert_eq!(ctx.image_size_hint(), (1024, 768));
}

// --- resize() method tests ---

#[test]
fn test_resize_no_op_when_fits() {
	let ctx = test_request_context(default_parms());
	let img = make_test_image(100, 100);
	let result = ctx.resize(img).unwrap();
	assert_eq!(result.width(), 100);
	assert_eq!(result.height(), 100);
}

#[test]
fn test_resize_scales_down() {
	let mut parms = default_parms();
	// Use preview to get 200x200 max
	parms.preview = Some("1".to_owned());
	let ctx = test_request_context(parms);
	let img = make_test_image(400, 200);
	let result = ctx.resize(img).unwrap();
	assert_eq!(result.width(), 200);
	assert_eq!(result.height(), 100);
}

#[test]
fn test_resize_badge_produces_lumaa8() {
	let mut parms = default_parms();
	parms.badge = Some("1".to_owned());
	let ctx = test_request_context(parms);
	let img = make_test_image(200, 100);
	let result = ctx.resize(img).unwrap();
	assert_eq!(result.width(), 96);
	assert_eq!(result.height(), 96);
	// Should be LumaA8
	assert!(result.as_luma_alpha8().is_some(), "badge resize should produce LumaA8");
}

#[test]
fn test_resize_avatar_max_height() {
	let mut parms = default_parms();
	parms.avatar = Some("1".to_owned());
	let ctx = test_request_context(parms);
	let img = make_test_image(1000, 1000);
	let result = ctx.resize(img).unwrap();
	assert_eq!(result.height(), 320);
	assert_eq!(result.width(), 320);
}

#[test]
fn test_resize_emoji_max_height() {
	let mut parms = default_parms();
	parms.emoji = Some("1".to_owned());
	let ctx = test_request_context(parms);
	let img = make_test_image(512, 256);
	let result = ctx.resize(img).unwrap();
	assert_eq!(result.height(), 128);
	assert_eq!(result.width(), 256);
}

// --- Module-level resize() tests ---

#[test]
fn test_module_resize_preserves_aspect_ratio() {
	let img = make_test_image(1000, 500);
	let result = crate::img::resize(img, 200, 200, fast_image_resize::FilterType::Bilinear).unwrap();
	assert_eq!(result.width(), 200);
	assert_eq!(result.height(), 100);
}

#[test]
fn test_module_resize_small_image() {
	let img = make_test_image(1, 1);
	let result = crate::img::resize(img, 100, 100, fast_image_resize::FilterType::Bilinear).unwrap();
	assert!(result.width() >= 1);
	assert!(result.height() >= 1);
}

#[test]
fn test_module_resize_tall_image() {
	let img = make_test_image(100, 1000);
	let result = crate::img::resize(img, 200, 200, fast_image_resize::FilterType::Bilinear).unwrap();
	assert_eq!(result.height(), 200);
	assert_eq!(result.width(), 20);
}

// --- image_to_frame() tests ---

#[test]
fn test_image_to_frame_rgba8() {
	let img = image::DynamicImage::ImageRgba8(image::RgbaImage::new(10, 10));
	assert!(crate::img::image_to_frame(&img, 100).is_ok());
}

#[test]
fn test_image_to_frame_rgb8() {
	let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(10, 10));
	assert!(crate::img::image_to_frame(&img, 0).is_ok());
}

#[test]
fn test_image_to_frame_luma8_error() {
	let img = image::DynamicImage::ImageLuma8(image::GrayImage::new(10, 10));
	assert_eq!(crate::img::image_to_frame(&img, 0), Err("Unimplemented"));
}

#[test]
fn test_image_to_frame_lumaa8_error() {
	let img = image::DynamicImage::ImageLumaA8(image::GrayAlphaImage::new(10, 10));
	assert_eq!(crate::img::image_to_frame(&img, 0), Err("Unimplemented"));
}

// --- jpegxr_img() tests ---

#[test]
fn test_jpegxr_8bpp_gray() {
	let buffer = vec![100, 150, 200, 250]; // 2x2 grayscale
	let result = crate::img::jpegxr_img(2, 2, 2, buffer, jpegxr::PixelFormat::PixelFormat8bppGray);
	assert!(result.is_some());
	let img = result.unwrap();
	assert_eq!(img.width(), 2);
	assert_eq!(img.height(), 2);
	assert!(img.as_luma8().is_some());
}

#[test]
fn test_jpegxr_24bpp_bgr_channel_swap() {
	// 1x1 pixel: BGR = [B=10, G=20, R=30]
	let buffer = vec![10, 20, 30];
	let result = crate::img::jpegxr_img(1, 1, 3, buffer, jpegxr::PixelFormat::PixelFormat24bppBGR);
	assert!(result.is_some());
	let img = result.unwrap();
	let rgb = img.as_rgb8().unwrap();
	let pixel = rgb.get_pixel(0, 0);
	assert_eq!(pixel.0, [30, 20, 10]); // R=30, G=20, B=10
}

#[test]
fn test_jpegxr_24bpp_rgb() {
	let buffer = vec![10, 20, 30]; // 1x1 pixel RGB
	let result = crate::img::jpegxr_img(1, 1, 3, buffer, jpegxr::PixelFormat::PixelFormat24bppRGB);
	assert!(result.is_some());
	let img = result.unwrap();
	let rgb = img.as_rgb8().unwrap();
	let pixel = rgb.get_pixel(0, 0);
	assert_eq!(pixel.0, [10, 20, 30]);
}

#[test]
fn test_jpegxr_32bpp_bgra_channel_swap() {
	// 1x1 pixel: BGRA = [B=10, G=20, R=30, A=255]
	let buffer = vec![10, 20, 30, 255];
	let result = crate::img::jpegxr_img(1, 1, 4, buffer, jpegxr::PixelFormat::PixelFormat32bppBGRA);
	assert!(result.is_some());
	let img = result.unwrap();
	let rgba = img.as_rgba8().unwrap();
	let pixel = rgba.get_pixel(0, 0);
	assert_eq!(pixel.0, [30, 20, 10, 255]); // R=30, G=20, B=10, A=255
}

#[test]
fn test_jpegxr_32bpp_rgba() {
	let buffer = vec![10, 20, 30, 255]; // 1x1 RGBA
	let result = crate::img::jpegxr_img(1, 1, 4, buffer, jpegxr::PixelFormat::PixelFormat32bppRGBA);
	assert!(result.is_some());
	let img = result.unwrap();
	let rgba = img.as_rgba8().unwrap();
	let pixel = rgba.get_pixel(0, 0);
	assert_eq!(pixel.0, [10, 20, 30, 255]);
}

#[test]
fn test_jpegxr_buffer_too_small() {
	let buffer = vec![0; 2]; // too small for 2x2 at 24bpp BGR
	let result = crate::img::jpegxr_img(2, 2, 6, buffer, jpegxr::PixelFormat::PixelFormat24bppBGR);
	assert!(result.is_none());
}

// --- response_img() tests ---

#[test]
fn test_response_img_badge_png() {
	use axum::response::IntoResponse;
	let mut parms = default_parms();
	parms.badge = Some("1".to_owned());
	let mut ctx = test_request_context(parms);
	ctx.codec = Ok(image::ImageFormat::Png);
	ctx.src_bytes = include_bytes!("../asset/dummy.png").to_vec();
	let img = image::load_from_memory(&ctx.src_bytes).unwrap();
	let resp = ctx.response_img(img);
	assert_eq!(resp.status(), axum::http::StatusCode::OK);
	assert_eq!(
		resp.headers().get("Content-Type").unwrap().to_str().unwrap(),
		"image/png"
	);
}

#[test]
fn test_response_img_avif_when_accepted() {
	use axum::response::IntoResponse;
	let mut ctx = test_request_context(default_parms());
	ctx.is_accept_avif = true;
	ctx.codec = Ok(image::ImageFormat::Png);
	ctx.src_bytes = include_bytes!("../asset/dummy.png").to_vec();
	let img = image::load_from_memory(&ctx.src_bytes).unwrap();
	let resp = ctx.response_img(img);
	assert_eq!(resp.status(), axum::http::StatusCode::OK);
	assert_eq!(
		resp.headers().get("Content-Type").unwrap().to_str().unwrap(),
		"image/avif"
	);
}

#[test]
fn test_response_img_webp_default() {
	use axum::response::IntoResponse;
	let mut ctx = test_request_context(default_parms());
	ctx.codec = Ok(image::ImageFormat::Png);
	ctx.src_bytes = include_bytes!("../asset/dummy.png").to_vec();
	let img = image::load_from_memory(&ctx.src_bytes).unwrap();
	let resp = ctx.response_img(img);
	assert_eq!(resp.status(), axum::http::StatusCode::OK);
	assert_eq!(
		resp.headers().get("Content-Type").unwrap().to_str().unwrap(),
		"image/webp"
	);
}

// --- encode_single() tests ---

#[test]
fn test_encode_single_valid_png() {
	use axum::response::IntoResponse;
	let mut ctx = test_request_context(default_parms());
	ctx.src_bytes = include_bytes!("../asset/dummy.png").to_vec();
	ctx.codec = Ok(image::ImageFormat::Png);
	let resp = ctx.encode_single();
	assert_eq!(resp.status(), axum::http::StatusCode::OK);
}

#[test]
fn test_encode_single_unknown_format() {
	use axum::response::IntoResponse;
	let mut ctx = test_request_context(default_parms());
	ctx.codec = Err(None);
	let resp = ctx.encode_single();
	assert_eq!(resp.status(), axum::http::StatusCode::BAD_GATEWAY);
	assert!(resp.headers().get("X-Proxy-Error").is_some());
}

#[test]
fn test_encode_single_corrupt_data() {
	use axum::response::IntoResponse;
	let mut ctx = test_request_context(default_parms());
	ctx.src_bytes = vec![0xFF, 0x00, 0xAB, 0xCD]; // garbage
	ctx.codec = Ok(image::ImageFormat::Png);
	let resp = ctx.encode_single();
	assert_eq!(resp.status(), axum::http::StatusCode::BAD_GATEWAY);
}

// --- SVG tests ---

#[test]
fn test_encode_svg_simple() {
	let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="red"/></svg>"#;
	let mut ctx = test_request_context(default_parms());
	ctx.src_bytes = svg.to_vec();
	let result = ctx.encode_svg(ctx.fontdb.clone());
	assert!(result.is_ok());
	let img = result.unwrap();
	assert_eq!(img.width(), 100);
	assert_eq!(img.height(), 100);
}

#[test]
fn test_encode_svg_scaling() {
	// SVG larger than default max_pixels (2048) should be scaled down
	let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4000" height="2000"><rect width="4000" height="2000" fill="blue"/></svg>"#;
	let mut ctx = test_request_context(default_parms());
	ctx.src_bytes = svg.to_vec();
	let result = ctx.encode_svg(ctx.fontdb.clone());
	assert!(result.is_ok());
	let img = result.unwrap();
	assert!(img.width() <= 2048);
	assert!(img.height() <= 2048);
}

#[test]
fn test_encode_svg_invalid() {
	let mut ctx = test_request_context(default_parms());
	ctx.src_bytes = b"<not valid svg>".to_vec();
	let result = ctx.encode_svg(ctx.fontdb.clone());
	assert!(result.is_err());
}

#[test]
fn test_encode_svg_empty() {
	let mut ctx = test_request_context(default_parms());
	ctx.src_bytes = Vec::new();
	let result = ctx.encode_svg(ctx.fontdb.clone());
	assert!(result.is_err());
}

// --- browsersafe tests ---

#[test]
fn test_browsersafe_contains_expected_types() {
	use crate::browsersafe::FILE_TYPE_BROWSERSAFE;
	assert!(FILE_TYPE_BROWSERSAFE.contains(&"audio/opus"));
	assert!(FILE_TYPE_BROWSERSAFE.contains(&"video/mp4"));
	assert!(FILE_TYPE_BROWSERSAFE.contains(&"audio/mpeg"));
	assert!(FILE_TYPE_BROWSERSAFE.contains(&"video/webm"));
	assert!(FILE_TYPE_BROWSERSAFE.contains(&"audio/flac"));
	assert!(FILE_TYPE_BROWSERSAFE.contains(&"audio/wav"));
}

#[test]
fn test_browsersafe_excludes_dangerous_types() {
	use crate::browsersafe::FILE_TYPE_BROWSERSAFE;
	assert!(!FILE_TYPE_BROWSERSAFE.contains(&"text/html"));
	assert!(!FILE_TYPE_BROWSERSAFE.contains(&"application/javascript"));
	assert!(!FILE_TYPE_BROWSERSAFE.contains(&"text/xml"));
	assert!(!FILE_TYPE_BROWSERSAFE.contains(&"image/svg+xml"));
}
