//! Opt-in native diagnostic HUD and low-frequency JSONL recording.
//! Desktop GL HUD is drawn in the final present pass, including exclusive
//! fullscreen. It never reads a framebuffer or consumes game input.
use std::sync::atomic::{AtomicI32,Ordering};
static MSAA:AtomicI32=AtomicI32::new(0);
pub(crate) fn actual_msaa(samples:i32){MSAA.store(samples,Ordering::Relaxed);}

/// Records submitted presentation paths, not GPU completion or visual quality.
#[derive(Clone, Copy)]
pub(crate) struct PresentStatus {
    pub fsr_requested: u8,
    pub fsr_eligible: bool,
    pub fsr_final_submitted: bool,
    pub shader_submitted: bool,
    pub source_redundant: bool,
    pub gpu_text_selected: bool,
}

#[cfg(target_os="windows")]
mod desktop {
    use super::*;
    use crate::font::{Font,TextAlignment};
    use crate::gles::{GLES,gles11_raw as gl};
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::{HashMap,HashSet};
    use std::io::Write;
    use std::sync::{Mutex,OnceLock};
    use std::time::{Duration,Instant,SystemTime,UNIX_EPOCH};

    pub fn enabled()->bool {
        static ON:OnceLock<bool>=OnceLock::new();
        *ON.get_or_init(||std::env::var("TOUCHHLE_DS_SUPER_MONITOR").ok().as_deref()==Some("1"))
    }
    #[derive(Default)]
    struct Uploads { keys:HashSet<u32>,last:Option<(u32,u32,u32)> }
    static UPLOADS:OnceLock<Mutex<Uploads>>=OnceLock::new();
    pub fn texture(key:u32,w:u32,h:u32){
        if !enabled(){return}
        if let Ok(mut uploads)=UPLOADS.get_or_init(||Mutex::new(Uploads::default())).lock(){
            uploads.keys.insert(key);uploads.last=Some((key,w,h));
        }
    }

    #[repr(C)] #[derive(Default,Copy,Clone)] struct FileTime {lo:u32,hi:u32}
    impl FileTime {fn ticks(self)->u64{(self.hi as u64)<<32|self.lo as u64}}
    #[repr(C)] #[derive(Default)] struct Memory {cb:u32,faults:u32,peak_working:usize,working:usize,peak_paged:usize,paged:usize,peak_nonpaged:usize,nonpaged:usize,pagefile:usize,peak_pagefile:usize,private:usize}
    #[link(name="kernel32")]
    extern "system" {
        fn GetCurrentProcess()->*mut std::ffi::c_void;
        fn GetProcessTimes(process:*mut std::ffi::c_void,created:*mut FileTime,exited:*mut FileTime,kernel:*mut FileTime,user:*mut FileTime)->i32;
        fn K32GetProcessMemoryInfo(process:*mut std::ffi::c_void,memory:*mut Memory,size:u32)->i32;
    }
    fn process()->(u64,usize,usize){unsafe{
        let (mut created,mut exited,mut kernel,mut user)=(FileTime::default(),FileTime::default(),FileTime::default(),FileTime::default());
        let handle=GetCurrentProcess();
        let ticks=if GetProcessTimes(handle,&mut created,&mut exited,&mut kernel,&mut user)!=0{kernel.ticks()+user.ticks()}else{0};
        let mut memory=Memory::default();memory.cb=std::mem::size_of::<Memory>() as u32;
        let size=memory.cb;K32GetProcessMemoryInfo(handle,&mut memory,size);
        (ticks,memory.working,memory.private)
    }}

