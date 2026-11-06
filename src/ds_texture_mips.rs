//! Straight-alpha RGBA reduction shared by original and replacement textures.
//! Each output pixel retains the original integer/alpha arithmetic. Only
//! independent output rows run in parallel; guest state and GL stay on owner.
use std::sync::OnceLock;
const PARALLEL_OUTPUT_PIXELS: u32 = 1024 * 1024;

fn configured_workers() -> usize {
    static WORKERS: OnceLock<usize> = OnceLock::new();
    *WORKERS.get_or_init(|| {
        let hardware = std::thread::available_parallelism().map_or(1, usize::from);
        let default = hardware.min(if cfg!(target_os = "android") { 4 } else { 8 });
        std::env::var("TOUCHHLE_DS_TEXTURE_WORKERS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(default)
            .clamp(1, hardware.min(32).max(1))
    })
}

pub(crate) fn next_rgba(source: &[u8], width: u32, height: u32) -> Option<(u32, u32, Vec<u8>)> {
    let _scene=crate::ds_scene_diagnostics::Scope::new(crate::ds_scene_diagnostics::Kind::MipStep,width as u64*height as u64,0);
    let pixels = (width / 2)
        .max(1)
        .checked_mul((height / 2).max(1))
        .unwrap_or(0);
    let workers = if pixels >= PARALLEL_OUTPUT_PIXELS {
        configured_workers()
    } else {
        1
    };
    next_rgba_with_workers(source, width, height, workers)
}

fn next_rgba_with_workers(
    source: &[u8],
    width: u32,
    height: u32,
    workers: usize,
) -> Option<(u32, u32, Vec<u8>)> {
    let bytes = width.checked_mul(height)?.checked_mul(4)? as usize;
    if width == 0 || height == 0 || source.len() != bytes {
        return None;
    }
    let nw = (width / 2).max(1);
    let nh = (height / 2).max(1);
    let mut output = vec![0u8; (nw * nh * 4) as usize];
    let workers = workers.max(1).min(nh as usize);
    if workers == 1 {
        reduce_rows(source, width, height, nw, 0, &mut output);
    } else {
        let rows_per_worker = (nh as usize).div_ceil(workers);
        let stride = nw as usize * 4;
        let mut failed = false;
        let mut launched = 0;
        std::thread::scope(|scope| {
            for (index, chunk) in output.chunks_mut(rows_per_worker * stride).enumerate() {
                let task = std::thread::Builder::new()
                    .name(format!("DSmip-{index}"))
                    .stack_size(512 * 1024)
                    .spawn_scoped(scope, move || {
                        reduce_rows(
                            source,
                            width,
                            height,
                            nw,
                            (index * rows_per_worker) as u32,
                            chunk,
                        );
                    });
                if task.is_ok() {
                    launched += 1;
                } else {
                    failed = true;
                }
            }
        });
        if failed {
            // A failed OS thread launch drops its unwritten chunk. After all
            // successfully launched workers join, recompute the whole result.
            reduce_rows(source, width, height, nw, 0, &mut output);
        } else {
            static REPORT: std::sync::Once = std::sync::Once::new();
            REPORT.call_once(|| {
                log!(
                    "[DStexture] alpha-aware mip parallel workers={} output={}x{}",
                    launched,
                    nw,
                    nh
                );
            });
        }
    }
    Some((nw, nh, output))
}

fn reduce_rows(source: &[u8], width: u32, height: u32, nw: u32, first_y: u32, output: &mut [u8]) {
    for (local_y, row) in output.chunks_exact_mut(nw as usize * 4).enumerate() {
        let y = first_y + local_y as u32;
        for x in 0..nw {
            let mut alpha = 0u32;
            let mut weighted = [0u32; 3];
            let mut plain = [0u32; 3];
            let mut count = 0;
            for sy in y * 2..(y * 2 + 2).min(height) {
                for sx in x * 2..(x * 2 + 2).min(width) {
                    let at = ((sy * width + sx) * 4) as usize;
                    let a = u32::from(source[at + 3]);
                    alpha += a;
                    count += 1;
                    for c in 0..3 {
                        let value = u32::from(source[at + c]);
                        weighted[c] += value * a;
                        plain[c] += value;
                    }
                }
            }
            let at = x as usize * 4;
            for c in 0..3 {
                row[at + c] = if alpha != 0 {
                    weighted[c] / alpha
                } else {
                    plain[c] / count
                } as u8;
            }
            row[at + 3] = (alpha / count) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn covered_red_has_no_blue_or_black_fringe() {
        let rgba = [255, 0, 0, 255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0];
        assert_eq!(next_rgba(&rgba, 2, 2).unwrap(), (1, 1, vec![255, 0, 0, 63]));
    }
    #[test]
    fn transparent_rgb_and_thin_axis_survive_without_division_by_zero() {
        assert_eq!(
            next_rgba(&[80, 40, 20, 0, 120, 80, 60, 0], 1, 2).unwrap(),
            (1, 1, vec![100, 60, 40, 0])
        );
        assert_eq!(
            next_rgba(&[80, 40, 20, 255, 120, 80, 60, 255], 2, 1).unwrap(),
            (1, 1, vec![100, 60, 40, 255])
        );
    }
    #[test]
    fn original_complete_chain_matches_prechange_golden_hashes() {
        let mut state = 71u32;
        let mut bytes = vec![0u8; 32 * 16 * 4];
        for byte in &mut bytes {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            *byte = (state >> 24) as u8;
        }
        let (mut w, mut h) = (32, 16);
        for (expected_w, expected_h, expected_hash) in [
            (16, 8, 0xa3c0fd6du32),
            (8, 4, 0xe0421ae4u32),
            (4, 2, 0x0fc2194du32),
            (2, 1, 0xe4ac5ac8u32),
            (1, 1, 0xf88c43eau32),
        ] {
            let (nw, nh, next) = next_rgba(&bytes, w, h).unwrap();
            assert_eq!((nw, nh), (expected_w, expected_h));
            let hash = next.iter().fold(0x811c9dc5u32, |h, b| {
                h.wrapping_mul(0x01000193) ^ u32::from(*b)
            });
            assert_eq!(hash, expected_hash);
            bytes = next;
            w = nw;
            h = nh;
        }
    }
    #[test]
    fn parallel_stripes_preserve_exact_mixed_alpha_and_odd_edges() {
        for (w, h) in [(63, 37), (1, 19), (19, 1), (2053, 1027)] {
            let mut state = 71u32;
            let mut source = vec![0u8; w * h * 4];
            for value in &mut source {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                *value = (state >> 24) as u8;
            }
            let serial = next_rgba_with_workers(&source, w as u32, h as u32, 1).unwrap();
            for workers in [0, 2, 4, 8] {
                assert_eq!(
                    next_rgba_with_workers(&source, w as u32, h as u32, workers).unwrap(),
                    serial
                );
            }
        }
    }
    #[test]
    fn malformed_input_is_rejected() {
        assert!(next_rgba(&[], 0, 1).is_none());
        assert!(next_rgba(&[0; 3], 1, 1).is_none());
        assert!(next_rgba(&[], u32::MAX, u32::MAX).is_none());
    }
}
