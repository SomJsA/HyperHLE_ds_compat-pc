//! Bounded, explicit diagnostic reads for original GLES2 fog uniforms.
//! State is owned by the actual EAGL context. Disabled: no GL calls or scans.
use crate::gles::{gles2_raw as gl, GLES};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Default)]
pub(super) struct State {
    enabled: Option<bool>,
    frames: u32,
    samples: u32,
    previous: BTreeMap<u32, [u32; 2]>,
    unlinked: BTreeSet<u32>,
}
impl State {
    pub(super) unsafe fn tick(&mut self, g: &mut dyn GLES) {
        if !*self
            .enabled
            .get_or_insert_with(|| std::env::var("TOUCHHLE_DS_ES2_FOG_PROBE").as_deref() == Ok("1"))
        {
            return;
        }
        self.frames = self.frames.saturating_add(1);
        if self.frames % 30 != 0 || self.samples >= 256 {
            return;
        }
        self.samples += 1;
        let mut linked = 0;
        let mut fogs = 0;
        for program in 1..=512 {
            if g.IsProgram(program) == 0 {
                continue;
            }
            let mut status = 0;
            g.GetProgramiv(program, gl::LINK_STATUS, &mut status);
            if status == 0 {
                if self.unlinked.insert(program) {
                    let mut buffer = [0u8; 1024];
                    let mut length = 0;
                    g.GetProgramInfoLog(
                        program,
                        buffer.len() as i32,
                        &mut length,
                        buffer.as_mut_ptr().cast(),
                    );
                    let n = (length.max(0) as usize).min(buffer.len());
                    log!(
                        "[DSes2-fog-probe] unlinked program={} info={}",
                        program,
                        String::from_utf8_lossy(&buffer[..n])
                    );
                }
                continue;
            }
            linked += 1;
            let mut count = 0;
            g.GetProgramiv(program, gl::ACTIVE_UNIFORMS, &mut count);
            let mut fog = -1;
            let mut colour = -1;
            for index in 0..count.clamp(0, 512) as u32 {
                let mut name = [0u8; 128];
                let mut len = 0;
                let mut size = 0;
                let mut kind = 0;
                g.GetActiveUniform(
                    program,
                    index,
                    name.len() as i32,
                    &mut len,
                    &mut size,
                    &mut kind,
                    name.as_mut_ptr().cast(),
                );
                let n = (len.max(0) as usize).min(name.len());
                if size == 1 && kind == gl::FLOAT_VEC2 && &name[..n] == b"g_Fog" {
                    fog = g.GetUniformLocation(program, b"g_Fog\0".as_ptr().cast());
                }
                if size == 1 && kind == gl::FLOAT_VEC3 && &name[..n] == b"g_FogColour" {
                    colour = g.GetUniformLocation(program, b"g_FogColour\0".as_ptr().cast());
                }
            }
            if fog < 0 {
                continue;
            }
            fogs += 1;
            let mut value = [0f32; 2];
            g.GetUniformfv(program, fog, value.as_mut_ptr());
            let bits = value.map(f32::to_bits);
            if self.previous.insert(program, bits) != Some(bits) {
                let mut rgb = [0f32; 3];
                if colour >= 0 {
                    g.GetUniformfv(program, colour, rgb.as_mut_ptr());
                }
                log!(
                    "[DSes2-fog-probe] frame={} program={} g_Fog={:?} colour={:?}",
                    self.frames,
                    program,
                    value,
                    rgb
                );
            }
        }
        if self.samples == 1 || self.samples % 20 == 0 {
            log!(
                "[DSes2-fog-probe] frame={} linked={} fog_programs={}",
                self.frames,
                linked,
                fogs
            );
        }
    }
}