    struct Hud {
        font:Option<Font>,updated:Instant,last_cpu:u64,last_time:Instant,
        pixels:Vec<u8>,width:u32,height:u32,revision:u64,
        textures:HashMap<usize,(u32,u64)>,log:Option<std::fs::File>,run:String,
    }
    impl Hud {
        fn new()->Self{
            let stamp=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
            let root=std::env::var("TOUCHHLE_USER_DATA_DIR").unwrap_or_else(|_|".".to_owned());
            let dir=std::path::Path::new(&root).join("diagnostics");let _=std::fs::create_dir_all(&dir);
            let path=dir.join(format!("super-monitor-{stamp}-{}.jsonl",std::process::id()));
            let log=std::fs::OpenOptions::new().write(true).create_new(true).open(&path).ok();
            echo!("[DSmonitor] HUD enabled; snapshots every 500 ms; log={}",path.display());
            Self{font:std::fs::read("touchHLE_fonts/LiberationMono-Regular.ttf").ok().and_then(Font::from_vec),updated:Instant::now()-Duration::from_secs(1),last_cpu:process().0,last_time:Instant::now(),pixels:Vec::new(),width:0,height:0,revision:0,textures:HashMap::new(),log,run:std::env::var("TOUCHHLE_DS_RUN_ID").unwrap_or_else(|_|"unlabelled".to_owned())}
        }
        unsafe fn update(&mut self,viewport:(u32,u32,u32,u32),source:Option<(u32,u32)>,present:PresentStatus){
            let now=Instant::now();
            if now.duration_since(self.updated)<Duration::from_millis(500){return}
            self.updated=now;self.revision+=1;
            let frames=crate::window::ds_monitor_frame_stats();
            let stages=crate::ds_perf::snapshot();
            let submissions=crate::ds_perf::texture_submissions();
            let (cpu,working,private)=process();
            let elapsed=now.duration_since(self.last_time).as_secs_f64().max(0.001);
            let cores=std::thread::available_parallelism().map(|v|v.get()).unwrap_or(1) as f64;
            let cpu_pct=cpu.saturating_sub(self.last_cpu) as f64/10_000_000.0/elapsed/cores*100.0;
            self.last_cpu=cpu;self.last_time=now;
            let (gpu,memory)=crate::gles::gles1_on_gl2::ds_monitor_gpu();
            let (keys,last)=UPLOADS.get().and_then(|u|u.lock().ok().map(|u|(u.keys.len(),u.last))).unwrap_or((0,None));
            let value=json!({"unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),"run":self.run,"frame":crate::window::DS_FRAME_COUNTER.load(Ordering::Relaxed),"fps":frames[0],"frame_mean_ms":frames[1],"frame_p95_ms":frames[2],"frame_p99_ms":frames[3],"frame_max_ms":frames[4],"frames_over_50ms":frames[5],"source_pixels":source,"viewport":viewport,"cpu_process_percent_normalized":cpu_pct,"working_set_bytes":working,"private_bytes":private,"gpu_renderer":gpu,"gpu_global_total_free_mib":memory,"msaa_actual":MSAA.load(Ordering::Relaxed),"msaa_requested":std::env::var("TOUCHHLE_DS_MSAA").ok(),"anisotropy_requested":std::env::var("TOUCHHLE_DS_ANISO").ok(),"hd_unique_upload_keys":keys,"hd_last_upload":last,"cpu_stages_mean_max_total_calls":stages,"texture_sync_submission_schema":1,"texture_sync_submissions":{"off":submissions[0],"flush":submissions[1],"finish":submissions[2]},"present_path_schema":1,"fsr_requested":present.fsr_requested,"fsr_eligible":present.fsr_eligible,"fsr_final_submitted":present.fsr_final_submitted,"present_shader_submitted":present.shader_submitted,"fsr_source_redundant":present.source_redundant,"gpu_copy_requested":crate::ds_perf_control::gpu_present_copy(),"readback_finish_requested":crate::ds_perf_control::readback_finish_enabled() as u8,"gpu_text_requested":crate::ds_perf_control::gpu_text_overlay(),"gpu_text_effective":present.gpu_text_selected,"gpu_text_gate_enabled":crate::ds_host_overlay::gpu_text_gate_enabled(),"host_text_card_submissions":crate::ds_perf::text_card_submissions(),"direct_present_transfer_submissions":crate::ds_perf::present_transfer_submissions(),"shader_timing_scope":"completed-explicit-gles-api-calls","gpu_time_ms":null});
            if let Some(log)=&mut self.log{let _=writeln!(log,"{value}");}
            let memory_text=memory.map(|(total,free)|format!("GPU global free/total {free}/{total} MiB")).unwrap_or_else(||"GPU VRAM counters: unavailable".to_owned());
            let last_text=last.map(|(k,w,h)|format!("HD keys {keys} | last {k:08x} {w}x{h}")).unwrap_or_else(||"HD upload: none observed".to_owned());
            let source=source.map(|(w,h)|format!("{w}x{h}")).unwrap_or_else(||"n/a".to_owned());
            let lines=vec![format!("SUPER MONITOR | {}",self.run.chars().take(36).collect::<String>()),format!("FPS {:.1} | mean {:.1} ms | p99 {:.1} ms",frames[0],frames[1],frames[3]),format!("p95 {:.1} | max {:.1} | >50ms {:.0}",frames[2],frames[4],frames[5]),format!("SOURCE {source} | VIEW {}x{}",viewport.2,viewport.3),format!("CPU {:.1}% | RAM {:.0}/{:.0} MiB",cpu_pct,working as f64/1048576.0,private as f64/1048576.0),format!("GPU {}",gpu.chars().take(44).collect::<String>()),memory_text,format!("MSAA ctx {} | AF {} | FSR req {}/EASU {}",MSAA.load(Ordering::Relaxed),std::env::var("TOUCHHLE_DS_ANISO").unwrap_or_else(|_|"1".to_owned()),present.fsr_requested,if present.fsr_final_submitted {"yes"}else{"no"}),format!("CPU ms read {:.2} text {:.2} CA {:.2} sync {:.2}",stages[0].0,stages[1].0,stages[2].0,stages[6].0),format!("Shader calls C/L {}/{} | max {:.2}/{:.2}ms",stages[4].2,stages[5].2,stages[4].1,stages[5].1),last_text,"GPU time n/a | snapshot 2Hz | log JSONL".to_owned()];
            let font_size=(viewport.3 as f32/95.0).clamp(12.0,23.0);
            let step=font_size*1.4;
            self.width=((font_size*32.0) as u32).min(viewport.2.saturating_sub(24)).max(1);
            self.height=(step*lines.len() as f32+16.0) as u32;
            self.pixels=vec![0;self.width as usize*self.height as usize*4];
            for pixel in self.pixels.chunks_exact_mut(4){pixel.copy_from_slice(&[4,12,18,215]);}
            let Some(font)=&self.font else{return};
            for (line,text) in lines.iter().enumerate(){
                let width=self.width;let height=self.height;let pixels=&mut self.pixels;
                font.draw(font_size,text,(8.0,8.0+step*line as f32),None,TextAlignment::Left,|glyph|{
                    let (gx,gy)=glyph.origin();let (gw,gh)=glyph.dimensions();
                    for y in 0..gh{for x in 0..gw{
                        let (px,py)=(gx.round() as i32+x,gy.round() as i32+y);
                        if px<0||py<0||px>=width as i32||py>=height as i32{continue}
                        let a=glyph.pixel_at((x,y));if a<=0.0{continue}
                        let at=(py as usize*width as usize+px as usize)*4;
                        let old=pixels[at+3] as f32/255.0;let out=a+old*(1.0-a);
                        for c in 0..3{pixels[at+c]=((230.0*a+pixels[at+c] as f32*old*(1.0-a))/out) as u8;}
                        pixels[at+3]=(out*255.0) as u8;
                    }}
                });
            }
        }
    }
    thread_local!{static HUD:RefCell<Option<Hud>>=const{RefCell::new(None)};}

    pub unsafe fn draw(gles:&mut dyn GLES,viewport:(u32,u32,u32,u32),source:Option<(u32,u32)>,present:PresentStatus){
        let value=present.fsr_requested as u64 | (present.fsr_eligible as u64)<<8 | (present.fsr_final_submitted as u64)<<9 | (present.shader_submitted as u64)<<10 | (present.gpu_text_selected as u64)<<11;
        static LAST:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(u64::MAX);
        if LAST.swap(value,Ordering::Relaxed)!=value {crate::ds_diagnostics::event("actual_present_submission_path",[value,viewport.2 as u64,viewport.3 as u64,0]);}
        if !enabled()||gles.is_es2()||viewport.2<32||viewport.3<32{return}
        HUD.with(|cell|{
            let mut cell=cell.borrow_mut();let hud=cell.get_or_insert_with(Hud::new);hud.update(viewport,source,present);
            if hud.pixels.is_empty(){return}
            let context=sdl2::sys::SDL_GL_GetCurrentContext() as usize;
            let mut old_active=0;gles.GetIntegerv(gl::ACTIVE_TEXTURE,&mut old_active);gles.ActiveTexture(gl::TEXTURE0);
            let old_texture={let mut n=0;gles.GetIntegerv(gl::TEXTURE_BINDING_2D,&mut n);n};
            let texture=hud.textures.entry(context).or_insert((0,0));
            if texture.0==0||gles.IsTexture(texture.0)==0{gles.GenTextures(1,&mut texture.0);texture.1=0;}
            gles.BindTexture(gl::TEXTURE_2D,texture.0);
            if texture.1!=hud.revision{
                let mut alignment=0;gles.GetIntegerv(gl::UNPACK_ALIGNMENT,&mut alignment);gles.PixelStorei(gl::UNPACK_ALIGNMENT,1);
                gles.TexParameteri(gl::TEXTURE_2D,gl::TEXTURE_MIN_FILTER,gl::LINEAR as _);
                gles.TexParameteri(gl::TEXTURE_2D,gl::TEXTURE_MAG_FILTER,gl::LINEAR as _);
                gles.TexParameteri(gl::TEXTURE_2D,gl::TEXTURE_WRAP_S,gl::CLAMP_TO_EDGE as _);
                gles.TexParameteri(gl::TEXTURE_2D,gl::TEXTURE_WRAP_T,gl::CLAMP_TO_EDGE as _);
                gles.TexImage2D(gl::TEXTURE_2D,0,gl::RGBA as _,hud.width as _,hud.height as _,0,gl::RGBA,gl::UNSIGNED_BYTE,hud.pixels.as_ptr() as _);
                gles.PixelStorei(gl::UNPACK_ALIGNMENT,alignment);
                texture.1=hud.revision;
            }
            let (vw,vh)=(viewport.2 as f32,viewport.3 as f32);
            let x1=1.0-24.0/vw;let x0=x1-2.0*hud.width as f32/vw;
            let y0=1.0-24.0/vh;let y1=y0-2.0*hud.height as f32/vh;
            let vertices=[x0,y0,x1,y0,x0,y1,x0,y1,x1,y0,x1,y1];
            let uv=[0.0f32,0.0,1.0,0.0,0.0,1.0,0.0,1.0,1.0,0.0,1.0,1.0];
            let blend=gles.IsEnabled(gl::BLEND);let textured=gles.IsEnabled(gl::TEXTURE_2D);
            let (mut src,mut dst)=(0,0);gles.GetIntegerv(gl::BLEND_SRC,&mut src);gles.GetIntegerv(gl::BLEND_DST,&mut dst);
            gles.Enable(gl::TEXTURE_2D);gles.Enable(gl::BLEND);gles.BlendFunc(gl::SRC_ALPHA,gl::ONE_MINUS_SRC_ALPHA);gles.Color4f(1.0,1.0,1.0,1.0);
            gles.EnableClientState(gl::VERTEX_ARRAY);gles.EnableClientState(gl::TEXTURE_COORD_ARRAY);
            gles.VertexPointer(2,gl::FLOAT,0,vertices.as_ptr() as _);gles.TexCoordPointer(2,gl::FLOAT,0,uv.as_ptr() as _);
            gles.DrawArrays(gl::TRIANGLES,0,6);
            gles.BindTexture(gl::TEXTURE_2D,old_texture as _);gles.BlendFunc(src as _,dst as _);
            if blend==0{gles.Disable(gl::BLEND)}if textured==0{gles.Disable(gl::TEXTURE_2D)}
            gles.ActiveTexture(old_active as _);
        });
    }
}
#[cfg(target_os="windows")]
pub(crate) use desktop::draw;
#[cfg(target_os="windows")]
pub(crate) fn texture(key:u32,w:u32,h:u32){
    crate::ds_diagnostics::event("hd_replacement_submission",[key as u64,w as u64,h as u64,0]);
    desktop::texture(key,w,h);
}
#[cfg(not(target_os="windows"))]
pub(crate) fn texture(_key:u32,_w:u32,_h:u32){}
