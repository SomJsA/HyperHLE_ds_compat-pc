//! PC CPU-overlay cache storage follows the portrait readback's row order.
//! This preserves the existing rotation and blending, while writing adjacent
//! destination pixels instead of jumping a whole framebuffer row per pixel.

pub(crate) struct CachedCard {
    pub width: u32,
    pub height: u32,
    pub first_center: f32,
    pub(super) pixels: Vec<u8>,
}

impl CachedCard {
    pub fn new(width: u32, height: u32, first_center: f32, landscape: &[u8]) -> Self {
        assert_eq!(landscape.len(), width as usize * height as usize * 4);
        let mut pixels = vec![0; landscape.len()];
        for x in 0..width as usize {
            for y in 0..height as usize {
                let from = (y * width as usize + x) * 4;
                let to = (x * height as usize + y) * 4;
                pixels[to..to + 4].copy_from_slice(&landscape[from..from + 4]);
            }
        }
        Self { width, height, first_center, pixels }
    }

    pub fn composite(&self, frame: &mut [u8], raw_w: u32, raw_h: u32, left: u32, top: u32, scale: f32, opacity: f32) {
        if frame.len() != raw_w as usize * raw_h as usize * 4 || !(scale > 0.0) { return; }
        if scale == 1.0 && opacity == 1.0 {
            // With byte alpha and opacity=1 the exact result has denominator
            // 255 (never a half-integer). Integer rounding therefore matches
            // the old floating-point blend while avoiding three round() calls
            // and pixel-coordinate divisions for every destination pixel.
            let columns = self.width.min(raw_h.saturating_sub(left));
            let rows = self.height.min(raw_w.saturating_sub(top));
            for x in 0..columns as usize {
                let from = x * self.height as usize * 4;
                let to = ((left as usize + x) * raw_w as usize + top as usize) * 4;
                for (src, dst) in self.pixels[from..from + rows as usize * 4].chunks_exact(4)
                    .zip(frame[to..to + rows as usize * 4].chunks_exact_mut(4)) {
                    let a = src[3] as u32;
                    let inverse = 255 - a;
                    for c in 0..3 { dst[c] = ((src[c] as u32 * a + dst[c] as u32 * inverse + 127) / 255) as u8; }
                    dst[3] = 255;
                }
            }
            return;
        }
        let dw = (self.width as f32 * scale).max(1.0) as u32;
        let dh = (self.height as f32 * scale).max(1.0) as u32;
        for x in 0..dw {
            let sx = (x as f32 / scale) as u32;
            let raw_y = left + x;
            if sx >= self.width || raw_y >= raw_h { continue; }
            for y in 0..dh {
                let sy = (y as f32 / scale) as u32;
                let raw_x = top + y;
                if sy >= self.height || raw_x >= raw_w { continue; }
                let from = ((sx * self.height + sy) * 4) as usize;
                let to = ((raw_y * raw_w + raw_x) * 4) as usize;
                let a = (self.pixels[from + 3] as f32 / 255.0 * opacity).clamp(0.0, 1.0);
                for c in 0..3 {
                    frame[to + c] = (self.pixels[from + c] as f32 * a + frame[to + c] as f32 * (1.0 - a)).round() as u8;
                }
                frame[to + 3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference(frame: &mut [u8], rw: u32, rh: u32, image: &[u8], w: u32, h: u32, left: u32, top: u32, scale: f32, opacity: f32) {
        for y in 0..(h as f32 * scale).max(1.0) as u32 {
            for x in 0..(w as f32 * scale).max(1.0) as u32 {
                let (sx, sy) = ((x as f32 / scale) as u32, (y as f32 / scale) as u32);
                if sx >= w || sy >= h || top + y >= rw || left + x >= rh { continue; }
                let from = ((sy * w + sx) * 4) as usize;
                let to = (((left + x) * rw + top + y) * 4) as usize;
                let a = (image[from + 3] as f32 / 255.0 * opacity).clamp(0.0, 1.0);
                for c in 0..3 { frame[to+c] = (image[from+c] as f32*a + frame[to+c] as f32*(1.0-a)).round() as u8; }
                frame[to+3] = 255;
            }
        }
    }
    #[test]
    fn preserves_rotation_alpha_clipping_and_downscale_byte_exactly() {
        let image: Vec<u8> = (0..13*7*4).map(|i| ((i*37+19)%256) as u8).collect();
        let card = CachedCard::new(13,7,2.5,&image);
        assert_eq!(card.first_center,2.5);
        for scale in [0.37,0.75,1.0] {
            for opacity in [0.0,0.35,1.0] {
                for (left,top) in [(0,0),(2,3),(19,15)] {
                    let mut expected: Vec<u8> = (0..20*21*4).map(|i| ((i*13)%256) as u8).collect();
                    let mut actual = expected.clone();
                    reference(&mut expected,20,21,&image,13,7,left,top,scale,opacity);
                    card.composite(&mut actual,20,21,left,top,scale,opacity);
                    assert_eq!(actual,expected,"scale={scale} alpha={opacity} at={left},{top}");
                }
            }
        }
    }
    #[test]
    fn compare_4k_cached_card_blend_cost() {
        let image: Vec<u8> = (0..1800*300*4).map(|i| ((i*37+19)%256) as u8).collect();
        let card = CachedCard::new(1800,300,40.0,&image);
        let mut frame = vec![127;2160*3840*4];
        let start=std::time::Instant::now();
        for _ in 0..24 { reference(&mut frame,2160,3840,&image,1800,300,100,100,1.0,1.0); }
        let old=start.elapsed();
        let start=std::time::Instant::now();
        for _ in 0..24 { card.composite(&mut frame,2160,3840,100,100,1.0,1.0); }
        let new=start.elapsed();
        println!("4K card blend, 24 frames: previous={old:?}, contiguous={new:?}, ratio={:.3}",old.as_secs_f64()/new.as_secs_f64());
        assert!(frame.iter().any(|v|*v!=127));
    }
}
