//! Preserve the guest's attribute pointers as well as enable bits around host draws.
use super::{GLES, gles2_raw as gl};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SavedAttrib {
    index: u32, enabled: i32, size: i32, kind: i32, normalized: i32,
    stride: i32, buffer: i32, pointer: *mut std::ffi::c_void,
}
impl SavedAttrib {
    pub(crate) unsafe fn capture(gles: &mut dyn GLES, index: u32) -> Self {
        let mut value=Self { index,enabled:0,size:0,kind:0,normalized:0,
            stride:0,buffer:0,pointer:std::ptr::null_mut() };
        for (key, target) in [
            (gl::VERTEX_ATTRIB_ARRAY_ENABLED,&mut value.enabled),
            (gl::VERTEX_ATTRIB_ARRAY_SIZE,&mut value.size),
            (gl::VERTEX_ATTRIB_ARRAY_TYPE,&mut value.kind),
            (gl::VERTEX_ATTRIB_ARRAY_NORMALIZED,&mut value.normalized),
            (gl::VERTEX_ATTRIB_ARRAY_STRIDE,&mut value.stride),
            (gl::VERTEX_ATTRIB_ARRAY_BUFFER_BINDING,&mut value.buffer),
        ] { gles.GetVertexAttribiv(index,key,target); }
        gles.GetVertexAttribPointerv(index,gl::VERTEX_ATTRIB_ARRAY_POINTER,&mut value.pointer);
        value
    }
    pub(crate) unsafe fn restore(self, gles: &mut dyn GLES) {
        gles.BindBuffer(gl::ARRAY_BUFFER,self.buffer as u32);
        // Core GL rejects the default null client pointer. A valid guest
        // pointer or buffer always has to be restored, even if disabled.
        if self.buffer != 0 || !self.pointer.is_null() {
            gles.VertexAttribPointer(self.index,self.size,self.kind as u32,
                self.normalized as u8,self.stride,self.pointer.cast_const());
        }
        if self.enabled != 0 { gles.EnableVertexAttribArray(self.index); }
        else { gles.DisableVertexAttribArray(self.index); }
    }
}
