//! Context-owned GPU textures for the same cached cards used by CPU fallback.
//! The transposed bitmap and inverse present rotation preserve CPU placement.
use super::{cards, CachedCard};
use crate::gles::{gles11_raw as gl, GLES};
use crate::matrix::Matrix;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};

static FAILED: AtomicBool = AtomicBool::new(false);

pub(super) fn enabled() -> bool {
    cfg!(target_os = "windows")
        && crate::ds_perf_control::gpu_text_overlay()
        && !FAILED.load(Ordering::Relaxed)
}

pub(super) fn observe_backend(fixed_function: bool) {
    if enabled() && !fixed_function {
        fail("present backend has no fixed-function card path");
    }
}

fn fail(reason: &str) {
    if !FAILED.swap(true, Ordering::Relaxed) {
        echo!("[DSoverlay-gpu] {reason}; CPU fallback enabled");
    }
}

#[derive(Default)]
pub(crate) struct State {
    slots: Vec<Slot>,
}

struct Slot {
    texture: u32,
    image: Arc<CachedCard>,
}

// Owned by one actual EAGL context. Shared namespaces may outlive its handle.
impl State {
    /// The owning GL context must be current; never perform GL calls from Drop.
    pub(crate) unsafe fn release(&mut self, gles: &mut dyn GLES) {
        for slot in self.slots.drain(..) { gles.DeleteTextures(1, &slot.texture); }
    }

    pub(crate) unsafe fn draw(&mut self, gles: &mut dyn GLES, size: (u32, u32), rotation: Matrix<2>) {
        let _stutter_span=crate::ds_stutter_probe::Span::new(9,0);
        if !enabled() { return; }
        let Some(inverse) = rotation.inverse() else {
            fail("singular present rotation");
            return;
        };
        let cards = cards(size.0, size.1);
        if cards.is_empty() { return; }
        let mut binding = 0;
        let mut unpack = 0;
        gles.GetIntegerv(gl::TEXTURE_BINDING_2D, &mut binding);
        gles.GetIntegerv(gl::UNPACK_ALIGNMENT, &mut unpack);
        gles.PixelStorei(gl::UNPACK_ALIGNMENT, 1);
        gles.DisableClientState(gl::COLOR_ARRAY);
        gles.EnableClientState(gl::VERTEX_ARRAY);
        gles.EnableClientState(gl::TEXTURE_COORD_ARRAY);
        gles.Enable(gl::TEXTURE_2D);
        gles.Enable(gl::BLEND);
        gles.BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
        gles.MatrixMode(gl::TEXTURE);
        gles.LoadIdentity();
        gles.MatrixMode(gl::MODELVIEW);
        let mode = [gl::MODULATE as i32];
        gles.TexEnviv(gl::TEXTURE_ENV, gl::TEXTURE_ENV_MODE, mode.as_ptr());

        for (index, card) in cards.iter().enumerate() {
            if index == self.slots.len() {
                let mut texture = 0;
                gles.GenTextures(1, &mut texture);
                self.slots.push(Slot { texture, image: Arc::clone(&card.image) });
                gles.BindTexture(gl::TEXTURE_2D, texture);
                for parameter in [gl::TEXTURE_MIN_FILTER, gl::TEXTURE_MAG_FILTER] {
                    gles.TexParameteri(gl::TEXTURE_2D, parameter, gl::NEAREST as _);
                }
                for parameter in [gl::TEXTURE_WRAP_S, gl::TEXTURE_WRAP_T] {
                    gles.TexParameteri(gl::TEXTURE_2D, parameter, gl::CLAMP_TO_EDGE as _);
                }
                if !upload(gles, &card.image, false) { break; }
            } else {
                let slot = &mut self.slots[index];
                gles.BindTexture(gl::TEXTURE_2D, slot.texture);
                if !Arc::ptr_eq(&slot.image, &card.image) {
                    let reuse = slot.image.width == card.image.width && slot.image.height == card.image.height;
                    if !upload(gles, &card.image, reuse) { break; }
                    slot.image = Arc::clone(&card.image);
                }
            }
            let width = (card.image.width as f32 * card.scale).max(1.0) as u32;
            let height = (card.image.height as f32 * card.scale).max(1.0) as u32;
            let vertices = vertices(size, card.left, card.top, width, height, inverse);
            let uv = [0.0f32, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
            gles.Color4f(1.0, 1.0, 1.0, card.opacity);
            gles.VertexPointer(2, gl::FLOAT, 0, vertices.as_ptr().cast());
            gles.TexCoordPointer(2, gl::FLOAT, 0, uv.as_ptr().cast());
            gles.DrawArrays(gl::TRIANGLES, 0, 6);
            crate::ds_perf::text_card_submission(true);
        }
        gles.Color4f(1.0, 1.0, 1.0, 1.0);
        gles.PixelStorei(gl::UNPACK_ALIGNMENT, unpack);
        gles.BindTexture(gl::TEXTURE_2D, binding as _);
    }
}

unsafe fn upload(gles: &mut dyn GLES, image: &CachedCard, reuse: bool) -> bool {
    let _stutter_span=crate::ds_stutter_probe::Span::new(10,((image.width as u64)<<32)|image.height as u64);
    // RGBA rows follow raw portrait readback order: height x width.
    if reuse {
        gles.TexSubImage2D(gl::TEXTURE_2D, 0, 0, 0, image.height as _, image.width as _, gl::RGBA, gl::UNSIGNED_BYTE, image.pixels.as_ptr().cast());
    } else {
        gles.TexImage2D(gl::TEXTURE_2D, 0, gl::RGBA as _, image.height as _, image.width as _, 0, gl::RGBA, gl::UNSIGNED_BYTE, image.pixels.as_ptr().cast());
    }
    let error = gles.GetError();
    if error != gl::NO_ERROR {
        FAILED.store(true, Ordering::Relaxed);
        echo!("[DSoverlay-gpu] upload failed error={error:#x}; CPU fallback next frame");
        false
    } else {
        log_once!("[DSoverlay-gpu] cached text cards active; CPU fallback available");
        true
    }
}

fn vertices(size: (u32, u32), left: u32, top: u32, width: u32, height: u32, inverse: Matrix<2>) -> [f32; 12] {
    let points = [(0, 0), (0, width), (height, 0), (height, 0), (0, width), (height, width)];
    let mut result = [0.0; 12];
    for (index, (x, y)) in points.into_iter().enumerate() {
        let clip = inverse.transform([
            2.0 * (top + x) as f32 / size.0 as f32 - 1.0,
            2.0 * (left + y) as f32 / size.1 as f32 - 1.0,
        ]);
        result[index * 2..index * 2 + 2].copy_from_slice(&clip);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn card_vertices_round_trip_present_rotation() {
        for turns in 0..4 {
            let rotation = Matrix::z_rotation(turns as f32 * std::f32::consts::FRAC_PI_2);
            let actual = vertices((320, 480), 70, 30, 120, 40, rotation.inverse().unwrap());
            for (index, (x, y)) in [(30.0,70.0),(30.0,190.0),(70.0,70.0),(70.0,70.0),(30.0,190.0),(70.0,190.0)].into_iter().enumerate() {
                let source = rotation.transform([actual[index*2], actual[index*2+1]]);
                assert!((source[0] - (2.0*x/320.0-1.0)).abs() < 0.00001);
                assert!((source[1] - (2.0*y/480.0-1.0)).abs() < 0.00001);
            }
        }
    }
}
