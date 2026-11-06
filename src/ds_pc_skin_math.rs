//! Pure SSE2 candidate used by explicit host mode and by shadow validation.
#[path = "ds_pc_skin_strided.rs"]
pub(crate) mod strided;
pub(crate) struct Output {
    pub bytes: Vec<u8>,
    pub flags: u32,
}
extern "C" {
    fn touchHLE_pc_skin_positions(
        dst: *mut f32,
        pos: *const f32,
        wgt: *const f32,
        idx: *const i8,
        mat: *const f32,
        count: u32,
    ) -> u32;
}
fn ordinary(v: f32) -> bool {
    // Bit classification must not depend on the caller's DAZ/rounding controls.
    let bits = v.to_bits() & 0x7fff_ffff;
    bits == 0 || (bits & 0x7f80_0000 != 0 && bits & 0x7f80_0000 != 0x7f80_0000)
}
pub(crate) fn calculate(
    pos: &[f32],
    wgt: &[f32],
    idx: &[i8],
    mat: &[f32],
    count: u32,
) -> Result<Output, &'static str> {
    let n = count as usize;
    if n == 0
        || n > 32768
        || pos.len() != n * 3
        || wgt.len() != n * 4
        || idx.len() != n * 4
        || mat.is_empty()
        || mat.len() % 16 != 0
        || mat.len() > 128 * 16
    {
        return Err("shape");
    }
    if pos.iter().chain(mat).any(|v| !ordinary(*v)) {
        return Err("fp-input");
    }
    for i in 0..n {
        for b in 0..4 {
            let w = wgt[4 * i + b];
            if b != 0 && w.to_bits() & 0x70000000 == 0 {
                break;
            }
            let index = idx[4 * i + b];
            if index < 0 || index as usize >= mat.len() / 16 || !ordinary(w) {
                return Err("weight-index");
            }
        }
    }
    let mut output = vec![0.0f32; n * 3];
    // Every active index and buffer extent validated above; output is independent.
    let flags = unsafe {
        touchHLE_pc_skin_positions(
            output.as_mut_ptr(),
            pos.as_ptr(),
            wgt.as_ptr(),
            idx.as_ptr(),
            mat.as_ptr(),
            count,
        )
    };
    if flags & 0x80000000 != 0 {
        return Err("fp-exception-native-required");
    }
    Ok(Output {
        bytes: output.into_iter().flat_map(f32::to_le_bytes).collect(),
        flags,
    })
}
#[cfg(test)]
#[path = "ds_pc_skin_oracle.rs"]
mod tests;
