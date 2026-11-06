//! Tutorial controls reuse the CC0 Kenney menu prompt artwork. Decode once;
//! rasterize only when the host's cached tutorial card is rebuilt.
use std::{collections::HashMap,sync::OnceLock};
struct Atlas {pixels:Vec<u8>,width:u32,height:u32,rects:HashMap<String,[u32;4]>}
fn atlas()->Option<&'static Atlas>{
 static VALUE:OnceLock<Option<Atlas>>=OnceLock::new();
 VALUE.get_or_init(||{
  let image=crate::image::Image::from_bytes(include_bytes!("../tools/pc-runtime/assets/input-prompts/tutorial-atlas.png")).ok()?;
  let meta:serde_json::Value=serde_json::from_slice(include_bytes!("../tools/pc-runtime/assets/input-prompts/tutorial-atlas.json")).ok()?;
  let(width,height)=image.dimensions();let mut rects=HashMap::new();
  for(key,value)in meta["sprites"].as_object()? {
   let a=value.as_array()?;if a.len()!=4{return None;}
   let mut r=[0u32;4];for(i,n)in a.iter().enumerate(){r[i]=u32::try_from(n.as_u64()?).ok()?;}
   if r[2]==0||r[3]==0||r[0].checked_add(r[2])?>width||r[1].checked_add(r[3])?>height{return None;}
   rects.insert(key.clone(),r);
  }
  Some(Atlas{pixels:image.pixels().to_vec(),width,height,rects})
 }).as_ref()
}
fn key(kind:&str,label:&str)->String{
 let lower=label.trim().to_ascii_lowercase().replace(' ',"_");
 match kind {
  "stick"=>format!("pad/{}",if label.contains('右'){"right-stick"}else{"left-stick"}),
  "dpad"=>"pad/dpad".into(),
  "mouse"=>format!("mouse/{}",if label.contains('右'){"right"}else if label.contains('左'){"left"}else{"move"}),
  "pad"=>format!("pad/{}",match lower.as_str(){"l3"=>"ls","r3"=>"rs",v=>v}),
  _=>format!("key/{}",match lower.as_str(){"left_shift"|"right_shift"=>"shift","left_ctrl"|"right_ctrl"=>"ctrl","left_alt"|"right_alt"=>"alt","return"=>"enter","esc"=>"escape","up"=>"arrow_up","down"=>"arrow_down","left"=>"arrow_left","right"=>"arrow_right",v=>v}),
 }
}
pub(crate) fn sprite(kind:&str,label:&str)->Option<[u32;4]>{atlas()?.rects.get(&key(kind,label)).copied()}
pub(crate) fn draw(bitmap:&mut[u8],w:u32,h:u32,r:[u32;4],left:f32,top:f32,height:f32){
 let Some(a)=atlas()else{return};debug_assert_eq!(a.pixels.len(),(a.width*a.height*4)as usize);
 let width=height*r[2]as f32/r[3]as f32;
 for y in 0..height.ceil()as u32 {for x in 0..width.ceil()as u32 {
  let sx=r[0]+((x as f32/width*r[2]as f32)as u32).min(r[2]-1);
  let sy=r[1]+((y as f32/height*r[3]as f32)as u32).min(r[3]-1);
  let at=((sy*a.width+sx)*4)as usize;let alpha=a.pixels[at+3]as u32;if alpha==0{continue;}
  // Image decodes premultiplied PNG pixels; the host text blend takes straight
  // RGB. Unpremultiply here so translucent white edges do not turn gray.
  let rgb=std::array::from_fn(|i|((a.pixels[at+i]as u32*255+alpha/2)/alpha).min(255)as u8);
  crate::ds_text_presentation::blend(bitmap,w,h,(left+x as f32)as i32,(top+y as f32)as i32,rgb,alpha as f32/255.);
 }}
}
#[cfg(test)]mod tests {
 use super::*;
 #[test]fn keyboard_a_and_controller_a_use_different_real_sprites(){assert_ne!(sprite("key","A").unwrap(),sprite("pad","A").unwrap());}
 #[test]fn tutorial_controls_and_remapped_bindings_have_asset_coverage(){
  for(kind,label)in[("key","W"),("key","Space"),("key","Right Shift"),("key","Z"),("key","G"),("key","5"),("pad","RB"),("pad","RT"),("pad","LS"),("pad","RS"),("stick","左摇杆"),("stick","右摇杆"),("mouse","鼠标右键"),("dpad","十字键")] {assert!(sprite(kind,label).is_some(),"{kind}:{label}");}
  assert!(sprite("key","unmapped custom key").is_none());
 }
}
