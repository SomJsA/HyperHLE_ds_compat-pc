//! Observe original dialogue timing; replace only mapped subtitle drawing.
#[path = "ds_subtitle_policy.rs"]
mod policy;
use crate::abi::{CallFromHost, GuestFunction};
use crate::ds_chapter_overlay::{patch, wide, word};
use crate::Environment;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

static READY: AtomicBool = AtomicBool::new(false);
static SNAPSHOT: AtomicU64 = AtomicU64::new(0);
static TIMING:AtomicU64=AtomicU64::new(0);
static TIMING_SERIAL:AtomicU32=AtomicU32::new(0);

pub(crate) fn text_entry(id: &str) -> Option<u32> { policy::entry(id) }

#[derive(Default)]
pub(crate) struct State {
    checked: bool,
    supported: bool,
    suppressed: bool,
    last: Option<(u32, u32, u32, u32)>,
    index: Option<u32>,
    serial: u32,
    track_owner: Option<(u32, u32, u32, u32, u32)>,
    replace_track: bool,
}

fn track_ids(env: &Environment, begin: u32, end: u32) -> Vec<Result<Option<String>, ()>> {
    if policy::valid_segment(begin, end, 0).is_none() { return vec![Err(())]; }
    (0..(end - begin) / 20).map(|index| {
        let address = begin + index * 20;
        let (start, end) = word(env, address).zip(word(env, address + 4)).ok_or(())?;
        if start == end { Ok(None) } else { wide(env, address).map(Some).ok_or(()) }
    }).collect()
}

fn suppress(env: &mut Environment, desired: bool) -> bool {
    let current = env.framework_state.opengles.ds_subtitle.suppressed;
    if current == desired {
        return true;
    }
    if !policy::preflight(policy::SITES.map(|(a, _, _)| word(env, a)), current) {
        echo!("[DSsubtitle-native] preflight failed; custom display withheld");
        return false;
    }
    for (address, original, skip) in policy::SITES {
        patch(env, address, if desired { skip } else { original });
    }
    env.framework_state.opengles.ds_subtitle.suppressed = desired;
    echo!(
        "[DSsubtitle-native] {}",
        if desired { "hidden" } else { "restored" }
    );
    true
}

