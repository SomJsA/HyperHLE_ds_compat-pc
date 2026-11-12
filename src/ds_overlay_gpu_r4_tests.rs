use super::*;
use super::super::Card;
use crate::gles::{GLESContext,gles1_on_gl2::GLES1OnGL2Context,gles2_native::GLES2NativeContext,gles2_on_gl3::GLES2OnGL3Context};

#[test]
#[ignore="real GL, owned hidden SDL window; run serially"]
fn r4_es2_cards_match_fixed_function_pixels_all_rotations() {
    assert_eq!(std::env::var("TOUCHHLE_DS_BACKGROUND_TEST").as_deref(),Ok("1"));
    FAILED.store(false,Ordering::Relaxed);
    let mut window=crate::window::Window::new("R4 GPU text comparison",None,None,&Default::default());
    window.set_share_with_current_context(false);
    let mut contexts:Vec<Box<dyn GLESContext>>=vec![
        Box::new(GLES1OnGL2Context::new(&mut window).unwrap()),
        Box::new(GLES2NativeContext::new(&mut window).unwrap()),
        Box::new(GLES2OnGL3Context::new(&mut window).unwrap()),
    ];
    let mut states=[State::default(),State::default(),State::default()];
    let mut cards=Vec::new();
    for i in 0..4u32 {
        let (w,h)=(25+i*5,15+i*3);let mut rgba=Vec::new();
        for y in 0..h {for x in 0..w {rgba.extend_from_slice(&[(x*9+i*31) as u8,(y*13+i*71) as u8,(x*3+y*7) as u8,(80+(x+y+i*3)%160) as u8]);}}
        cards.push(Card {image:Arc::new(CachedCard::new(w,h,0.,&rgba)),left:5+i*7,top:8+i*9,
            scale:if i%2==0 {1.0}else{0.75},opacity:if i%3==0 {0.45}else{1.0}});
    }
    for turns in 0..4 {
        let rotation=Matrix::<2>::z_rotation(turns as f32*std::f32::consts::FRAC_PI_2);
        let mut reference=Vec::new();
        for (index,context) in contexts.iter_mut().enumerate() {
            let mut driver=context.make_current(&mut window);let g=driver.as_mut();
            unsafe {
                while g.GetError()!=0 {}
                g.BindFramebufferOES(gl::FRAMEBUFFER_OES,0);g.Viewport(0,0,96,144);
                for cap in [gl::DEPTH_TEST,gl::STENCIL_TEST,gl::CULL_FACE,gl::SCISSOR_TEST] {g.Disable(cap);}
                g.ColorMask(1,1,1,1);g.ClearColor(0.1,0.2,0.3,1.);g.Clear(gl::COLOR_BUFFER_BIT);
                g.ActiveTexture(gl::TEXTURE0);
                if !g.is_es2() {
                    g.ClientActiveTexture(gl::TEXTURE0);g.MatrixMode(gl::PROJECTION);g.LoadIdentity();g.MatrixMode(gl::MODELVIEW);g.LoadIdentity();
                }
                states[index].draw_cards(g,(96,144),rotation,&cards);
                let mut pixels=vec![0u8;96*144*4];g.ReadPixels(0,0,96,144,gl::RGBA,gl::UNSIGNED_BYTE,pixels.as_mut_ptr().cast());
                assert_eq!(g.GetError(),0,"context={index} turn={turns}");
                if index==0 {reference=pixels;}else {
                    let bad=pixels.iter().zip(reference.iter()).filter(|(a,b)|(**a as i32-**b as i32).abs()>2).count();
                    assert_eq!(bad,0,"GPU card placement/color/alpha changed: context={index} turn={turns}");
                }
            }
        }
    }
    for (index,context) in contexts.iter_mut().enumerate() {let mut g=context.make_current(&mut window);unsafe{states[index].release(g.as_mut());}}
}
