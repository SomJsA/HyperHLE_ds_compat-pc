//! Explicit hardware regressions for the EAGL transfer cache, hidden and serial.
use super::*;
use crate::gles::gles1_on_gl2::GLES1OnGL2Context;
const GL_RGBA4: GLenum = 0x8056;

#[test]
#[ignore = "requires real OpenGL and owned hidden SDL window; run serially"]
fn pc_eagl_transfer_texture_storage_is_local_to_each_context() {
    assert_eq!(std::env::var("TOUCHHLE_DS_BACKGROUND_TEST").as_deref(), Ok("1"));
    let mut win = crate::window::Window::new("EAGL transfer context regression", None, None, &Default::default());
    win.set_share_with_current_context(false);
    let mut contexts = [GLES1OnGL2Context::new(&mut win).unwrap(), GLES1OnGL2Context::new(&mut win).unwrap()];
    let mut states = [TransferState::default(), TransferState::default()];
    for (index, context) in contexts.iter_mut().enumerate() {
        let mut gl = context.make_current(&mut win);
        unsafe {
            gl.BindFramebufferOES(gles11::FRAMEBUFFER_OES, 0);
            gl.Viewport(0, 0, 32, 32);
            gl.ClearColor(if index == 0 {1.0} else {0.0}, if index == 1 {1.0} else {0.0}, 0.0, 1.0);
            gl.Clear(gles11::COLOR_BUFFER_BIT);
            gl.ActiveTexture(gles11::TEXTURE0);
            // This is the exact texture-cache helper used by native presentation.
            let (texture, storage_valid) = prepare_es1_present_texture(gl.as_mut(), &mut states[index], 32, 32);
            if !storage_valid {
                gl.TexImage2D(gles11::TEXTURE_2D,0,gles11::RGBA as _,32,32,0,gles11::RGBA,gles11::UNSIGNED_BYTE,std::ptr::null());
                states[index].es1_texture = Some((texture,32,32));
            }
            let (cached, valid) = prepare_es1_present_texture(gl.as_mut(), &mut states[index], 32, 32);
            assert_eq!(cached, texture);
            assert!(valid, "steady-state storage must remain reusable in its owner context");
            gl.CopyTexSubImage2D(gles11::TEXTURE_2D,0,0,0,0,0,32,32);
            assert_eq!(gl.GetError(), gles11::NO_ERROR, "native transfer reused cached storage from another context; context={index} storage_valid={storage_valid}");
            let mut fbo = 0;
            gl.GenFramebuffersOES(1, &mut fbo);
            gl.BindFramebufferOES(gles11::FRAMEBUFFER_OES, fbo);
            gl.FramebufferTexture2DOES(gles11::FRAMEBUFFER_OES,gles11::COLOR_ATTACHMENT0_OES,gles11::TEXTURE_2D,texture,0);
            assert_eq!(gl.CheckFramebufferStatusOES(gles11::FRAMEBUFFER_OES),gles11::FRAMEBUFFER_COMPLETE_OES);
            let mut pixel = [0u8;4];
            gl.ReadPixels(16,16,1,1,gles11::RGBA,gles11::UNSIGNED_BYTE,pixel.as_mut_ptr().cast());
            assert_eq!(pixel, if index==0 {[255,0,0,255]} else {[0,255,0,255]});
            gl.BindFramebufferOES(gles11::FRAMEBUFFER_OES,0);
            gl.DeleteFramebuffersOES(1,&fbo);
        }
    }
    for (index, context) in contexts.iter_mut().enumerate() {
        let mut gl = context.make_current(&mut win);
        let texture = states[index].es1_texture.unwrap().0;
        unsafe { states[index].release(gl.as_mut()); assert_eq!(gl.IsTexture(texture), 0); }
    }
}

