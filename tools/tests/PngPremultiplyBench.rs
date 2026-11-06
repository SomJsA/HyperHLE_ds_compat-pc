//! Offline developer benchmark, not compiled into the game or Android APK.
//! rustc --edition=2021 -Copt-level=3 -Clto=thin -Ccodegen-units=1
//!   -Lnative=<matching release stb_image_wrapper/out> this.rs -o bench.exe
//! Run on read-only PNG paths. TOUCHHLE_DS_TEXTURE_WORKERS chooses worker count.
#[path="../../src/image_premultiply.rs"] mod premultiply;
use std::{ffi::{c_int,c_uchar,c_void,CStr,c_char},time::Instant};
#[link(name="stb_image_wrapper",kind="static")]
extern "C" {
 fn stbi_convert_iphone_png_to_rgb(flag:c_int);
 fn stbi_set_unpremultiply_on_load(flag:c_int);
 fn stbi_load_from_memory(buffer:*const c_uchar,len:c_int,x:*mut c_int,y:*mut c_int,channels:*mut c_int,desired:c_int)->*mut c_uchar;
 fn stbi_image_free(value:*mut c_void);
 fn stbi_failure_reason()->*const c_char;
}
#[inline(never)] fn legacy(pixels:&mut[u8]) {
 let mut i=0;while i<pixels.len(){let a=pixels[i+3]as f32/255.0;pixels[i]=(pixels[i]as f32*a)as u8;pixels[i+1]=(pixels[i+1]as f32*a)as u8;pixels[i+2]=(pixels[i+2]as f32*a)as u8;i+=4;}
}
fn main() {
 for path in std::env::args().skip(1) {
  let bytes=std::fs::read(&path).expect("read-only PNG input");let(mut w,mut h,mut channels)=(0,0,0);let start=Instant::now();
  let pixels=unsafe{stbi_convert_iphone_png_to_rgb(1);stbi_set_unpremultiply_on_load(1);stbi_load_from_memory(bytes.as_ptr(),bytes.len().try_into().unwrap(),&mut w,&mut h,&mut channels,4)};
  if pixels.is_null(){panic!("decoder rejected input: {:?}",unsafe{CStr::from_ptr(stbi_failure_reason())});}
  let decode_us=start.elapsed().as_micros();assert!(w>0&&h>0);let len=(w as usize).checked_mul(h as usize).unwrap().checked_mul(4).unwrap();
  let rgba=unsafe{std::slice::from_raw_parts(pixels,len)};let name=std::path::Path::new(&path).file_name().unwrap().to_str().unwrap();let name=name.replace('\\',"_").replace('"',"_");
  let mut expected=rgba.to_vec();legacy(&mut expected);
  println!("{{\"image\":\"{name}\",\"width\":{w},\"height\":{h},\"decode_only_us\":{decode_us},\"input_bytes\":{}}}",bytes.len());
  for round in 0..6 {for variant in if round%2==0{[0,1]}else{[1,0]} {
   let mut value=rgba.to_vec();let begin=Instant::now();
   let work=if variant==0{legacy(std::hint::black_box(&mut value));None}else{Some(premultiply::apply(std::hint::black_box(&mut value)))};
   let elapsed=begin.elapsed().as_micros();assert_eq!(value,expected,"byte output differs");
   println!("{{\"image\":\"{name}\",\"round\":{round},\"variant\":{variant},\"premultiply_us\":{elapsed},\"requested\":{},\"spawned\":{},\"fallback_chunks\":{},\"exact_equal\":true}}",work.map_or(1,|w|w.requested),work.map_or(0,|w|w.spawned),work.map_or(0,|w|w.fallback_chunks));
  }}
  unsafe{stbi_image_free(pixels.cast());}
 }
}
