//! Canonical landscape bounds derived from native HUD coordinates, not pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds { pub x:f32, pub y:f32, pub width:f32, pub height:f32 }

pub(crate) fn rect(left:f32,top:f32,width:f32,height:f32,screen_width:f32,screen_height:f32)->Option<Bounds>{
    if ![left,top,width,height,screen_width,screen_height].iter().all(|v|v.is_finite())
        || !(64.0..=4096.0).contains(&screen_width) || !(64.0..=4096.0).contains(&screen_height)
        || width<=0.0 || height<=0.0 {return None;}
    let out=Bounds{x:(left+width*0.5)*480.0/screen_width,y:(top+height*0.5)*320.0/screen_height,
        width:width*480.0/screen_width,height:height*320.0/screen_height};
    if !(0.0..=480.0).contains(&out.x)||!(0.0..=320.0).contains(&out.y)
        || !(1.0..=480.0).contains(&out.width)||!(1.0..=320.0).contains(&out.height){return None;}
    Some(out)
}

pub(crate) fn swipe(x:f32,y:f32,width:f32,height:f32,scale:f32,radians:f32,sw:f32,sh:f32)->Option<Bounds>{
    if ![x,y,width,height,scale,radians].iter().all(|v|v.is_finite())
        || !(0.001..=16.0).contains(&scale) || width<=0.0 || height<=0.0 {return None;}
    let (s,c)=radians.sin_cos();
    let w=(c.abs()*width+s.abs()*height)*scale;
    let h=(s.abs()*width+c.abs()*height)*scale;
    rect(x-w*0.5,y-h*0.5,w,h,sw,sh)
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn native_pixel_tier_and_rotated_arrow_keep_the_same_anchor(){
        let a=swipe(240.,160.,64.,32.,1.,0.,480.,320.).unwrap();
        let b=swipe(480.,320.,128.,64.,1.,0.,960.,640.).unwrap();
        assert_eq!(a,b);
        let rotated=swipe(240.,160.,64.,32.,1.,std::f32::consts::FRAC_PI_2,480.,320.).unwrap();
        assert!((rotated.x-240.).abs()<0.001 && (rotated.y-160.).abs()<0.001);
        assert!((rotated.width-32.).abs()<0.001 && (rotated.height-64.).abs()<0.001);
    }
    #[test] fn tap_layout_rect_normalizes_native_viewport(){
        assert_eq!(rect(416.,256.,128.,128.,960.,640.),Some(Bounds{x:240.,y:160.,width:64.,height:64.}));
    }
    #[test] fn unknown_or_corrupt_geometry_is_rejected(){
        assert!(rect(0.,0.,1.,1.,0.,320.).is_none());
        assert!(rect(-100.,0.,20.,20.,480.,320.).is_none());
        assert!(swipe(f32::NAN,160.,64.,32.,1.,0.,480.,320.).is_none());
        assert!(swipe(240.,160.,64.,32.,0.,0.,480.,320.).is_none());
        assert!(swipe(240.,160.,64.,32.,1.,f32::INFINITY,480.,320.).is_none());
    }
}
