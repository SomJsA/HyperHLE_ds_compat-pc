//! Actual GLES2 pixels/state, including an unbound drawable and changed rotations.
use super::*;
use crate::gles::{gles2_on_gl3::GLES2OnGL3Context,gles2_native::GLES2NativeContext,GLESContext};

#[test]
#[ignore="requires owned hidden SDL window and real OpenGL; run serially"]
fn r4_es2_present_rotation_quality_and_guest_state() {
    assert_eq!(std::env::var("TOUCHHLE_DS_BACKGROUND_TEST").as_deref(),Ok("1"));
    let mut win=crate::window::Window::new("R4 ES2 pixels/state",None,None,&Default::default());
    win.set_share_with_current_context(false);
    let mut contexts:[Box<dyn GLESContext>;2]=[Box::new(GLES2OnGL3Context::new(&mut win).unwrap()),Box::new(GLES2NativeContext::new(&mut win).unwrap())];
    let mut transfers=[TransferState::default(),TransferState::default()];
    let mut texts=[crate::ds_host_overlay::GpuState::default(),crate::ds_host_overlay::GpuState::default()];
    let mut presents=[crate::gles::present::PresentState::default(),crate::gles::present::PresentState::default()];
    use crate::gles::gles2_raw as gl;
    let colors=[[255u8,0,0,255],[0,255,0,255],[0,0,255,255],[255,255,0,255]];
    for (key,value) in [("TOUCHHLE_DS_PRESENT_SHARPEN","0.35"),("TOUCHHLE_DS_AA","1"),("TOUCHHLE_DS_SHADOW_LIFT","0"),("TOUCHHLE_DS_SAFE_BLACK","0")] {std::env::set_var(key,value);}
    for mode in [0,1,2] {
        std::env::set_var("TOUCHHLE_DS_PRESENT_FSR",mode.to_string());
        for (index,degrees) in [(0,0),(1,90),(0,180),(1,270),(0,90)] {
            let mut driver=contexts[index].make_current(&mut win);let gles=driver.as_mut();
            unsafe {
                // A freshly selected driver can retain initialization errors;
                // attribute only subsequent errors to this explicit fixture.
                let mut initial=Vec::new();loop {let e=gles.GetError();if e==0 {break;}initial.push(e);}
                if !initial.is_empty() {echo!("[R4-gl-test] pre-fixture errors={:?}",initial);}

                let mut rb=0;gles.GenRenderbuffers(1,&mut rb);gles.BindRenderbuffer(gl::RENDERBUFFER,rb);
                // ES multisample resolves require matching color formats.
                gles.RenderbufferStorage(gl::RENDERBUFFER,0x8058,32,48);
                let mut fbo=0;gles.GenFramebuffers(1,&mut fbo);gles.BindFramebuffer(gl::FRAMEBUFFER,fbo);
                gles.FramebufferRenderbuffer(gl::FRAMEBUFFER,gl::COLOR_ATTACHMENT0,gl::RENDERBUFFER,rb);
                assert_eq!(gles.CheckFramebufferStatus(gl::FRAMEBUFFER),gl::FRAMEBUFFER_COMPLETE);
                let mut msaa_rb=0;let mut msaa_fbo=0;
                gles.GenRenderbuffers(1,&mut msaa_rb);gles.BindRenderbuffer(gl::RENDERBUFFER,msaa_rb);
                gles.RenderbufferStorageMultisampleAPPLE(gl::RENDERBUFFER,4,0x8058,32,48);
                let mut samples=0;gles.GetRenderbufferParameteriv(gl::RENDERBUFFER,0x8cab,&mut samples);
                assert!(samples>=2,"APPLE allocation silently used one sample");
                gles.GenFramebuffers(1,&mut msaa_fbo);gles.BindFramebuffer(gl::FRAMEBUFFER,msaa_fbo);
                gles.FramebufferRenderbuffer(gl::FRAMEBUFFER,gl::COLOR_ATTACHMENT0,gl::RENDERBUFFER,msaa_rb);
                assert_eq!(gles.CheckFramebufferStatus(gl::FRAMEBUFFER),gl::FRAMEBUFFER_COMPLETE);
                gles.Enable(gl::SCISSOR_TEST);gles.ColorMask(1,1,1,1);
                assert_eq!(gles.GetError(),0,"multisample setup");
                for (q,color) in colors.iter().enumerate() {
                    gles.Scissor((q%2) as i32*16,(q/2) as i32*24,16,24);
                    gles.ClearColor(color[0] as f32/255.,color[1] as f32/255.,color[2] as f32/255.,1.);
                    gles.Clear(gl::COLOR_BUFFER_BIT);
                }
                gles.Disable(gl::SCISSOR_TEST);
                gles.BindFramebuffer(gl::READ_FRAMEBUFFER_APPLE,msaa_fbo);
                gles.BindFramebuffer(gl::DRAW_FRAMEBUFFER_APPLE,fbo);
                gles.ResolveMultisampleFramebufferAPPLE();
                assert_eq!(gles.GetError(),0,"matching-format resolve");
                gles.BindFramebuffer(gl::FRAMEBUFFER,fbo);
                let mut app_vbo=0;gles.GenBuffers(1,&mut app_vbo);gles.BindBuffer(gl::ARRAY_BUFFER,app_vbo);
                gles.BufferData(gl::ARRAY_BUFFER,256,std::ptr::null(),gl::STATIC_DRAW);
                let attrs=[6u32,7,14,15];
                for a in attrs {gles.VertexAttribPointer(a,3,gl::FLOAT,gl::FALSE,16,8usize as *const _);gles.EnableVertexAttribArray(a);}
                let before=attrs.map(|a|crate::gles::present_attrib::SavedAttrib::capture(gles,a));
                let mut tex=[0u32;2];gles.GenTextures(2,tex.as_mut_ptr());
                gles.ActiveTexture(gl::TEXTURE0);gles.BindTexture(gl::TEXTURE_2D,tex[0]);
                gles.ActiveTexture(gl::TEXTURE2);gles.BindTexture(gl::TEXTURE_2D,tex[1]);
                gles.BindRenderbuffer(gl::RENDERBUFFER,0);
                gles.ColorMask(1,0,1,0);gles.Viewport(3,5,50,60);
                gles.Enable(gl::SCISSOR_TEST);
                gles.Enable(gl::STENCIL_TEST);
                assert_eq!(gles.GetError(),gl::NO_ERROR,"test setup");
                let rotation=crate::matrix::Matrix::<2>::z_rotation(degrees as f32*std::f32::consts::PI/180.);
                present_renderbuffer_es2(gles,&mut transfers[index],&mut presents[index],&mut texts[index],rb,(0,0,96,64),rotation,None);
                assert_eq!(gles.GetError(),gl::NO_ERROR,"present mode={mode} context={index}");
                for (a,saved) in attrs.into_iter().zip(before) {assert_eq!(crate::gles::present_attrib::SavedAttrib::capture(gles,a),saved,"attribute={a}");}
                let mut active=0;gles.GetIntegerv(gl::ACTIVE_TEXTURE,&mut active);assert_eq!(active,gl::TEXTURE2 as i32);
                let mut bound=0;gles.GetIntegerv(gl::TEXTURE_BINDING_2D,&mut bound);assert_eq!(bound,tex[1] as i32);
                gles.ActiveTexture(gl::TEXTURE0);gles.GetIntegerv(gl::TEXTURE_BINDING_2D,&mut bound);assert_eq!(bound,tex[0] as i32);
                gles.GetIntegerv(gl::RENDERBUFFER_BINDING,&mut bound);assert_eq!(bound,0);
                gles.GetIntegerv(gl::FRAMEBUFFER_BINDING,&mut bound);assert_eq!(bound,fbo as i32);
                let mut mask=[0;4];gles.GetBooleanv(gl::COLOR_WRITEMASK,mask.as_mut_ptr());assert_eq!(mask,[1,0,1,0]);
                assert_ne!(gles.IsEnabled(gl::STENCIL_TEST),0);
                gles.BindFramebuffer(gl::FRAMEBUFFER,0);
                for (x,y) in [(16,16),(80,16),(16,48),(80,48)] {
                    let uv=rotation.transform([(x as f32+0.5)/96.-0.5,(y as f32+0.5)/64.-0.5]);
                    let expected=colors[usize::from(uv[0]>=0.)+2*usize::from(uv[1]>=0.)];
                    let mut pixel=[0u8;4];gles.ReadPixels(x,y,1,1,gl::RGBA,gl::UNSIGNED_BYTE,pixel.as_mut_ptr().cast());
                    for channel in 0..3 {assert!((pixel[channel] as i32-expected[channel] as i32).abs()<36,
                        "mode={mode} context={index} degrees={degrees} xy={x},{y} actual={pixel:?} expected={expected:?}");}
                }
                gles.Disable(gl::STENCIL_TEST);gles.Disable(gl::SCISSOR_TEST);gles.ColorMask(1,1,1,1);
                gles.DeleteFramebuffers(1,&fbo);gles.DeleteRenderbuffers(1,&rb);gles.DeleteBuffers(1,&app_vbo);gles.DeleteTextures(2,tex.as_ptr());
                gles.DeleteFramebuffers(1,&msaa_fbo);gles.DeleteRenderbuffers(1,&msaa_rb);
            }
        }
    }
    for (index,context) in contexts.iter_mut().enumerate() {let mut gl=context.make_current(&mut win);unsafe{transfers[index].release(gl.as_mut());presents[index].release(gl.as_mut());}}
    for key in ["TOUCHHLE_DS_PRESENT_SHARPEN","TOUCHHLE_DS_AA","TOUCHHLE_DS_SHADOW_LIFT","TOUCHHLE_DS_SAFE_BLACK","TOUCHHLE_DS_PRESENT_FSR"] {std::env::remove_var(key);}
}
