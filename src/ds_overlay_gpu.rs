//! Context-owned GPU textures for the same cached cards used by CPU fallback.
//! The transposed bitmap and inverse present rotation preserve CPU placement.
use super::{cards, CachedCard};
#[path="ds_overlay_gpu_es2.rs"]
mod es2;
use crate::gles::{gles11_raw as gl, GLES};
use crate::matrix::Matrix;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

static FAILED: AtomicBool = AtomicBool::new(false);
static BACKEND_SUPPORTED:AtomicBool=AtomicBool::new(true);

pub(super) fn enabled() -> bool {
    cfg!(target_os = "windows")
        && crate::ds_perf_control::gpu_text_overlay()
        && !FAILED.load(Ordering::Relaxed)
        && BACKEND_SUPPORTED.load(Ordering::Relaxed)
}

pub(super) fn observe_backend(supported:bool) {
    BACKEND_SUPPORTED.store(supported,Ordering::Relaxed);
}

fn fail(reason: &str) {
    if !FAILED.swap(true, Ordering::Relaxed) {
        echo!("[DSoverlay-gpu] {reason}; CPU fallback enabled");
    }
}

#[derive(Default)]
pub(crate) struct State {
    es2:es2::State,
    slots: Vec<Slot>,
    resident_bytes: usize,
    serial: u64,
    uploads: u64,
    uploaded_bytes: u64,
    reported_uploads: u64,
    policy: Option<(bool, bool)>,
}

struct Slot {
    texture: u32,
    image: Arc<CachedCard>,
    used: u64,
}

const CACHE_SLOTS: usize = 16;
const CACHE_BYTES: usize = 32 * 1024 * 1024;

// Owned by one actual EAGL context. Shared namespaces may outlive its handle.
impl State {
    pub(crate) unsafe fn prepare_backend(&mut self,gles:&mut dyn GLES) {
        let requested=cfg!(target_os="windows") && crate::ds_perf_control::gpu_text_overlay();
        let supported=!requested || !gles.is_es2() || self.es2.initialize(gles);
        // Publish current-backend availability; shader failure/cache itself is
        // owned by this State, so another context can still initialize normally.
        observe_backend(supported);
    }
    /// The owning GL context must be current; never perform GL calls from Drop.
    pub(crate) unsafe fn release(&mut self, gles: &mut dyn GLES) {
        self.es2.release(gles);
        for slot in self.slots.drain(..) {
            gles.DeleteTextures(1, &slot.texture);
        }
        self.resident_bytes = 0;
    }

    pub(crate) unsafe fn draw(
        &mut self,
        gles: &mut dyn GLES,
        size: (u32, u32),
        rotation: Matrix<2>,
    ) {
        let _stutter_span = crate::ds_stutter_probe::Span::new(9, 0);
        if !enabled() {
            return;
        }
        self.draw_cards(gles, size, rotation, &cards(size.0, size.1));
    }

