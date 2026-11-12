//! Pure SSE2 candidate used by explicit host mode and by shadow validation.
#[path = "ds_pc_skin_strided.rs"]
pub(crate) mod strided;
pub(crate) struct Output {
    pub bytes: Vec<u8>,
    pub flags: u32,
}

pub(crate) struct OutputView<'a> {
    pub bytes: &'a [u8],
    pub flags: u32,
}

pub(crate) fn pack_bytes<'a>(values: &[f32], bytes: &'a mut Vec<u8>, flags: u32) -> OutputView<'a> {
    bytes.resize(values.len() * 4, 0);
    for (chunk, value) in bytes.chunks_exact_mut(4).zip(values) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    OutputView { bytes, flags }
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
    calculate_validated(pos, wgt, idx, mat, count)
}

/// Run the kernel after the caller has already validated the complete input
/// spans, active indices, finite values, and destination bounds. The shadow
/// capture path performs those checks before borrowing guest memory, so doing
/// them again here only adds CPU work to every skin call.
pub(crate) fn calculate_validated(
    pos: &[f32],
    wgt: &[f32],
    idx: &[i8],
    mat: &[f32],
    count: u32,
) -> Result<Output, &'static str> {
    let mut output = Vec::new();
    calculate_validated_into(pos, wgt, idx, mat, count, &mut output)
}

/// Run the validated packed kernel with caller-owned output storage.
pub(crate) fn calculate_validated_into(
    pos: &[f32],
    wgt: &[f32],
    idx: &[i8],
    mat: &[f32],
    count: u32,
    output: &mut Vec<f32>,
) -> Result<Output, &'static str> {
    output.clear();
    let mut bytes = Vec::new();
    let view = calculate_validated_into_reuse(pos, wgt, idx, mat, count, output, &mut bytes)?;
    Ok(Output {
        bytes: view.bytes.to_vec(),
        flags: view.flags,
    })
}

pub(crate) fn calculate_validated_into_reuse<'a>(
    pos: &[f32],
    wgt: &[f32],
    idx: &[i8],
    mat: &[f32],
    count: u32,
    output: &mut Vec<f32>,
    bytes: &'a mut Vec<u8>,
) -> Result<OutputView<'a>, &'static str> {
    let n = count as usize;
    output.clear();
    output.resize(n * 3, 0.0);
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
    Ok(pack_bytes(output, bytes, flags))
}
#[cfg(test)]
#[path = "ds_pc_skin_oracle.rs"]
mod tests;
