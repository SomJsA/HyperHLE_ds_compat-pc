//! Small, cached projection panels. Six entry frames; no full-scene effects.
use crate::ds_text_presentation::blend;
pub(crate) fn text_opacity(ms:u128,animated:bool)->f32 {
    if animated {((ms as f32-220.)/160.).clamp(0.,1.)} else {1.}
}
pub(crate) fn panel_phase(ms:u128,animated:bool)->u8 {
    if animated {(ms/50).min(6) as u8}else{6}
}
pub(crate) fn raster(w:u32,h:u32,mode:&str,phase:u8)->Vec<u8> {
    let mut out=vec![0;w as usize*h as usize*4];let progress=(phase as f32/6.).clamp(0.,1.);
    if phase==0{return out;}
    let unit=(h as f32/160.).clamp(1.,4.);let line=unit.ceil() as u32;
    for y in 0..h {for x in 0..w {
        let shown=match mode {
            "minimal"=>true,
            "segmented"=>y as f32<=h as f32*progress,
            _=>(x as f32-w as f32*0.5).abs()<=w as f32*0.5*progress,
        };if !shown{continue;}
        let edge=y<line||y>=h.saturating_sub(line)||((x<line*2||x>=w.saturating_sub(line*2))&&(y<h/6||y>h*5/6));
        let scan=mode!="minimal" && phase<6 && (y as f32-h as f32*progress).abs()<unit;
        let bands=mode!="minimal"&&(y/(line.max(1)))%4==0;
        let color=if edge||scan{[110,211,228]}else{[5,27,38]};
        let alpha=if edge{0.58}else if scan{0.64}else if bands{0.58}else{0.50};
        blend(&mut out,w,h,x as i32,y as i32,color,alpha*if mode=="minimal"{progress}else{1.});
    }}out
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn panel_leads_text_then_settles(){assert_eq!(text_opacity(200,true),0.);assert!(panel_phase(200,true)>0);assert_eq!(text_opacity(400,true),1.);assert_eq!(panel_phase(50_000,true),6);assert_eq!(panel_phase(0,false),6);}
    #[test]fn empty_and_stable_frames_are_bounded(){assert!(raster(40,20,"scan",0).iter().all(|v|*v==0));let p=raster(40,20,"scan",6);assert_eq!(p.len(),3200);assert!(p.chunks_exact(4).all(|p|p[3]<200));}
}