    unsafe fn draw_cards(
        &mut self,
        gles: &mut dyn GLES,
        size: (u32, u32),
        rotation: Matrix<2>,
        cards: &[super::Card],
    ) {
        let Some(inverse) = rotation.inverse() else {
            fail("singular present rotation");
            return;
        };
        if cards.is_empty() {
            return;
        }
        self.serial = self.serial.wrapping_add(1);
        let (reuse_cards, trace) = *self.policy.get_or_insert_with(|| {
            (
                std::env::var("TOUCHHLE_DS_GPU_CARD_CACHE").as_deref() != Ok("0"),
                std::env::var("TOUCHHLE_DS_GPU_CARD_CACHE_TRACE").as_deref() == Ok("1"),
            )
        });
        let mut binding = 0;
        let mut unpack = 0;
        gles.GetIntegerv(gl::TEXTURE_BINDING_2D, &mut binding);
        gles.GetIntegerv(gl::UNPACK_ALIGNMENT, &mut unpack);
        gles.PixelStorei(gl::UNPACK_ALIGNMENT, 1);
        let shader_pass=if gles.is_es2() {
            match self.es2.begin(gles) {Some(saved)=>Some(saved),None=>{
                gles.PixelStorei(gl::UNPACK_ALIGNMENT,unpack);gles.BindTexture(gl::TEXTURE_2D,binding as u32);
                observe_backend(false);return;}}
        } else {None};
        gles.Enable(gl::BLEND);gles.BlendFunc(gl::SRC_ALPHA,gl::ONE_MINUS_SRC_ALPHA);
        if shader_pass.is_none() {
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
        }

        for (index, card) in cards.iter().enumerate() {
            let cached = if reuse_cards {
                self.slots
                    .iter()
                    .position(|slot| Arc::ptr_eq(&slot.image, &card.image))
            } else {
                (index < self.slots.len()).then_some(index)
            };
            let slot_index = if let Some(index) = cached {
                index
            } else {
                let bytes = self.resident_bytes;
                let recycle = if reuse_cards
                    && (self.slots.len() >= CACHE_SLOTS
                        || bytes.saturating_add(card.image.pixels.len()) > CACHE_BYTES)
                {
                    self.slots
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| s.used != self.serial)
                        .min_by_key(|(_, s)| s.used)
                        .map(|(i, _)| i)
                } else {
                    None
                };
                if let Some(index) = recycle {
                    index
                } else {
                    let mut texture = 0;
                    gles.GenTextures(1, &mut texture);
                    gles.BindTexture(gl::TEXTURE_2D, texture);
                    for parameter in [gl::TEXTURE_MIN_FILTER, gl::TEXTURE_MAG_FILTER] {
                        gles.TexParameteri(gl::TEXTURE_2D, parameter, gl::NEAREST as _);
                    }
                    for parameter in [gl::TEXTURE_WRAP_S, gl::TEXTURE_WRAP_T] {
                        gles.TexParameteri(gl::TEXTURE_2D, parameter, gl::CLAMP_TO_EDGE as _);
                    }
                    if !upload(gles, &card.image, false) {
                        gles.DeleteTextures(1, &texture);
                        break;
                    }
                    self.record_upload(card.image.pixels.len());
                    self.resident_bytes = self
                        .resident_bytes
                        .saturating_add(card.image.pixels.len());
                    self.slots.push(Slot {
                        texture,
                        image: Arc::clone(&card.image),
                        used: self.serial,
                    });
                    self.slots.len() - 1
                }
            };
            {
                let slot = &mut self.slots[slot_index];
                gles.BindTexture(gl::TEXTURE_2D, slot.texture);
                if !Arc::ptr_eq(&slot.image, &card.image) {
                    let old_bytes = slot.image.pixels.len();
                    let reuse = slot.image.width == card.image.width
                        && slot.image.height == card.image.height;
                    if !upload(gles, &card.image, reuse) {
                        break;
                    }
                    self.uploads += 1;
                    self.uploaded_bytes += card.image.pixels.len() as u64;
                    self.resident_bytes = self
                        .resident_bytes
                        .saturating_sub(old_bytes)
                        .saturating_add(card.image.pixels.len());
                    slot.image = Arc::clone(&card.image);
                }
                slot.used = self.serial;
            }
            let width = (card.image.width as f32 * card.scale).max(1.0) as u32;
            let height = (card.image.height as f32 * card.scale).max(1.0) as u32;
            let vertices = vertices(size, card.left, card.top, width, height, inverse);
            let uv = [
                0.0f32, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0,
            ];
            if shader_pass.is_some() {self.es2.draw(gles,&vertices,card.opacity);} else {
            gles.Color4f(1.0, 1.0, 1.0, card.opacity);
            gles.VertexPointer(2, gl::FLOAT, 0, vertices.as_ptr().cast());
            gles.TexCoordPointer(2, gl::FLOAT, 0, uv.as_ptr().cast());
            gles.DrawArrays(gl::TRIANGLES, 0, 6);
            }
            crate::ds_perf::text_card_submission(true);
        }
        if let Some(saved)=shader_pass {self.es2.end(gles,saved);} else {gles.Color4f(1.0,1.0,1.0,1.0);}
        gles.PixelStorei(gl::UNPACK_ALIGNMENT, unpack);
        gles.BindTexture(gl::TEXTURE_2D, binding as _);
        // Active cards are required for this frame. Retained, inactive cards
        // may never keep the cache above its byte/slot budget.
        let mut bytes = self.resident_bytes;
        while self.slots.len() > CACHE_SLOTS || bytes > CACHE_BYTES {
            let Some(index) = self
                .slots
                .iter()
                .enumerate()
                .filter(|(_, s)| s.used != self.serial)
                .min_by_key(|(_, s)| s.used)
                .map(|(i, _)| i)
            else {
                break;
            };
            let slot = self.slots.swap_remove(index);
            bytes = bytes.saturating_sub(slot.image.pixels.len());
            self.resident_bytes = bytes;
            gles.DeleteTextures(1, &slot.texture);
        }
        if trace {
            if self.reported_uploads != self.uploads {
                self.reported_uploads = self.uploads;
                echo!("[DSoverlay-cache] enabled={} uploads={} uploaded_bytes={} resident_slots={} resident_bytes={}",
                    reuse_cards, self.uploads, self.uploaded_bytes, self.slots.len(), bytes);
            }
        }
    }

    fn record_upload(&mut self, bytes: usize) {
        self.uploads += 1;
        self.uploaded_bytes += bytes as u64;
    }
}

