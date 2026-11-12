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

#[test]
#[ignore="requires owned hidden SDL window and real OpenGL; run serially"]
fn r4_es2_zero_range_fog_pixels_preserve_valid_fog() {
    use crate::gles::gles2_raw as gl;
    use crate::gles::gles1_on_gl2::GLES1OnGL2Context;
    assert_eq!(std::env::var("TOUCHHLE_DS_BACKGROUND_TEST").as_deref(),Ok("1"));
    let previous=std::env::var_os("TOUCHHLE_DS_ES2_FOG_FIX");
    let mut win=crate::window::Window::new("R4 fog pixels",None,None,&Default::default());
    win.set_share_with_current_context(false);
    let mut contexts:[Box<dyn GLESContext>;3]=[
        Box::new(GLES2NativeContext::new(&mut win).unwrap()),
        Box::new(GLES2OnGL3Context::new(&mut win).unwrap()),
        Box::new(GLES1OnGL2Context::new(&mut win).unwrap()),
    ];
    for (backend,context) in contexts.iter_mut().enumerate() {
        let mut driver=context.make_current(&mut win);let g=driver.as_mut();
        unsafe {
            while g.GetError()!=0 {}
            let mut rb=0;let mut fb=0;let mut vbo=0;
            let mut white=0;g.GenTextures(1,&mut white);g.ActiveTexture(gl::TEXTURE0);g.BindTexture(gl::TEXTURE_2D,white);
            g.TexParameteri(gl::TEXTURE_2D,gl::TEXTURE_MIN_FILTER,gl::NEAREST as i32);
            g.TexParameteri(gl::TEXTURE_2D,gl::TEXTURE_MAG_FILTER,gl::NEAREST as i32);
            let pixel=[255u8;4];g.TexImage2D(gl::TEXTURE_2D,0,gl::RGBA as i32,1,1,0,gl::RGBA,gl::UNSIGNED_BYTE,pixel.as_ptr().cast());

            g.GenRenderbuffersOES(1,&mut rb);g.BindRenderbufferOES(gl::RENDERBUFFER,rb);
            g.RenderbufferStorageOES(gl::RENDERBUFFER,0x8058,16,16);
            g.GenFramebuffersOES(1,&mut fb);g.BindFramebufferOES(gl::FRAMEBUFFER,fb);
            g.FramebufferRenderbufferOES(gl::FRAMEBUFFER,gl::COLOR_ATTACHMENT0,gl::RENDERBUFFER,rb);
            assert_eq!(g.CheckFramebufferStatusOES(gl::FRAMEBUFFER),gl::FRAMEBUFFER_COMPLETE);
            for cap in [gl::BLEND,gl::DEPTH_TEST,gl::CULL_FACE,gl::SCISSOR_TEST,gl::STENCIL_TEST] {g.Disable(cap);}
            g.ColorMask(1,1,1,1);g.Viewport(0,0,16,16);
            g.GenBuffers(1,&mut vbo);g.BindBuffer(gl::ARRAY_BUFFER,vbo);
            let vertices=[-1f32,-1.,1.,-1.,-1.,1.,1.,1.];
            g.BufferData(gl::ARRAY_BUFFER,32,vertices.as_ptr().cast(),gl::STATIC_DRAW);
            g.EnableVertexAttribArray(0);g.VertexAttribPointer(0,2,gl::FLOAT,0,8,std::ptr::null());
            for enabled in [false,true] {
                std::env::set_var("TOUCHHLE_DS_ES2_FOG_FIX",if enabled {"1"}else{"0"});
                let vertex=format!("{}attribute highp vec2 aPos;\nuniform highp float uZ;\nvarying lowp vec4 vFog;\nvoid main(){{gl_Position=vec4(aPos,0.0,1.0);vFog=Fog(uZ);}}\n",crate::gles::ds_native_fog::ORIGINAL);
                let fragment="precision mediump float;\nvarying lowp vec4 vFog;\nuniform sampler2D g_Texture0;\nvoid main(){vec3 material=vec3(0.8,0.4,0.2)*texture2D(g_Texture0,vec2(0.5)).rgb;gl_FragColor=vec4(vFog.rgb+material*vFog.a,1.0);}";
                let mut shaders=[0;2];
                for (i,(kind,text)) in [(gl::VERTEX_SHADER,vertex.as_str()),(gl::FRAGMENT_SHADER,fragment)].into_iter().enumerate() {
                    shaders[i]=g.CreateShader(kind);let pointer=text.as_ptr().cast();let length=text.len() as i32;
                    g.ShaderSource(shaders[i],1,&pointer,&length);g.CompileShader(shaders[i]);
                    let mut ok=0;g.GetShaderiv(shaders[i],gl::COMPILE_STATUS,&mut ok);assert_ne!(ok,0,"shader backend={backend} enabled={enabled}");
                }
                let program=g.CreateProgram();for shader in shaders {g.AttachShader(program,shader);}
                g.BindAttribLocation(program,0,b"aPos\0".as_ptr().cast());g.LinkProgram(program);
                let mut ok=0;g.GetProgramiv(program,gl::LINK_STATUS,&mut ok);assert_ne!(ok,0,"link backend={backend}");
                for shader in shaders {g.DeleteShader(shader);}
                g.UseProgram(program);
                let fog=g.GetUniformLocation(program,b"g_Fog\0".as_ptr().cast());
                let colour=g.GetUniformLocation(program,b"g_FogColour\0".as_ptr().cast());
                let z=g.GetUniformLocation(program,b"uZ\0".as_ptr().cast());
                assert!(fog>=0 && colour>=0 && z>=0);
                for (range,depth,rgb,expected) in [
                    ([f32::INFINITY,0.],1000.,[0.,0.,0.],if enabled {[204,102,51]}else{[0,0,0]}),
                    ([1./15.,25.],20.,[0.,0.,0.],[68,34,17]),
                    ([1./15.,25.],30.,[0.,0.,0.],[0,0,0]),
                    ([1./15.,25.],30.,[0.1,0.1,0.1],[26,26,26]),
                ] {
                    g.Uniform2f(fog,range[0],range[1]);g.Uniform3f(colour,rgb[0],rgb[1],rgb[2]);g.Uniform1f(z,depth);
                    if backend<2 {
                        let mut read=[0f32;2];g.GetUniformfv(program,fog,read.as_mut_ptr());
                        assert_eq!(read[0].to_bits(),range[0].to_bits());assert_eq!(read[1].to_bits(),range[1].to_bits());
                    }
                    g.DrawArrays(gl::TRIANGLE_STRIP,0,4);let mut pixel=[0u8;4];
                    g.ReadPixels(8,8,1,1,gl::RGBA,gl::UNSIGNED_BYTE,pixel.as_mut_ptr().cast());
                    assert_eq!(g.GetError(),0,"GL backend={backend} enabled={enabled}");
                    for c in 0..3 {assert!((pixel[c] as i32-expected[c]).abs()<=3,
                        "fog backend={backend} enabled={enabled} range={range:?} z={depth} actual={pixel:?} expected={expected:?}");}
                }
                if enabled {
                    g.Uniform2f(fog,1./15.,25.);g.Uniform3f(colour,0.,0.,0.);g.Uniform1f(z,30.);
                    let saved=crate::gles::ds_fog_scope::begin(g,0x7027023c);assert!(saved.is_some());
                    g.DrawArrays(gl::TRIANGLE_STRIP,0,4);
                    crate::gles::ds_fog_scope::end(g,saved);
                    let mut pixel=[0u8;4];g.ReadPixels(8,8,1,1,gl::RGBA,gl::UNSIGNED_BYTE,pixel.as_mut_ptr().cast());
                    for c in 0..3 {assert!((pixel[c] as i32-[204,102,51][c]).abs()<=3,"scoped fog backend={backend}: {pixel:?}");}
                    let marker=g.GetUniformLocation(program,b"touchHLE_DsFogDisabled\0".as_ptr().cast());
                    let mut marker_value=-1;g.GetUniformiv(program,marker,&mut marker_value);assert_eq!(marker_value,0);
                    g.DrawArrays(gl::TRIANGLE_STRIP,0,4);g.ReadPixels(8,8,1,1,gl::RGBA,gl::UNSIGNED_BYTE,pixel.as_mut_ptr().cast());
                    assert!(pixel[..3].iter().all(|v|*v<=3),"valid fog leaked off after scoped draw backend={backend}");
                    assert_eq!(g.GetError(),0,"scoped fog restore backend={backend}");
                } else {
                    assert!(crate::gles::ds_fog_scope::begin(g,0x7027023c).is_none(),"unknown unpatched shader must remain untouched");
                }
                g.UseProgram(0);g.DeleteProgram(program);
            }
            g.DisableVertexAttribArray(0);g.BindFramebufferOES(gl::FRAMEBUFFER,0);
            g.DeleteFramebuffersOES(1,&fb);g.DeleteRenderbuffersOES(1,&rb);g.DeleteBuffers(1,&vbo);g.DeleteTextures(1,&white);
        }
    }
    if let Some(value)=previous {std::env::set_var("TOUCHHLE_DS_ES2_FOG_FIX",value);}else{std::env::remove_var("TOUCHHLE_DS_ES2_FOG_FIX");}
}
