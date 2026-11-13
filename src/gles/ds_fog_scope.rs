//! Guest texture identities belong to a GL sharegroup; bindings to one EAGL
//! context. Rc metadata retains the old object's identity across deletion and
//! numeric-name reuse in another shared context. No per-draw GL identity query.
use crate::gles::{gles2_raw as gl, GLES};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};
#[derive(Default)]
struct Texture {
    target: Cell<u32>,
    key: Cell<Option<u32>>,
}
#[derive(Clone)]
struct Binding {
    name: u32,
    object: Rc<Texture>,
}
#[derive(Default)]
pub(crate) struct Namespace {
    objects: HashMap<u32, Rc<Texture>>,
}
pub(crate) struct GuestState {
    opening_fog: HashMap<u32, crate::gles::ds_opening_fog::OpeningFog>,
    enabled: bool,
    force: bool,
    no_fog: Vec<u32>,
    active: Option<usize>,
    bindings: [Option<Binding>; 8],
    pub(crate) namespace: Rc<RefCell<Namespace>>,
}
impl Default for GuestState {
    fn default() -> Self {
        Self {
            opening_fog: Default::default(),
            enabled: std::env::var("TOUCHHLE_DS_ES2_FOG_FIX").as_deref() == Ok("1")
                && std::env::var("TOUCHHLE_DS_ES2_SCOPED_FOG").as_deref() != Ok("0"),
            force: std::env::var("TOUCHHLE_DS_NO_FOG").as_deref() == Ok("1"),
            no_fog: std::env::var("TOUCHHLE_DS_NO_FOG_HASH")
                .ok()
                .map(|s| crate::ds_texid::parse_keys(&s))
                .unwrap_or_default(),
            active: Some(0),
            bindings: std::array::from_fn(|_| None),
            namespace: Default::default(),
        }
    }
}
impl GuestState {
    pub(crate) fn opening_range(&mut self, renderer:u32, frame:u64, start:f32, end:f32)->Option<(f32,f32)> {
        // Context-owned history, independent of shared texture identities.
        // An unexpected renderer population fails closed and remains bounded.
        if self.opening_fog.len()>=16 && !self.opening_fog.contains_key(&renderer) { return None; }
        self.opening_fog.entry(renderer).or_default().range(frame,start,end)
    }
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }
    pub(crate) fn active(&mut self, unit: u32) {
        if !self.enabled {
            return;
        }
        self.active = unit
            .checked_sub(gl::TEXTURE0)
            .filter(|v| *v < 8)
            .map(|v| v as usize);
        if self.active.is_none() {
            self.bindings = std::array::from_fn(|_| None);
        }
    }
    pub(crate) fn bind(&mut self, target: u32, name: u32) {
        if !self.enabled || !matches!(target, gl::TEXTURE_2D | gl::TEXTURE_CUBE_MAP) {
            return;
        }
        let object = if name == 0 {
            None
        } else {
            let mut ns = self.namespace.borrow_mut();
            let object = ns
                .objects
                .entry(name)
                .or_insert_with(|| {
                    Rc::new(Texture {
                        target: Cell::new(target),
                        key: Cell::new(None),
                    })
                })
                .clone();
            // A name already has an immutable target; GL rejects a mismatch.
            if object.target.get() != 0 && object.target.get() != target {
                return;
            }
            object.target.set(target);
            Some(Binding { name, object })
        };
        if target == gl::TEXTURE_2D {
            if let Some(unit) = self.active {
                self.bindings[unit] = object;
            }
        }
    }
    pub(crate) fn invalidate_upload(&self, target: u32, level: i32) {
        if !self.enabled || target != gl::TEXTURE_2D || level != 0 {
            return;
        }
        if let Some(binding) = self.active.and_then(|unit| self.bindings[unit].as_ref()) {
            binding.object.key.set(None);
        }
    }
    pub(crate) fn uploaded(&mut self, target: u32, name: u32, key: u32) {
        if !self.enabled || target != gl::TEXTURE_2D {
            return;
        }
        if let Some(unit) = self.active {
            if let Some(binding) = &self.bindings[unit] {
                if binding.name == name {
                    binding.object.key.set((key != 0).then_some(key));
                } else {
                    self.bindings[unit] = None;
                }
            }
        }
    }
    pub(crate) fn generated(&mut self, name: u32) {
        if self.enabled && name != 0 {
            self.namespace
                .borrow_mut()
                .objects
                .insert(name, Rc::new(Texture::default()));
        }
    }
    pub(crate) fn delete(&mut self, name: u32) {
        if !self.enabled || name == 0 {
            return;
        }
        self.namespace.borrow_mut().objects.remove(&name);
        for binding in &mut self.bindings {
            if binding.as_ref().is_some_and(|b| b.name == name) {
                *binding = None;
            }
        }
    }
    pub(crate) fn selected_key(&self) -> Option<u32> {
        if !self.enabled {
            return None;
        }
        if self.force {
            return Some(0);
        }
        let key = self.bindings[0].as_ref()?.object.key.get()?;
        self.no_fog.contains(&key).then_some(key)
    }
}
const OVERRIDE_UNIFORM: &[u8] = b"touchHLE_DsFogDisabled\0";
pub(crate) struct Saved {
    program: u32,
    location: i32,
    value: i32,
}
/// Only selected resource content reaches here. The override uniform exists only in the
/// fingerprint-verified shader patch; validate its scalar bool type as well.
pub(crate) unsafe fn begin(g: &mut dyn GLES, key: u32) -> Option<Saved> {
    if g.is_native_es1() {
        return None;
    }
    let mut program = 0;
    g.GetIntegerv(gl::CURRENT_PROGRAM, &mut program);
    if program == 0 {
        return None;
    }
    let program = program as u32;
    let location = g.GetUniformLocation(program, OVERRIDE_UNIFORM.as_ptr().cast());
    if location < 0 {
        return None;
    }
    let mut count = 0;
    g.GetProgramiv(program, gl::ACTIVE_UNIFORMS, &mut count);
    let mut verified = false;
    let mut sampler = false;
    for index in 0..count.clamp(0, 512) as u32 {
        let mut name = [0u8; 128];
        let mut length = 0;
        let mut size = 0;
        let mut kind = 0;
        g.GetActiveUniform(
            program,
            index,
            128,
            &mut length,
            &mut size,
            &mut kind,
            name.as_mut_ptr().cast(),
        );
        let n = (length.max(0) as usize).min(name.len());
        if &name[..n] == &OVERRIDE_UNIFORM[..OVERRIDE_UNIFORM.len() - 1] {
            verified = size == 1 && kind == gl::BOOL;
        }
        if &name[..n] == b"g_Texture0" {
            sampler = size == 1 && kind == gl::SAMPLER_2D;
        }
    }
    if !verified {
        return None;
    }
    if key != 0 {
        if !sampler {
            return None;
        }
        let location = g.GetUniformLocation(program, b"g_Texture0\0".as_ptr().cast());
        if location < 0 {
            return None;
        }
        let mut unit = -1;
        g.GetUniformiv(program, location, &mut unit);
        if unit != 0 {
            return None;
        }
    }
    let mut value = 0;
    g.GetUniformiv(program, location, &mut value);
    g.Uniform1i(location, 1);
    static REPORTED: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
    let bit = match key {
        0x7027023c => 1,
        0x56c62e8a => 2,
        _ => 4,
    };
    if REPORTED.fetch_or(bit, std::sync::atomic::Ordering::Relaxed) & bit == 0 {
        log!(
            "[DSes2-fog-scope] existing no-fog resource rule applied key={:08x} program={}",
            key,
            program
        );
    }
    Some(Saved {
        program,
        location,
        value,
    })
}
pub(crate) unsafe fn end(g: &mut dyn GLES, saved: Option<Saved>) {
    let Some(saved) = saved else {
        return;
    };
    let mut current = 0;
    g.GetIntegerv(gl::CURRENT_PROGRAM, &mut current);
    if current as u32 != saved.program {
        g.UseProgram(saved.program);
    }
    g.Uniform1i(saved.location, saved.value);
    if current as u32 != saved.program {
        g.UseProgram(current as u32);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> GuestState {
        GuestState {
            enabled: true,
            force: false,
            no_fog: vec![0x7027023c, 0x56c62e8a],
            ..Default::default()
        }
    }
    #[test]
    fn shared_objects_keep_identity_when_a_name_is_deleted_and_reused() {
        let mut a = state();
        let mut b = state();
        b.namespace = a.namespace.clone();
        a.bind(gl::TEXTURE_2D, 7);
        a.uploaded(gl::TEXTURE_2D, 7, 0x7027023c);
        b.bind(gl::TEXTURE_2D, 7);
        assert_eq!(b.selected_key(), Some(0x7027023c));
        a.delete(7);
        a.generated(7);
        a.bind(gl::TEXTURE_2D, 7);
        a.uploaded(gl::TEXTURE_2D, 7, 0x1234);
        assert_eq!(a.selected_key(), None);
        assert_eq!(b.selected_key(), Some(0x7027023c));
        b.bind(gl::TEXTURE_2D, 7);
        assert_eq!(b.selected_key(), None);
    }
    #[test]
    fn texture_units_independent_namespaces_and_unknown_uploads_fail_closed() {
        let mut a = state();
        let mut b = state();
        a.bind(gl::TEXTURE_2D, 1);
        a.uploaded(gl::TEXTURE_2D, 1, 0x7027023c);
        b.bind(gl::TEXTURE_2D, 1);
        b.uploaded(gl::TEXTURE_2D, 1, 0x1234);
        assert_eq!(a.selected_key(), Some(0x7027023c));
        assert_eq!(b.selected_key(), None);
        a.active(gl::TEXTURE1);
        a.bind(gl::TEXTURE_2D, 2);
        a.uploaded(gl::TEXTURE_2D, 2, 0x1234);
        assert_eq!(a.selected_key(), Some(0x7027023c));
        a.active(gl::TEXTURE0);
        a.invalidate_upload(gl::TEXTURE_2D, 0);
        assert_eq!(a.selected_key(), None);
    }
}
