//! Pure bounded matrix candidate for explicit original-verified host/shadow mode.
pub(crate) struct Output {
    pub bytes: [u8; 64],
    pub flags: u32,
}
extern "C" {
    fn touchHLE_pc_transform_multiply(dst: *mut f32, a: *const f32, b: *const f32) -> u32;
}
fn ordinary(value: f32) -> bool {
    let bits = value.to_bits() & 0x7fff_ffff;
    bits == 0 || (bits & 0x7f80_0000 != 0 && bits & 0x7f80_0000 != 0x7f80_0000)
}
pub(crate) fn calculate(a: &[f32; 16], b: &[f32; 16]) -> Result<Output, &'static str> {
    if a.iter().chain(b).any(|value| !ordinary(*value)) {
        return Err("fp-input");
    }
    let mut result = [0.0f32; 16];
    // Fixed extents, independent destination, and validated bit classes. The
    // C++ kernel restores the complete caller MXCSR before returning.
    let flags = unsafe {
        touchHLE_pc_transform_multiply(result.as_mut_ptr(), a.as_ptr(), b.as_ptr())
    };
    if flags & 0x8000_0000 != 0 {
        return Err("fp-exception-native-required");
    }
    let mut bytes = [0u8; 64];
    for (index, value) in result.into_iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(Output { bytes, flags })
}

#[cfg(test)]
#[path = "ds_pc_transform_oracle.rs"]
mod tests;
