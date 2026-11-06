//! CPU compatibility presenter storage. Keep the exact RGBA transfer while
//! avoiding a large zero-filled allocation on every frame.
#[derive(Default)]
pub(super) struct RgbaBuffer {
    pixels: Vec<u8>,
}

/// GPU draw -> framebuffer copy -> texture sample stay in one GL command
/// stream. Keep the explicit wait for the CPU compatibility transfer and an
/// opt-in diagnostic fallback; cross-context texture-upload sync is separate.
pub(super) fn needs_finish(gpu_copy: bool, skip_all: bool, force_gpu: bool) -> bool {
    !skip_all && (!gpu_copy || force_gpu)
}

impl RgbaBuffer {
    pub(super) const fn new() -> Self { Self { pixels: Vec::new() } }

    pub(super) fn prepare(&mut self, width: i32, height: i32) -> &mut [u8] {
        assert!(width > 0 && height > 0);
        let length = (width as usize).checked_mul(height as usize)
            .and_then(|value| value.checked_mul(4)).expect("present dimensions overflow");
        self.pixels.resize(length, 0);
        &mut self.pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_size_readback_retains_storage_without_per_frame_clear() {
        let mut buffer = RgbaBuffer::new();
        let pixels = buffer.prepare(640, 960);
        pixels.fill(0x75);
        let pointer = pixels.as_ptr();
        for _ in 0..120 {
            let next = buffer.prepare(640, 960);
            assert_eq!(next.as_ptr(), pointer);
            assert!(next.iter().all(|byte| *byte == 0x75));
        }
    }

    #[test]
    fn resize_and_orientation_use_exact_transfer_length() {
        let mut buffer = RgbaBuffer::new();
        buffer.prepare(320, 480).fill(0xa9);
        assert_eq!(buffer.prepare(480, 320).len(), 320 * 480 * 4);
        let larger = buffer.prepare(640, 960);
        assert_eq!(larger.len(), 640 * 960 * 4);
        assert!(larger[320 * 480 * 4..].iter().all(|byte| *byte == 0));
        assert_eq!(buffer.prepare(1, 1), &[0xa9; 4]);
    }

    #[test]
    fn gpu_queue_dependency_does_not_force_cpu_completion() {
        assert!(needs_finish(false, false, false));
        assert!(!needs_finish(true, false, false));
        assert!(needs_finish(true, false, true));
        assert!(!needs_finish(false, true, false));
        assert!(!needs_finish(true, true, true));
    }
}
