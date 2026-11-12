use std::sync::atomic::{AtomicU64, Ordering};

static GUEST_DRAWABLE_DEPTH_CLEAR_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);

/// Record a depth clear on the drawable/MSAA framebuffer. The present path
/// checks the predicted present frame because the frame counter advances at
/// presentRenderbuffer entry, after guest glClear calls for that frame.
pub(crate) fn note_guest_drawable_depth_clear(frame: u64) {
    GUEST_DRAWABLE_DEPTH_CLEAR_FRAME.store(frame, Ordering::Relaxed);
}

pub(crate) fn guest_depth_cleared(frame: u64) -> bool {
    GUEST_DRAWABLE_DEPTH_CLEAR_FRAME.load(Ordering::Relaxed) == frame
}

#[cfg(test)]
mod tests {
    use super::{guest_depth_cleared, note_guest_drawable_depth_clear};

    #[test]
    fn depth_clear_marker_is_frame_scoped() {
        note_guest_drawable_depth_clear(41);
        assert!(guest_depth_cleared(41));
        assert!(!guest_depth_cleared(42));
    }
}