#[test]
#[ignore = "requires real OpenGL and owned hidden SDL window; run serially"]
fn pc_eagl_transfer_shared_objects_keep_helper_attachments_private() {
    assert_eq!(std::env::var("TOUCHHLE_DS_BACKGROUND_TEST").as_deref(), Ok("1"));
    let mut win = crate::window::Window::new("shared EAGL FBO regression", None, None, &Default::default());
    win.set_share_with_current_context(false);
    let mut first = GLES1OnGL2Context::new(&mut win).unwrap();
    { let _gl = first.make_current(&mut win); }
    win.set_share_with_current_context(true);
    let mut second = GLES1OnGL2Context::new(&mut win).unwrap();
    win.set_share_with_current_context(false);
    let mut states = [TransferState::default(), TransferState::default()];
    let mut color_buffers = [0u32;2];
    let first_fbo;
    unsafe {
        let mut gl = first.make_current(&mut win);
        first_fbo = prepare_es1_source_framebuffer(gl.as_mut(), &mut states[0]);
        gl.BindFramebufferOES(gles11::FRAMEBUFFER_OES, first_fbo);
        gl.GenRenderbuffersOES(2, color_buffers.as_mut_ptr());
        for (index, color) in [[1.,0.,0.,1.],[0.,1.,0.,1.]].iter().enumerate() {
            gl.BindRenderbufferOES(gles11::RENDERBUFFER_OES,color_buffers[index]);
            gl.RenderbufferStorageOES(gles11::RENDERBUFFER_OES,GL_RGBA4,32,32);
            gl.FramebufferRenderbufferOES(gles11::FRAMEBUFFER_OES,gles11::COLOR_ATTACHMENT0_OES,gles11::RENDERBUFFER_OES,color_buffers[index]);
            assert_eq!(gl.CheckFramebufferStatusOES(gles11::FRAMEBUFFER_OES),gles11::FRAMEBUFFER_COMPLETE_OES);
            gl.ClearColor(color[0],color[1],color[2],color[3]);gl.Clear(gles11::COLOR_BUFFER_BIT);
        }
        gl.FramebufferRenderbufferOES(gles11::FRAMEBUFFER_OES,gles11::COLOR_ATTACHMENT0_OES,gles11::RENDERBUFFER_OES,color_buffers[0]);
        gl.Finish();
    }
    let guest_fbo;
    unsafe {
        let mut gl = second.make_current(&mut win);
        let mut name=0;gl.GenFramebuffersOES(1,&mut name);guest_fbo=name;
        // EXT FBOs can share objects; generated names need not overlap.
        // Verify attachment isolation rather than assuming an allocator layout.
        echo!("[EAGLtransfer-test] first_fbo={} guest_fbo={} first_visible_in_shared_context={}",
            first_fbo,guest_fbo,gl.IsFramebufferOES(first_fbo));
        gl.BindFramebufferOES(gles11::FRAMEBUFFER_OES,guest_fbo);
        gl.FramebufferRenderbufferOES(gles11::FRAMEBUFFER_OES,gles11::COLOR_ATTACHMENT0_OES,gles11::RENDERBUFFER_OES,color_buffers[0]);
        assert_eq!(gl.CheckFramebufferStatusOES(gles11::FRAMEBUFFER_OES),gles11::FRAMEBUFFER_COMPLETE_OES);
        let own = prepare_es1_source_framebuffer(gl.as_mut(), &mut states[1]);
        assert_ne!(own,guest_fbo,"present helper must not take over a guest FBO from another context's cached name");
        gl.BindFramebufferOES(gles11::FRAMEBUFFER_OES,own);
        gl.FramebufferRenderbufferOES(gles11::FRAMEBUFFER_OES,gles11::COLOR_ATTACHMENT0_OES,gles11::RENDERBUFFER_OES,color_buffers[1]);
        gl.BindFramebufferOES(gles11::FRAMEBUFFER_OES,guest_fbo);
        let mut pixel=[0u8;4];gl.ReadPixels(16,16,1,1,gles11::RGBA,gles11::UNSIGNED_BYTE,pixel.as_mut_ptr().cast());
        assert_eq!(pixel,[255,0,0,255],"native present helper overwrote guest attachment");
    }
    unsafe {
        let mut gl = first.make_current(&mut win);
        gl.BindFramebufferOES(gles11::FRAMEBUFFER_OES,first_fbo);
        let mut pixel=[0u8;4];gl.ReadPixels(16,16,1,1,gles11::RGBA,gles11::UNSIGNED_BYTE,pixel.as_mut_ptr().cast());
        assert_eq!(pixel,[255,0,0,255],"second helper modified first helper attachment");
        states[0].release(gl.as_mut());
        assert_eq!(gl.IsFramebufferOES(first_fbo),0);
    }
    unsafe {
        let mut gl = second.make_current(&mut win);
        assert_ne!(gl.IsFramebufferOES(guest_fbo),0,"releasing first helper must preserve second guest framebuffer");
        states[1].release(gl.as_mut());
        gl.DeleteFramebuffersOES(1,&guest_fbo);gl.DeleteRenderbuffersOES(2,color_buffers.as_ptr());
        assert_eq!(gl.GetError(),gles11::NO_ERROR);
    }
}
