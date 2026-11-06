//! Hardware regressions: run explicitly with a hidden owned SDL window.
use super::*;
use crate::gles::{gles1_on_gl2::GLES1OnGL2Context, GLESContext};

fn window() -> crate::window::Window {
    assert_eq!(std::env::var("TOUCHHLE_DS_BACKGROUND_TEST").as_deref(), Ok("1"));
    crate::window::Window::new("present cache regression", None, None, &Default::default())
}

#[test]
#[ignore = "requires real OpenGL and TOUCHHLE_DS_BACKGROUND_TEST=1; run serially"]
fn pc_present_context_shader_belongs_to_current_context() {
    let mut win = window();
    win.set_share_with_current_context(false);
    let mut first = GLES1OnGL2Context::new(&mut win).unwrap();
    let mut second = GLES1OnGL2Context::new(&mut win).unwrap();
    let mut first_state = PresentState::default();
    let mut second_state = PresentState::default();
    unsafe {
        let mut gl = first.make_current(&mut win);
        let program = present_sharpen_program(gl.as_mut(), &mut first_state).expect("first shader");
        assert_ne!(gl.IsProgram(program.program), 0);
        drop(gl);
        let mut gl = second.make_current(&mut win);
        let program = present_sharpen_program(gl.as_mut(), &mut second_state).expect("second shader");
        assert_ne!(gl.IsProgram(program.program), 0, "present shader reused a name from an unrelated GL context");
        assert_eq!(gl.GetError(), gles11::NO_ERROR);
    }
}

#[test]
#[ignore = "requires real OpenGL and TOUCHHLE_DS_BACKGROUND_TEST=1; run serially"]
fn pc_present_context_fsr_attachment_belongs_to_current_context() {
    let mut win = window();
    win.set_share_with_current_context(false);
    let mut first = GLES1OnGL2Context::new(&mut win).unwrap();
    let mut second = GLES1OnGL2Context::new(&mut win).unwrap();
    let mut states = [PresentState::default(), PresentState::default()];
    for (index, width, height) in [(0,96,64), (1,96,64), (0,128,72), (1,96,64), (0,96,64)] {
        let context = if index == 0 { &mut first } else { &mut second };
        let mut gl = context.make_current(&mut win);
        unsafe {
            let target = ensure_present_fsr_target(gl.as_mut(), &mut states[index], width, height).expect("FSR target");
            gl.BindFramebufferOES(gles11::FRAMEBUFFER_OES, target.framebuffer);
            assert_eq!(gl.CheckFramebufferStatusOES(gles11::FRAMEBUFFER_OES), gles11::FRAMEBUFFER_COMPLETE_OES,
                "FSR target reused a name without its context-local color attachment");
            gl.BindFramebufferOES(gles11::FRAMEBUFFER_OES, 0);
            assert_eq!(gl.GetError(), gles11::NO_ERROR);
        }
    }
}

#[test]
#[ignore = "requires real OpenGL and TOUCHHLE_DS_BACKGROUND_TEST=1; run serially"]
fn pc_present_context_rotation_survives_scale_and_fsr_switches() {
    let mut win = window();
    win.set_share_with_current_context(false);
    let mut first = GLES1OnGL2Context::new(&mut win).unwrap();
    let mut second = GLES1OnGL2Context::new(&mut win).unwrap();
    let mut states = [PresentState::default(), PresentState::default()];
    let colors = [[255u8,0,0,255], [0,255,0,255], [0,0,255,255], [255,255,0,255]];
    let rotation = Matrix::<2>::z_rotation(std::f32::consts::FRAC_PI_2);
    std::env::set_var("TOUCHHLE_DS_PRESENT_SHARPEN", "0");
    std::env::set_var("TOUCHHLE_DS_AA", "0");
    for mode in [0,1,2] {
        std::env::set_var("TOUCHHLE_DS_PRESENT_FSR", mode.to_string());
        // Same output, 6x supersampling -> 4x active FSR -> 6x, on two
        // unrelated contexts. Validate actual pixels, not just GL names.
        for (index, width, height) in [(0,96,144), (1,96,144), (0,64,96), (1,64,96), (0,96,144)] {
            let context = if index == 0 { &mut first } else { &mut second };
            let mut gl = context.make_current(&mut win);
            let mut rgba = Vec::new();
            for y in 0..height { for x in 0..width {
                rgba.extend_from_slice(&colors[usize::from(x>=width/2)+2*usize::from(y>=height/2)]);
            }}
            unsafe {
                let mut texture=0;
                gl.GenTextures(1, &mut texture);
                gl.ActiveTexture(gles11::TEXTURE0);
                gl.BindTexture(gles11::TEXTURE_2D, texture);
                gl.TexImage2D(gles11::TEXTURE_2D,0,gles11::RGBA as _,width,height,0,
                    gles11::RGBA,gles11::UNSIGNED_BYTE,rgba.as_ptr().cast());
                for key in [gles11::TEXTURE_MIN_FILTER,gles11::TEXTURE_MAG_FILTER] {
                    gl.TexParameteri(gles11::TEXTURE_2D,key,gles11::LINEAR as _);
                }
                for key in [gles11::TEXTURE_WRAP_S,gles11::TEXTURE_WRAP_T] {
                    gl.TexParameteri(gles11::TEXTURE_2D,key,gles11::CLAMP_TO_EDGE as _);
                }
                gl.BindFramebufferOES(gles11::FRAMEBUFFER_OES,0);
                gl.Enable(gles11::TEXTURE_2D);
                for cap in [gles11::DEPTH_TEST,gles11::STENCIL_TEST,gles11::BLEND,gles11::CULL_FACE,gles11::SCISSOR_TEST] {
                    gl.Disable(cap);
                }
                gl.MatrixMode(gles11::PROJECTION);gl.LoadIdentity();
                gl.MatrixMode(gles11::MODELVIEW);gl.LoadIdentity();
                gl.Color4f(1.0,1.0,1.0,1.0);
                gl.TexEnviv(gles11::TEXTURE_ENV,gles11::TEXTURE_ENV_MODE,[gles11::REPLACE as i32].as_ptr());
                present_frame(gl.as_mut(),&mut states[index],(0,0,96,64),rotation,None,Some((width as _,height as _)),None);
                for (x,y) in [(16,16),(80,16),(16,48),(80,48)] {
                    let uv = rotation.transform([(x as f32+0.5)/96.0-0.5,(y as f32+0.5)/64.0-0.5]);
                    let expected = colors[usize::from(uv[0]>=0.0)+2*usize::from(uv[1]>=0.0)];
                    let mut pixel=[0u8;4];
                    gl.ReadPixels(x,y,1,1,gles11::RGBA,gles11::UNSIGNED_BYTE,pixel.as_mut_ptr().cast());
                    for channel in 0..3 { assert!((pixel[channel] as i32-expected[channel] as i32).abs()<32,
                        "rotation changed: context={index} FSR={mode} source={width}x{height} xy={x},{y} actual={pixel:?} expected={expected:?}"); }
                }
                assert_eq!(gl.GetError(),gles11::NO_ERROR,"context={index} FSR={mode}");
                gl.DeleteTextures(1,&texture);
            }
        }
    }
    std::env::remove_var("TOUCHHLE_DS_PRESENT_FSR");
    std::env::remove_var("TOUCHHLE_DS_PRESENT_SHARPEN");
    std::env::remove_var("TOUCHHLE_DS_AA");
}
