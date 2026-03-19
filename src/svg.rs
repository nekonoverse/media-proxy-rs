use std::sync::Arc;

use image::{DynamicImage, ImageBuffer};
use resvg::usvg;

use crate::RequestContext;

impl RequestContext{
	pub(crate) fn encode_svg(&self,fontdb:Arc<usvg::fontdb::Database>)->Result<DynamicImage,()>{
		let mut options=usvg::Options{
			fontdb:fontdb.clone(),
			..Default::default()
		};
		for f in fontdb.faces(){
			if let Some((name,_))=f.families.get(0){
				//デフォルトフォントに存在する事が確実なフォントを使う
				options.font_family=name.to_owned();
				break;
			}
		}
		let tree=usvg::Tree::from_data(&self.src_bytes,&options);
		let tree=match tree{
			Ok(t)=>t,
			Err(_)=>return Err(())
		};
		let size=size(&tree);
		let hint=self.image_size_hint();
		
		let (width,height,scale)=if size.width()>hint.0 as f32||size.height()>hint.1 as f32{
			let scale = f32::min(hint.0 as f32 / size.width(), hint.1 as f32 / size.height());
			let width=std::cmp::max((size.width() * scale).round() as u32,1);
			let height=std::cmp::max((size.height() * scale).round() as u32,1);
			(width,height,scale)
		}else{
			(size.width() as u32,size.height() as u32,1f32)
		};
		// Cap SVG rasterization dimensions to prevent decompression bomb
		const MAX_SVG_DIM: u32 = 4096;
		if width > MAX_SVG_DIM || height > MAX_SVG_DIM {
			return Err(());
		}
		let tf=usvg::Transform::from_scale(scale,scale);
		// Use u64 arithmetic to prevent overflow on width*height*4
		let buf_size = width as u64 * height as u64 * 4;
		if buf_size > 64 * 1024 * 1024 { // 64 MB max pixel buffer
			return Err(());
		}
		let mut rgba=vec![0;buf_size as usize];
		let mut pxmap=match resvg::tiny_skia::PixmapMut::from_bytes(&mut rgba,width,height){
			Some(p) => p,
			None => return Err(()),
		};
		resvg::render(&tree,tf,&mut pxmap);
		match ImageBuffer::from_vec(width,height,rgba){
			Some(img)=>{
				Ok(DynamicImage::ImageRgba8(img))
			},
			None=>{
				Err(())
			}
		}
	}
}
fn size(tree:&usvg::Tree)->usvg::Size{
	let bb=tree.root().bounding_box();
	if bb.width()>tree.size().width()||bb.height()>tree.size().height(){
		if let Some(size)=usvg::Size::from_wh(bb.width(),bb.height()){
			return size;
		}
	}
	tree.size()
}