pub(crate) fn observe(env: &mut Environment) {
    if std::env::var_os("TOUCHHLE_DS_SUBTITLE_OVERLAY").is_none() {
        return;
    }
    if !env.framework_state.opengles.ds_subtitle.checked {
        if !env
            .bundle
            .bundle_identifier()
            .to_ascii_lowercase()
            .contains("deadspace")
        {
            return;
        }
        let supported = policy::preflight(policy::SITES.map(|(a, _, _)| word(env, a)), false);
        let state = &mut env.framework_state.opengles.ds_subtitle;
        state.checked = true;
        state.supported = supported;
        echo!(
            "[DSsubtitle-hook] supported={} entries=original-unreviewed",
            supported
        );
    }
    if !env.framework_state.opengles.ds_subtitle.supported {
        return;
    }
    let hud: u32 =
        GuestFunction::from_addr_and_thumb_flag(0x0003_0474, false).call_from_host(env, ());
    let world = if hud >= 0x1000 {
        word(env, hud.saturating_add(4)).unwrap_or(0)
    } else {
        0
    };
    let track = if world >= 0x1000 {
        word(env, hud.saturating_add(0x1220)).unwrap_or(0)
    } else {
        0
    };
    let segment = if track >= 0x1000 {
        word(env, track.saturating_add(4)).unwrap_or(u32::MAX)
    } else {
        u32::MAX
    };
    let (begin, end) = if track >= 0x1000 {
        word(env, track.saturating_add(0x10)).zip(word(env, track.saturating_add(0x14))).unwrap_or((0,0))
    } else { (0,0) };
    let owner = (hud, world, track, begin, end);
    if env.framework_state.opengles.ds_subtitle.track_owner != Some(owner) {
        let ids = track_ids(env, begin, end);
        let replace_track = policy::replace_track(ids.iter().map(|id| id.as_ref().map(|id| id.as_deref()).map_err(|_| ())));
        let state = &mut env.framework_state.opengles.ds_subtitle;
        state.track_owner = Some(owner);
        state.replace_track = replace_track;
        state.last = None;
        echo!("[DSsubtitle-track] track={track:#x} replace_blank_gaps={replace_track}");
    }
    // Include world ownership so restarting a level cannot reuse an old caption.
    let key = (hud, world, track, segment);
    if env.framework_state.opengles.ds_subtitle.last != Some(key) {
        let id = if track >= 0x1000 {
            word(env, track.saturating_add(0x10))
                .zip(word(env, track.saturating_add(0x14)))
                .and_then(|(begin, end)| policy::valid_segment(begin, end, segment))
                .and_then(|address| wide(env, address))
        } else {
            None
        };
        let index = id.as_deref().and_then(policy::entry);
        let state = &mut env.framework_state.opengles.ds_subtitle;
        state.index = index;
        state.last = Some(key);
        state.serial = state.serial.wrapping_add(1);
        echo!(
            "[DSsubtitle-event] serial={} track={:#x} segment={} id={:?} mapped={}",
            state.serial,
            track,
            segment,
            id,
            index.is_some()
        );
    }
    let state = &env.framework_state.opengles.ds_subtitle;
    let finished=env.mem.get_bytes_fallible(crate::mem::ConstVoidPtr::from_bits(track),1).and_then(|v|v.first()).copied();
    let value=policy::visible_value(state.serial,state.index,finished);
    let desired = (value != 0 || state.replace_track) && READY.load(Ordering::Acquire);
    let patched = suppress(env, desired);
    let timing=if desired&&patched&&value!=0 {
        let duration=word(env,track.saturating_add(0x24)).zip(word(env,track.saturating_add(0x28)))
            .filter(|(a,b)|*a>=0x1000&&b.checked_sub(*a)==end.checked_sub(begin).map(|n|n/20*4))
            .and_then(|(a,_)|a.checked_add(segment*4)).and_then(|a|word(env,a));
        let id = word(env,track.saturating_add(0x10))
            .zip(word(env,track.saturating_add(0x14)))
            .and_then(|(begin,end)| policy::valid_segment(begin,end,segment))
            .and_then(|address| wide(env,address));
        let offset = id.as_deref().map(policy::timing_offset).unwrap_or(0);
        duration.zip(word(env,track.saturating_add(8)))
            .map(|(total,left)| policy::timing_value_with_offset(total,left as i32,offset))
            .unwrap_or(0)
    }else{0};
    TIMING_SERIAL.store(0,Ordering::Release);
    TIMING.store(timing,Ordering::Release);
    TIMING_SERIAL.store((value>>32) as u32,Ordering::Release);
    SNAPSHOT.store(
        if desired && patched { value } else { 0 },
        Ordering::Release,
    );
}

#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_SubtitleOverlay_nativeSnapshot(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> i64 {
    SNAPSHOT.load(Ordering::Acquire) as i64
}

#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_SubtitleOverlay_nativeSetReady(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    ready: u8,
) {
    READY.store(ready != 0, Ordering::Release);
    if ready == 0 {
        SNAPSHOT.store(0, Ordering::Release);
    }
}

pub(crate) fn host_ready(ready:bool) { READY.store(ready, Ordering::Release); }
pub(crate) fn host_snapshot()->u64 { SNAPSHOT.load(Ordering::Acquire) }

/// Timing is accepted only for the exact captured subtitle serial.
pub(crate) fn host_timing(snapshot:u64)->u64 {
    let serial=(snapshot>>32) as u32;
    if snapshot==0||TIMING_SERIAL.load(Ordering::Acquire)!=serial{return 0;}
    let value=TIMING.load(Ordering::Acquire);
    if TIMING_SERIAL.load(Ordering::Acquire)==serial{value}else{0}
}
#[no_mangle]
pub extern "system" fn Java_org_touchhle_android_SubtitleOverlay_nativeTiming(
    _env:*mut std::ffi::c_void,_class:*mut std::ffi::c_void,snapshot:i64,
)->i64 {host_timing(snapshot as u64) as i64}
