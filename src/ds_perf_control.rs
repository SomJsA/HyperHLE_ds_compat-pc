//! Runtime switches for paired performance A/B tests.
use std::sync::atomic::{AtomicU8, Ordering};

const AUTO: u8 = u8::MAX;
static GPU_PRESENT_COPY: AtomicU8 = AtomicU8::new(AUTO);
static TEXTURE_SYNC: AtomicU8 = AtomicU8::new(AUTO);
static GPU_TEXT_OVERLAY: AtomicU8 = AtomicU8::new(AUTO);
static FPS_LIMIT: AtomicU8 = AtomicU8::new(AUTO);
static READBACK_FINISH: AtomicU8 = AtomicU8::new(AUTO);

pub(crate) fn fps_limit(configured: Option<f64>) -> Option<f64> {
    match FPS_LIMIT.load(Ordering::Relaxed) {
        AUTO => configured,
        0 => None,
        fps => Some(f64::from(fps)),
    }
}

pub(crate) fn set_fps_limit(fps: u8) {
    FPS_LIMIT.store(fps, Ordering::Relaxed);
    echo!("[DSperfControl] fps_limit={fps} (0=off)");
}

pub(crate) fn gpu_text_overlay() -> bool {
    match GPU_TEXT_OVERLAY.load(Ordering::Relaxed) {
        AUTO => std::env::var("TOUCHHLE_DS_GPU_TEXT_OVERLAY").as_deref() == Ok("1"),
        state => state == 1,
    }
}

pub(crate) fn set_gpu_text_overlay(enabled: bool) {
    GPU_TEXT_OVERLAY.store(enabled as u8, Ordering::Relaxed);
    echo!("[DSperfControl] gpu_text_overlay={enabled}");
}

pub(crate) fn readback_finish_enabled() -> bool {
    match READBACK_FINISH.load(Ordering::Relaxed) {
        AUTO => std::env::var("TOUCHHLE_DS_SKIP_READBACK_FINISH").as_deref() != Ok("1"),
        state => state == 1,
    }
}

pub(crate) fn set_readback_finish(enabled: bool) {
    READBACK_FINISH.store(enabled as u8, Ordering::Relaxed);
    echo!("[DSperfControl] readback_finish={enabled}");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextureSyncMode {
    Off,
    Flush,
    Finish,
}

impl TextureSyncMode {
    fn code(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Flush => 1,
            Self::Finish => 2,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Off),
            1 => Some(Self::Flush),
            2 => Some(Self::Finish),
            _ => None,
        }
    }
}

pub(crate) fn parse_texture_sync(value: Option<&str>) -> TextureSyncMode {
    match value.map(str::trim) {
        None | Some("") | Some("0") | Some("off") | Some("none") => {
            TextureSyncMode::Off
        }
        Some("flush") => TextureSyncMode::Flush,
        Some(_) => TextureSyncMode::Finish,
    }
}

pub(crate) fn gpu_present_copy() -> bool {
    let state = GPU_PRESENT_COPY.load(Ordering::Relaxed);
    if state != AUTO {
        return state == 1;
    }
    std::env::var("TOUCHHLE_DS_GPU_PRESENT_COPY").as_deref() == Ok("1")
}

pub(crate) fn set_gpu_present_copy(enabled: bool) {
    GPU_PRESENT_COPY.store(enabled as u8, Ordering::Relaxed);
    echo!("[DSperfControl] gpu_present_copy={enabled}");
}

pub(crate) fn texture_upload_sync() -> TextureSyncMode {
    let state = TEXTURE_SYNC.load(Ordering::Relaxed);
    if let Some(mode) = TextureSyncMode::from_code(state) {
        return mode;
    }
    parse_texture_sync(
        std::env::var("TOUCHHLE_DS_TEX_UPLOAD_FINISH")
            .ok()
            .as_deref(),
    )
}

pub(crate) fn set_texture_upload_sync(mode: TextureSyncMode) {
    TEXTURE_SYNC.store(mode.code(), Ordering::Relaxed);
    echo!("[DSperfControl] texture_upload_sync={mode:?}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_sync_modes_are_explicit_and_fail_safe() {
        assert_eq!(parse_texture_sync(None), TextureSyncMode::Off);
        assert_eq!(parse_texture_sync(Some("0")), TextureSyncMode::Off);
        assert_eq!(parse_texture_sync(Some("flush")), TextureSyncMode::Flush);
        assert_eq!(parse_texture_sync(Some("1")), TextureSyncMode::Finish);
        assert_eq!(
            parse_texture_sync(Some("unexpected")),
            TextureSyncMode::Finish
        );
    }
}
