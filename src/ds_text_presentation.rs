//! Pure presentation for chapter acrostics and radio interference.
//! No guest state, timers, input, or iOS font scaling is changed here.
use rusttype::{point, Font, Scale};
use std::sync::OnceLock;

pub(crate) const RED: [u8; 3] = [235, 42, 48];
pub(crate) const WHITE: [u8; 3] = [255, 255, 255];
// Extra distance between title characters, in em. Header spacing stays independent.
const CHAPTER_TRACKING_EM:f32=0.10;
pub(crate) struct Raster { pub width: u32, pub height: u32, pub pixels: Vec<u8>, pub first_center: f32 }
pub(crate) struct ChapterFonts { pub chinese: Font<'static>, pub english: Font<'static> }
pub(crate) fn chapter_fonts() -> &'static ChapterFonts {
    static FONTS: OnceLock<ChapterFonts> = OnceLock::new();
    FONTS.get_or_init(|| ChapterFonts {
        chinese: Font::try_from_bytes(crate::ds_font_roles::TITLE).expect("chapter MStiffHei Chinese font"),
        english: Font::try_from_bytes(crate::ds_font_roles::TITLE).expect("chapter MStiffHei English font"),
    })
}

fn bounds(font: &Font, text: &str, size: f32, bold_first: bool) -> (f32, f32) {
    let mut left=0f32;let mut right=0f32;
    let tracking=if bold_first {size*CHAPTER_TRACKING_EM}else{0.};
    for (i,glyph) in font.layout(text,Scale::uniform(size),point(0.,0.)).enumerate() {
        if let Some(b)=glyph.pixel_bounding_box() {
            left=left.min(b.min.x as f32+i as f32*tracking);
            right=right.max(b.max.x as f32+i as f32*tracking + if bold_first && i==0 {(size*0.025).max(1.)} else {0.});
        }
        right=right.max(glyph.position().x+i as f32*tracking+glyph.unpositioned().h_metrics().advance_width);
    }
    (left,right)
}
pub(crate) fn fit_all(font: &Font, titles: &[&str], width: f32, height: f32) -> f32 {
    let(mut lo,mut hi)=(1f32,height.max(1.));
    for _ in 0..18 {
        let mid=(lo+hi)*0.5;
        if titles.iter().all(|t| {let(l,r)=bounds(font,t,mid,true);r-l<=width}) {lo=mid;} else {hi=mid;}
    }
    lo
}
pub(crate) fn blend(pixels: &mut [u8], width: u32, height: u32, x:i32,y:i32,color:[u8;3],alpha:f32) {
    if x<0 || y<0 || x>=width as i32 || y>=height as i32 || alpha<=0. {return;}
    let at=(y as usize*width as usize+x as usize)*4;
    let a=alpha.clamp(0.,1.);let old=pixels[at+3] as f32/255.;let out=a+old*(1.-a);
    for c in 0..3 {pixels[at+c]=((color[c] as f32*a+pixels[at+c] as f32*old*(1.-a))/out.max(0.001)).round() as u8;}
    pixels[at+3]=(out*255.).round() as u8;
}
fn draw_line(image:&mut Raster,font:&Font,text:&str,size:f32,top:f32,highlight:bool) {
    let(l,r)=bounds(font,text,size,highlight);
    let x=(image.width as f32-(r-l))*0.5-l;
    let ascent=font.v_metrics(Scale::uniform(size)).ascent;
    let first=text.chars().position(|c| !c.is_whitespace()).unwrap_or(usize::MAX);
    for (i,mut glyph) in font.layout(text,Scale::uniform(size),point(x,top+ascent)).enumerate() {
        glyph.set_position(point(glyph.position().x+i as f32*size*CHAPTER_TRACKING_EM,glyph.position().y));
        if let Some(b)=glyph.pixel_bounding_box() {
            let marked=highlight && i==first;
            let weight=if marked {(size*0.025).ceil() as i32} else {0};
            glyph.draw(|gx,gy,a| for dx in 0..=weight {
                blend(&mut image.pixels,image.width,image.height,b.min.x+gx as i32+dx,b.min.y+gy as i32,if marked {RED}else{WHITE},a);
            });
        }
    }
}
fn draw_header(image:&mut Raster,fonts:&ChapterFonts,chapter:usize,size:f32,top:f32) {
    let number=chapter.to_string();let small=size*0.70;
    let(l,r)=bounds(&fonts.chinese,"第",small,false);let(nl,nr)=bounds(&fonts.english,&number,size,false);
    let(zl,zr)=bounds(&fonts.chinese,"章",small,false);
    let gap=size*0.22;let total=r-l+gap+nr-nl+gap+zr-zl;
    let left=(image.width as f32-total)*0.5;
    for (font,text,scale,x,y) in [(&fonts.chinese,"第",small,left-l,top+size*0.15),(&fonts.english,number.as_str(),size,left+(r-l)+gap-nl,top),(&fonts.chinese,"章",small,left+r-l+gap+nr-nl+gap-zl,top+size*0.15)] {
        let ascent=font.v_metrics(Scale::uniform(scale)).ascent;
        for glyph in font.layout(text,Scale::uniform(scale),point(x,y+ascent)) {
            if let Some(b)=glyph.pixel_bounding_box(){glyph.draw(|gx,gy,a|blend(&mut image.pixels,image.width,image.height,b.min.x+gx as i32,b.min.y+gy as i32,WHITE,a));}
        }
    }
}
pub(crate) fn chapter(chapter:usize, chinese:&str,english:&str,all_cn:&[&str],all_en:&[&str],width:u32,max_height:u32)->Raster {
    let fonts=chapter_fonts();
    let pad=(width as f32*0.018).max(2.);let available=width as f32-pad*2.;
    // Use the largest common size that fits every chapter in one line. CN/EN
    // have independent fonts and sizes; the viewport height is the other bound.
    let label=(max_height as f32*0.189).max(16.);
    let gap=max_height as f32*0.035;
    let title_room=(max_height as f32-pad*2.-label-gap*2.).max(2.);
    let cn=fit_all(&fonts.chinese,all_cn,available,title_room*0.57);
    let en=fit_all(&fonts.english,all_en,available,title_room*0.43);
    let height=(pad*2.+label+gap*2.+cn+en).ceil() as u32;
    let mut image=Raster{width,height,pixels:vec![0;width as usize*height as usize*4],first_center:pad+label+gap+cn*0.5};
    draw_header(&mut image,fonts,chapter,label,pad);
    draw_line(&mut image,&fonts.chinese,chinese,cn,pad+label+gap,true);
    draw_line(&mut image,&fonts.english,english,en,pad+label+gap*2.+cn,true);
    image
}

