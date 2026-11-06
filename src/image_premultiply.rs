//! Exact PNG RGBA premultiplication. Workers own disjoint mutable byte slices;
//! no decoder globals, GL, guest memory, shared Environment or unsafe Send/Sync.
use std::sync::{Mutex,OnceLock};
const PARALLEL_BYTES:usize=4*1024*1024;
#[derive(Copy,Clone,Debug,Default)]
pub(super) struct Work {pub requested:usize,pub spawned:usize,pub fallback_chunks:usize}
fn configured_workers()->usize {
    static WORKERS:OnceLock<usize>=OnceLock::new();
    *WORKERS.get_or_init(|| {
        let hardware=std::thread::available_parallelism().map_or(1,usize::from).clamp(1,32);
        std::env::var("TOUCHHLE_DS_TEXTURE_WORKERS").ok().and_then(|v|v.parse::<usize>().ok())
            .unwrap_or(hardware.min(if cfg!(target_os="android"){4}else{8})).clamp(1,hardware)
    })
}
pub(super) fn apply(pixels:&mut[u8])->Work {with_workers(pixels,configured_workers(),None)}
fn rows(pixels:&mut[u8]) {
    for rgba in pixels.chunks_exact_mut(4) {
        match rgba[3] {
            255=>{}, // The original f32 alpha is exactly1, all RGB bytes unchanged.
            0=>{rgba[0]=0;rgba[1]=0;rgba[2]=0;},
            alpha=>{
                // Preserve the original division/multiplication/truncation.
                // Integer arithmetic and reciprocal multiplication can differ.
                let a=alpha as f32/255.0;
                rgba[0]=(rgba[0] as f32*a) as u8;
                rgba[1]=(rgba[1] as f32*a) as u8;
                rgba[2]=(rgba[2] as f32*a) as u8;
            }
        }
    }
}
fn with_workers(pixels:&mut[u8],workers:usize,fail_at:Option<usize>)->Work {
    assert_eq!(pixels.len()%4,0);
    let workers=workers.clamp(1,32).min((pixels.len()/4).max(1));
    if workers==1 || pixels.len()<PARALLEL_BYTES {rows(pixels);return Work{requested:1,..Work::default()};}
    let pixels_per_chunk=(pixels.len()/4).div_ceil(workers);
    // Keep each unique chunk in a small job slot until its worker takes it.
    // If OS thread creation fails, the caller takes exactly that chunk. Never
    // rerun the whole image: premultiplication is not idempotent for alpha1..254.
    let jobs=pixels.chunks_mut(pixels_per_chunk*4).map(|v|Mutex::new(Some(v))).collect::<Vec<_>>();
    let mut work=Work{requested:jobs.len(),..Work::default()};
    std::thread::scope(|scope| {
        for (index,job) in jobs.iter().enumerate() {
            let task=||{if let Some(chunk)=job.lock().unwrap().take(){rows(chunk);}};
            let launched=fail_at!=Some(index) && std::thread::Builder::new().name("DSrgba".into()).stack_size(512*1024).spawn_scoped(scope,task).is_ok();
            if launched {work.spawned+=1;}
            else {work.fallback_chunks+=1;if let Some(chunk)=job.lock().unwrap().take(){rows(chunk);}}
        }
    });
    work
}

#[cfg(test)] mod tests {
    use super::*;
    fn legacy(pixels:&mut[u8]) {
        let mut i=0;while i<pixels.len(){let a=pixels[i+3] as f32/255.0;pixels[i]=(pixels[i] as f32*a) as u8;pixels[i+1]=(pixels[i+1] as f32*a) as u8;pixels[i+2]=(pixels[i+2] as f32*a) as u8;i+=4;}
    }
    fn pattern(len:usize)->Vec<u8>{(0..len).map(|i|((i*37+i/97)%256)as u8).collect()}
    #[test] fn all_color_alpha_pairs_preserve_legacy_float_rounding_and_alpha(){
        let mut source=Vec::new();for a in 0..=255u8{for c in 0..=255u8{source.extend_from_slice(&[c,c.wrapping_mul(17),255-c,a]);}}
        let mut expected=source.clone();legacy(&mut expected);rows(&mut source);assert_eq!(source,expected);
    }
    #[test] fn parallel_mixed_alpha_and_odd_chunk_boundaries_are_byte_identical(){
        let source=pattern(PARALLEL_BYTES+4*13);let mut expected=source.clone();legacy(&mut expected);
        for workers in [1,2,4,8]{let mut actual=source.clone();let work=with_workers(&mut actual,workers,None);assert_eq!(actual,expected);if workers>1{assert_eq!(work.spawned,work.requested);}}
    }
    #[test] fn failed_worker_processes_only_its_owned_chunk_once(){
        let source=pattern(PARALLEL_BYTES+4*13);let mut expected=source.clone();legacy(&mut expected);
        for fail_at in [0,2,7]{let mut actual=source.clone();let work=with_workers(&mut actual,8,Some(fail_at));assert_eq!(actual,expected);assert_eq!(work.fallback_chunks,1);assert_eq!(work.spawned,7);}
    }
    #[test] fn opaque_and_zero_alpha_keep_exact_bytes(){let mut bytes=vec![3,8,255,255,255,11,98,0];let work=with_workers(&mut bytes,8,None);assert_eq!(bytes,[3,8,255,255,0,0,0,0]);assert_eq!(work.spawned,0);}
    #[test] fn empty_and_zero_requested_workers_are_safe(){assert_eq!(with_workers(&mut[],0,None).requested,1);let mut data=vec![2,9,34,128];let mut expected=data.clone();legacy(&mut expected);with_workers(&mut data,0,None);assert_eq!(data,expected);}
}
