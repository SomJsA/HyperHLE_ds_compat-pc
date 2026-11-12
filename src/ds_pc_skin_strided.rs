//! Bounded borrowed inputs for the synchronous CPU skin kernel. No guest writes.
use super::{Output, OutputView};

pub(crate) struct Inputs<'a> {
    pub positions: &'a [u8],
    pub position_stride: usize,
    pub weights: &'a [u8],
    pub weight_stride: usize,
    pub indices: &'a [u8],
    pub index_stride: usize,
    pub matrices: &'a [u8],
    pub count: u32,
}

extern "C" {
    fn touchHLE_pc_skin_positions_strided(
        dst: *mut f32,
        pos: *const u8,
        pos_stride: u32,
        wgt: *const u8,
        wgt_stride: u32,
        idx: *const i8,
        idx_stride: u32,
        mat: *const u8,
        count: u32,
    ) -> u32;
}

fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}
fn ordinary(bits: u32) -> bool {
    let bits = bits & 0x7fff_ffff;
    bits == 0 || (bits & 0x7f80_0000 != 0 && bits & 0x7f80_0000 != 0x7f80_0000)
}
fn shape(data: &[u8], stride: usize, components: usize, max: usize, n: usize) -> bool {
    n > 0
        && n <= 32768
        && stride >= components
        && stride <= max
        && (components <= 4 || stride % 4 == 0)
        && data.len() == (n - 1) * stride + components
}

// Needed before borrowing the exact palette span. No float operations, input
// copies or allocation, and no read of inactive bone indices past termination.
pub(crate) fn palette_matrices(
    weights: &[u8],
    weight_stride: usize,
    indices: &[u8],
    index_stride: usize,
    count: u32,
) -> Result<usize, &'static str> {
    let n = count as usize;
    if !shape(weights, weight_stride, 16, 64, n) || !shape(indices, index_stride, 4, 16, n) {
        return Err("strided-weight-index-shape");
    }
    let mut maximum = 0usize;
    for i in 0..n {
        for b in 0..4 {
            let bits = word(weights, i * weight_stride + b * 4);
            if b != 0 && bits & 0x7000_0000 == 0 {
                break;
            }
            let index = indices[i * index_stride + b] as i8;
            if index < 0 || !ordinary(bits) {
                return Err("strided-weight-index");
            }
            maximum = maximum.max(index as usize);
        }
    }
    Ok(maximum + 1)
}