pub(crate) fn clean_interference(text:&str)->(String,bool) {
    let mut result=text.to_owned();let mut marked=false;
    for marker in ["（干扰）","(干扰)","[干扰]","(static)","(Static)"] {
        if result.contains(marker) {marked=true;result=result.replace(marker," ▒▒ ");}
    }
    (result,marked)
}
/// 2.56 seconds: normal, jitter, garble, recover, blink, recover, ghosts.
/// Normal frames share one cache entry. The marker stays in source data only.
pub(crate) fn signal_phase(elapsed_ms:u128)->u32 {
    let p=(elapsed_ms/80%32) as u32;
    if (12..=17).contains(&p)||p==24||p>=29 {p}else{0}
}
pub(crate) fn signal_text(text:&str,phase:u32)->String {
    if !(15..=17).contains(&phase){return text.to_owned();}
    let wide=['▧','□','◇','▒'];let ascii=['#','%','?','@'];
    text.chars().enumerate().map(|(i,c)| {
        if c.is_whitespace()||c.is_ascii_punctuation(){c}
        else if c.is_ascii(){ascii[(i+phase as usize)%4]}
        else{wide[(i+phase as usize)%4]}
    }).collect()
}
/// Receives a transparent text-only raster; the background is composited later.
pub(crate) fn interference(pixels:&mut [u8],width:u32,height:u32,phase:u32) {
    if phase==24 {pixels.fill(0);return;}
    let jitter=(12..=14).contains(&phase);let ghost=(29..=31).contains(&phase);
    if !jitter && !ghost{return;}
    let source=pixels.to_vec();
    let amount=(height as i32/35).clamp(1,4);
    let dx=if phase%2==0 {amount}else{-amount};
    let dy=if phase==13 {1}else{0};
    if jitter{pixels.fill(0);}
    for y in 0..height {
        for x in 0..width {
            let at=(y as usize*width as usize+x as usize)*4;let a=source[at+3] as f32/255.;
            if a==0.{continue;}
            if jitter{blend(pixels,width,height,x as i32+dx,y as i32+dy,[source[at],source[at+1],source[at+2]],a);}
            else{
                blend(pixels,width,height,x as i32+dx*2,y as i32,[100,230,255],a*0.38);
                blend(pixels,width,height,x as i32-dx,y as i32+1,[255,100,110],a*0.26);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn removes_only_signal_markers() {
        assert_eq!(clean_interference("注意（干扰）RIG编号（保持）"),("注意 ▒▒ RIG编号（保持）".into(),true));
        assert_eq!(clean_interference("正常对白"),("正常对白".into(),false));
    }
    #[test] fn every_title_fits_and_only_one_character_is_red() {
        let overlay:serde_json::Value=serde_json::from_str(include_str!("ds_overlay_texts.json")).unwrap();
        let entries=overlay["entries"].as_array().unwrap();
        let title=|n:usize,english:bool| {
            let id=format!("CHAPTER_{n:02}_GAME_TITLE");
            entries.iter().find(|entry|entry["id"]==id).and_then(|entry|entry[if english{"english"}else{"text"}].as_str()).unwrap().to_owned()
        };
        let titles:String=(1..=12).map(|n|title(n,true)).collect::<Vec<_>>().join(" ");
        let chinese:Vec<String>=(1..=12).map(|n|title(n,false)).collect();
        let english:Vec<String>=(1..=12).map(|n|title(n,true)).collect();
        let cn:Vec<&str>=chinese.iter().map(String::as_str).collect();
        let en:Vec<&str>=english.iter().map(String::as_str).collect();
        let f=&chapter_fonts().english;let size=fit_all(f,&en,410.,180.);
        for c in format!("Chapter 0123456789{titles}").chars(){assert_ne!(f.glyph(c).id().0,0,"missing chapter glyph {c}");}
        for c in format!("第章{}",chinese.concat()).chars(){assert_ne!(chapter_fonts().chinese.glyph(c).id().0,0,"missing Chinese glyph {c}");}
        for t in en.iter().copied(){let(l,r)=bounds(f,t,size,true);assert!(r-l<=410.01);}
        assert!(en.iter().any(|t| {let(l,r)=bounds(f,t,size+0.1,true);r-l>410.}));
        let r=chapter(1,&cn[0],&en[0],&cn,&en,440,200);
        assert!(r.height<=201);
        assert!(r.pixels.chunks_exact(4).any(|p| p[0]>180 && p[1]<80 && p[3]>100));
        assert!(r.pixels.chunks_exact(4).any(|p| p[0]>240 && p[1]>240 && p[2]>240 && p[3]>100));
    }
    #[test] fn interference_is_bounded_and_animates() {
        let mut p=vec![255;40*20*4];let original=p.clone();
        interference(&mut p,40,20,0);assert_eq!(p,original);
        interference(&mut p,40,20,12);assert_ne!(p,original);assert_eq!(p.len(),original.len());
        interference(&mut p,40,20,24);assert!(p.iter().all(|v|*v==0));
    }
    #[test] fn signal_sequence_has_readable_recovery_and_distinct_garble() {
        assert_eq!([0,12,15,18,24,25,29,32].map(|p|signal_phase(p*80)),[0,12,15,0,24,0,29,0]);
        let s="通讯中断 ABC";assert_eq!(signal_text(s,0),s);
        assert_ne!(signal_text(s,15),s);assert_ne!(signal_text(s,15),signal_text(s,16));
    }
}