#[cfg(all(test,target_os="windows"))]
#[path="ds_overlay_gpu_r4_tests.rs"]
mod r4_tests;

unsafe fn upload(gles: &mut dyn GLES, image: &CachedCard, reuse: bool) -> bool {
    let _stutter_span =
        crate::ds_stutter_probe::Span::new(10, ((image.width as u64) << 32) | image.height as u64);
    // RGBA rows follow raw portrait readback order: height x width.
    if reuse {
        gles.TexSubImage2D(
            gl::TEXTURE_2D,
            0,
            0,
            0,
            image.height as _,
            image.width as _,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            image.pixels.as_ptr().cast(),
        );
    } else {
        gles.TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RGBA as _,
            image.height as _,
            image.width as _,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            image.pixels.as_ptr().cast(),
        );
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

fn vertices(
    size: (u32, u32),
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    inverse: Matrix<2>,
) -> [f32; 12] {
    let points = [
        (0, 0),
        (0, width),
        (height, 0),
        (height, 0),
        (0, width),
        (height, width),
    ];
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
            for (index, (x, y)) in [
                (30.0, 70.0),
                (30.0, 190.0),
                (70.0, 70.0),
                (70.0, 70.0),
                (30.0, 190.0),
                (70.0, 190.0),
            ]
            .into_iter()
            .enumerate()
            {
                let source = rotation.transform([actual[index * 2], actual[index * 2 + 1]]);
                assert!((source[0] - (2.0 * x / 320.0 - 1.0)).abs() < 0.00001);
                assert!((source[1] - (2.0 * y / 480.0 - 1.0)).abs() < 0.00001);
            }
        }
    }

    #[test]
    #[ignore = "requires owned hidden Windows OpenGL contexts; run serially"]
    fn card_cache_preserves_pixels_order_rotation_and_context_ownership() {
        use super::super::Card;
        use crate::gles::{gles1_on_gl2::GLES1OnGL2Context, GLESContext};
        assert_eq!(
            std::env::var("TOUCHHLE_DS_BACKGROUND_TEST").as_deref(),
            Ok("1")
        );
        let mut window =
            crate::window::Window::new("GPU card cache test", None, None, &Default::default());
        window.set_share_with_current_context(false);
        let mut contexts = [
            GLES1OnGL2Context::new(&mut window).unwrap(),
            GLES1OnGL2Context::new(&mut window).unwrap(),
        ];
        let images: Vec<_> = (0..24u32)
            .map(|i| {
                let (width, height) = (24 + (i % 3) * 4, 12 + (i % 2) * 4);
                let pixels: Vec<u8> = (0..width * height)
                    .flat_map(|at| {
                        [
                            (at * 11 + i * 19) as u8,
                            (at * 3 + i * 41) as u8,
                            (at * 7 + i * 23) as u8,
                            (64 + (at * 5 + i * 13) % 192) as u8,
                        ]
                    })
                    .collect();
                Arc::new(CachedCard::new(width, height, 0.0, &pixels))
            })
            .collect();
        let mut cached = [State::default(), State::default()];
        let mut references = [State::default(), State::default()];
        let mut pixels_checked = 0;
        for turns in 0..4 {
            let rotation = Matrix::z_rotation(turns as f32 * std::f32::consts::FRAC_PI_2);
            for indices in [
                vec![0, 1, 2, 3],
                vec![2, 0],
                vec![3, 1, 0],
                vec![0, 1, 2, 3],
                vec![4, 5, 6, 7],
                (8..24).collect(),
                vec![2, 0, 3, 1],
                vec![0, 1, 2, 3],
            ] {
                let cards: Vec<_> = indices
                    .iter()
                    .enumerate()
                    .map(|(j, i)| Card {
                        image: Arc::clone(&images[*i]),
                        left: 8 + (j as u32 % 5) * 10,
                        top: 6 + (j as u32 % 4) * 7,
                        scale: if j % 2 == 0 { 1.0 } else { 0.75 },
                        opacity: if j % 3 == 0 { 0.45 } else { 1.0 },
                    })
                    .collect();
                for context in 0..2 {
                    let mut gles = contexts[context].make_current(&mut window);
                    unsafe {
                        let mut output = Vec::new();
                        for (cache_on, state) in [
                            (false, &mut references[context]),
                            (true, &mut cached[context]),
                        ] {
                            std::env::set_var(
                                "TOUCHHLE_DS_GPU_CARD_CACHE",
                                if cache_on { "1" } else { "0" },
                            );
                            gles.BindFramebufferOES(gl::FRAMEBUFFER_OES, 0);
                            gles.Viewport(0, 0, 96, 144);
                            for cap in [
                                gl::DEPTH_TEST,
                                gl::STENCIL_TEST,
                                gl::CULL_FACE,
                                gl::SCISSOR_TEST,
                            ] {
                                gles.Disable(cap);
                            }
                            gles.ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
                            gles.ClearColor(0.1, 0.2, 0.3, 1.0);
                            gles.Clear(gl::COLOR_BUFFER_BIT);
                            gles.ActiveTexture(gl::TEXTURE0);
                            gles.ClientActiveTexture(gl::TEXTURE0);
                            gles.MatrixMode(gl::PROJECTION);
                            gles.LoadIdentity();
                            gles.MatrixMode(gl::MODELVIEW);
                            gles.LoadIdentity();
                            state.draw_cards(gles.as_mut(), (96, 144), rotation, &cards);
                            let mut pixels = vec![0u8; 96 * 144 * 4];
                            gles.ReadPixels(
                                0,
                                0,
                                96,
                                144,
                                gl::RGBA,
                                gl::UNSIGNED_BYTE,
                                pixels.as_mut_ptr().cast(),
                            );
                            assert_eq!(gles.GetError(), gl::NO_ERROR);
                            output.push(pixels);
                            assert!(state.slots.len() <= CACHE_SLOTS);
                        }
                        assert_eq!(output[0], output[1], "cache changed pixels: context={context} rotation={turns} cards={indices:?}");
                        pixels_checked += output[0].len() / 4;
                    }
                }
            }
        }
        for i in 0..2 {
            assert!(
                cached[i].uploads < references[i].uploads,
                "cache did not reduce uploads"
            );
            eprintln!("GPU card cache context={i} legacy_uploads={} cached_uploads={} legacy_bytes={} cached_bytes={}",
                references[i].uploads, cached[i].uploads, references[i].uploaded_bytes, cached[i].uploaded_bytes);
            let mut gles = contexts[i].make_current(&mut window);
            unsafe {
                cached[i].release(gles.as_mut());
                references[i].release(gles.as_mut());
                assert!(cached[i].slots.is_empty() && references[i].slots.is_empty());
                assert_eq!(gles.GetError(), gl::NO_ERROR);
            }
        }
        std::env::remove_var("TOUCHHLE_DS_GPU_CARD_CACHE");
        eprintln!("GPU card cache equal pixels checked={pixels_checked}");
    }
}