pub(crate) fn calculate(input: &Inputs<'_>) -> Result<Output, &'static str> {
    let n = input.count as usize;
    if !shape(input.positions, input.position_stride, 12, 64, n)
        || input.matrices.is_empty()
        || input.matrices.len() % 64 != 0
        || input.matrices.len() > 128 * 64
    {
        return Err("strided-shape");
    }
    let matrices = palette_matrices(
        input.weights,
        input.weight_stride,
        input.indices,
        input.index_stride,
        input.count,
    )?;
    if matrices > input.matrices.len() / 64 {
        return Err("strided-palette-index");
    }
    validate_floats(input)?;
    calculate_validated(input)
}

/// Validate only the borrowed floating-point views. Palette/index shape is
/// checked separately by `palette_matrices` because capture needs its result
/// to locate the exact matrix span once.
pub(crate) fn validate_floats(input: &Inputs<'_>) -> Result<(), &'static str> {
    let n = input.count as usize;
    for i in 0..n {
        for c in 0..3 {
            if !ordinary(word(input.positions, i * input.position_stride + c * 4)) {
                return Err("strided-fp-input");
            }
        }
    }
    if input
        .matrices
        .chunks_exact(4)
        .any(|v| !ordinary(u32::from_le_bytes(v.try_into().unwrap())))
    {
        return Err("strided-fp-input");
    }
    Ok(())
}

/// Run the borrowed-input kernel after capture has validated all views.
pub(crate) fn calculate_validated(input: &Inputs<'_>) -> Result<Output, &'static str> {
    let mut output = Vec::new();
    calculate_validated_into(input, &mut output)
}

/// Run the validated strided kernel with caller-owned output storage.
pub(crate) fn calculate_validated_into(
    input: &Inputs<'_>,
    output: &mut Vec<f32>,
) -> Result<Output, &'static str> {
    let mut bytes = Vec::new();
    let view = calculate_validated_into_reuse(input, output, &mut bytes)?;
    Ok(Output {
        bytes: view.bytes.to_vec(),
        flags: view.flags,
    })
}

pub(crate) fn calculate_validated_into_reuse<'a>(
    input: &Inputs<'_>,
    output: &mut Vec<f32>,
    bytes: &'a mut Vec<u8>,
) -> Result<OutputView<'a>, &'static str> {
    let n = input.count as usize;
    output.clear();
    output.resize(n * 3, 0.0);
    // Exact spans, bounded strides, all active indices and float bit patterns
    // validated above. Borrowed inputs remain immutable for this synchronous call.
    let flags = unsafe {
        touchHLE_pc_skin_positions_strided(
            output.as_mut_ptr(),
            input.positions.as_ptr(),
            input.position_stride as u32,
            input.weights.as_ptr(),
            input.weight_stride as u32,
            input.indices.as_ptr().cast(),
            input.index_stride as u32,
            input.matrices.as_ptr(),
            input.count,
        )
    };
    if flags & 0x8000_0000 != 0 {
        return Err("fp-exception-native-required");
    }
    Ok(super::pack_bytes(output, bytes, flags))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(
        pos: &'a [u8],
        weights: &'a [u8],
        indices: &'a [u8],
        matrices: &'a [u8],
    ) -> Inputs<'a> {
        Inputs {
            positions: pos,
            position_stride: 12,
            weights,
            weight_stride: 16,
            indices,
            index_stride: 4,
            matrices,
            count: 1,
        }
    }

    #[test]
    fn strided_shapes_indices_specials_and_inactive_padding_are_bounded() {
        let pos = [1.0f32, -2.0, 3.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let mut weights = [1.0f32, 0.0, f32::NAN, f32::NAN]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let mut indices = [0u8, 255, 255, 255];
        let mut matrix = vec![0u8; 64];
        for i in 0..4 {
            matrix[i * 20..i * 20 + 4].copy_from_slice(&1.0f32.to_le_bytes());
        }
        let output = calculate(&input(&pos, &weights, &indices, &matrix)).unwrap();
        assert_eq!(output.bytes, pos);
        assert_eq!(output.flags, 0);
        assert!(calculate(&Inputs {
            positions: &pos[..11],
            ..input(&pos, &weights, &indices, &matrix)
        })
        .is_err());
        assert!(calculate(&Inputs {
            position_stride: usize::MAX,
            ..input(&pos, &weights, &indices, &matrix)
        })
        .is_err());
        assert!(calculate(&Inputs {
            count: 0,
            ..input(&pos, &weights, &indices, &matrix)
        })
        .is_err());
        assert!(calculate(&Inputs {
            count: 32769,
            ..input(&pos, &weights, &indices, &matrix)
        })
        .is_err());
        assert!(calculate(&Inputs {
            weights: &weights[..15],
            ..input(&pos, &weights, &indices, &matrix)
        })
        .is_err());
        assert!(calculate(&Inputs {
            indices: &indices[..3],
            ..input(&pos, &weights, &indices, &matrix)
        })
        .is_err());
        assert!(calculate(&Inputs {
            matrices: &matrix[..63],
            ..input(&pos, &weights, &indices, &matrix)
        })
        .is_err());
        indices[0] = 1;
        assert!(calculate(&input(&pos, &weights, &indices, &matrix)).is_err());
        indices[0] = 255;
        assert!(palette_matrices(&weights, 16, &indices, 4, 1).is_err());
        indices[0] = 0;
        weights[..4].copy_from_slice(&f32::from_bits(1).to_le_bytes());
        assert!(calculate(&input(&pos, &weights, &indices, &matrix)).is_err());
        weights[..4].copy_from_slice(&1.0f32.to_le_bytes());
        matrix[..4].copy_from_slice(&f32::INFINITY.to_le_bytes());
        assert!(calculate(&input(&pos, &weights, &indices, &matrix)).is_err());
    }

    #[test]
    fn unaligned_byte_views_padding_and_multiple_strides_match_packed() {
        let n = 17;
        let pos = (0..n * 3)
            .map(|i| (i as f32 - 21.0) / 13.0)
            .collect::<Vec<_>>();
        let weights = (0..n)
            .flat_map(|_| [0.75f32, 0.25, 0.0, f32::NAN])
            .collect::<Vec<_>>();
        let indices = (0..n).flat_map(|_| [0i8, 1, -1, -1]).collect::<Vec<_>>();
        let matrices = (0..32)
            .map(|i| {
                if i % 5 == 0 {
                    1.0
                } else {
                    (i as f32 - 12.0) / 17.0
                }
            })
            .collect::<Vec<_>>();
        let expected =
            crate::ds_pc_skin_math::calculate(&pos, &weights, &indices, &matrices, n as u32)
                .unwrap();
        fn floats(values: &[f32], components: usize, stride: usize) -> Vec<u8> {
            let mut raw = vec![0xff; 1 + (values.len() / components - 1) * stride + components * 4];
            for (i, row) in values.chunks_exact(components).enumerate() {
                for (c, v) in row.iter().enumerate() {
                    raw[1 + i * stride + c * 4..1 + i * stride + c * 4 + 4]
                        .copy_from_slice(&v.to_le_bytes());
                }
            }
            raw
        }
        for (ps, ws, is) in [(12, 16, 4), (24, 20, 7), (64, 64, 16)] {
            let p = floats(&pos, 3, ps);
            let w = floats(&weights, 4, ws);
            let m = floats(&matrices, 16, 64);
            let mut idx = vec![0xee; 1 + (n - 1) * is + 4];
            for i in 0..n {
                for b in 0..4 {
                    idx[1 + i * is + b] = indices[i * 4 + b] as u8;
                }
            }
            let result = calculate(&Inputs {
                positions: &p[1..],
                position_stride: ps,
                weights: &w[1..],
                weight_stride: ws,
                indices: &idx[1..],
                index_stride: is,
                matrices: &m[1..],
                count: n as u32,
            })
            .unwrap();
            assert_eq!(result.bytes, expected.bytes);
            assert_eq!(result.flags, expected.flags);
            assert_eq!(p[0], 0xff);
            assert_eq!(w[0], 0xff);
            assert_eq!(m[0], 0xff);
            assert_eq!(idx[0], 0xee);
        }
    }
}
