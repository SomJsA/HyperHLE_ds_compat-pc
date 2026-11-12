//! Opt-in, present-boundary luminance sampling for the PC flicker investigation.
//!
//! This is diagnostic only. It never changes GL state or pixels. The samples
//! are sent through the existing asynchronous diagnostics writer so a short
//! capture does not add synchronous filesystem I/O to the present path.

use std::sync::OnceLock;

use crate::gles::{gles11_raw, GLES};

pub(crate) fn requested() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("TOUCHHLE_DS_FLICKER_TRACE").as_deref() == Ok("1"))
}

/// Sample a small grid from the completed drawable and emit one event.
///
/// Values are `[mean_luma_x1000, max_luma, min_luma, presented_frame]`.
/// Luma is the integer Rec.709 approximation `(54R + 183G + 19B) / 256`.
pub(crate) unsafe fn sample(gles: &mut dyn GLES) {
    if !requested() || !crate::ds_diagnostics::enabled() {
        return;
    }

    let mut viewport = [0i32; 4];
    gles.GetIntegerv(gles11_raw::VIEWPORT, viewport.as_mut_ptr());
    let (x0, y0, width, height) = (viewport[0], viewport[1], viewport[2], viewport[3]);
    if width <= 0 || height <= 0 || width > 4096 || height > 4096 {
        return;
    }

    const GRID_X: i32 = 5;
    const GRID_Y: i32 = 3;
    let mut sum = 0u64;
    let mut min = u8::MAX;
    let mut max = 0u8;
    let mut count = 0u64;
    for gy in 1..=GRID_Y {
        for gx in 1..=GRID_X {
            let x = x0 + width * gx / (GRID_X + 1);
            let y = y0 + height * gy / (GRID_Y + 1);
            let mut pixel = [0u8; 4];
            gles.ReadPixels(
                x,
                y,
                1,
                1,
                gles11_raw::RGBA,
                gles11_raw::UNSIGNED_BYTE,
                pixel.as_mut_ptr().cast(),
            );
            let luma = ((54u32 * pixel[0] as u32
                + 183u32 * pixel[1] as u32
                + 19u32 * pixel[2] as u32)
                / 256) as u8;
            sum += luma as u64;
            min = min.min(luma);
            max = max.max(luma);
            count += 1;
        }
    }
    let mean_x1000 = sum.saturating_mul(1000) / count.max(1);
    let frame = crate::window::DS_FRAME_COUNTER.load(std::sync::atomic::Ordering::Relaxed);
    crate::ds_diagnostics::event(
        "flicker_luma",
        [mean_x1000, max as u64, min as u64, frame],
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn luma_weighting_keeps_white_above_black() {
        let white = (54u32 * 255 + 183u32 * 255 + 19u32 * 255) / 256;
        assert_eq!(white, 255);
        assert_eq!((54u32 * 0 + 183u32 * 0 + 19u32 * 0) / 256, 0);
    }
}
