//! Real-driver ownership and pixel regression; diagnostic waits stay in the test.
use super::*;
use crate::gles::{gles1_on_gl2::GLES1OnGL2Context, GLESContext};

#[test]
#[ignore = "requires owned hidden Windows OpenGL contexts; run serially"]
fn gpu_timestamps_keep_queries_and_pixels_in_their_own_contexts() {
    assert_eq!(std::env::var("TOUCHHLE_DS_BACKGROUND_TEST").as_deref(),Ok("1"));
    let mut window=crate::window::Window::new("GPU timestamp ownership test",None,None,&Default::default());
    window.set_share_with_current_context(false);
    let mut contexts=[GLES1OnGL2Context::new(&mut window).unwrap(),GLES1OnGL2Context::new(&mut window).unwrap()];
    let mut profiles=std::array::from_fn::<_,2,_>(|_|State{enabled:true,..State::default()});
    let mut ids=[0;2];
    for i in 0..2 {
        let mut driver=contexts[i].make_current(&mut window);
        unsafe {
            crate::window::DS_FRAME_COUNTER.store(30,Ordering::Relaxed);
            profiles[i].begin(2);
            assert!(profiles[i].supported);
            driver.BindFramebufferOES(crate::gles::gles11_raw::FRAMEBUFFER_OES,0);
            driver.ClearColor(if i==0 {1.}else{0.},if i==1 {1.}else{0.},0.,1.);
            driver.Clear(crate::gles::gles11_raw::COLOR_BUFFER_BIT);
            profiles[i].end(2);
            let mut pixel=[0u8;4];
            // Test-only synchronization proves completed pixels without adding
            // a wait to the production profiler's polling implementation.
            driver.ReadPixels(16,16,1,1,crate::gles::gles11_raw::RGBA,crate::gles::gles11_raw::UNSIGNED_BYTE,pixel.as_mut_ptr().cast());
            assert_eq!(pixel,if i==0 {[255,0,0,255]}else{[0,255,0,255]});
            crate::window::DS_FRAME_COUNTER.store(34,Ordering::Relaxed);
            driver.Flush();
            profiles[i].since=None; // Keep test assertions independent of log reset.
            let deadline=Instant::now()+std::time::Duration::from_secs(2);
            while !profiles[i].pending.is_empty() && Instant::now()<deadline {
                profiles[i].poll();
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert_eq!(profiles[i].pending.len(),0);
            assert_eq!(profiles[i].totals[2].calls,1);
            assert!(profiles[i].totals[2].ns>0);
            assert_eq!(profiles[i].dropped,0);
            ids[i]=profiles[i].free[0][0];
            assert_ne!(gl::IsQuery(ids[i]),0);
            assert_eq!(driver.GetError(),gl::NO_ERROR);
        }
    }
    for i in 0..2 {
        let mut driver=contexts[i].make_current(&mut window);
        unsafe {
            assert_ne!(gl::IsQuery(ids[i]),0,"another context released this query");
            profiles[i].release();
            assert_eq!(gl::IsQuery(ids[i]),0);
            assert_eq!(driver.GetError(),gl::NO_ERROR);
        }
    }
}
