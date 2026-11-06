pub fn scale_quads(positions: &mut [[f32; 3]], indices: &[usize], scale: f32) -> bool {
    if indices.is_empty() || indices.len() % 6 != 0 || !scale.is_finite() || scale <= 0.0 {
        return false;
    }
    let mut visited = vec![false; positions.len()];
    let mut quads = Vec::with_capacity(indices.len() / 6);
    for triangles in indices.chunks_exact(6) {
        let mut corners = triangles.to_vec();
        corners.sort_unstable();
        corners.dedup();
        if corners.len() != 4
            || triangles.chunks_exact(3).any(|triangle| {
                triangle[0] == triangle[1]
                    || triangle[0] == triangle[2]
                    || triangle[1] == triangle[2]
            })
            || corners.iter().any(|&corner| {
                corner >= positions.len()
                    || visited[corner]
                    || positions[corner].iter().any(|value| !value.is_finite())
            })
        {
            return false;
        }
        for &corner in &corners {
            visited[corner] = true;
        }
        quads.push(corners);
    }
    for corners in quads {
        let mut center = [0.0; 3];
        for &corner in &corners {
            for axis in 0..3 {
                center[axis] += positions[corner][axis] * 0.25;
            }
        }
        for corner in corners {
            for axis in 0..3 {
                positions[corner][axis] =
                    center[axis] + (positions[corner][axis] - center[axis]) * scale;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::scale_quads;

    #[test]
    fn halves_each_particle_without_moving_its_center_or_unused_vertices() {
        let mut positions = [
            [99.0, 99.0, 99.0],
            [8.0, 18.0, 30.0],
            [12.0, 18.0, 30.0],
            [8.0, 22.0, 30.0],
            [12.0, 22.0, 30.0],
            [-12.0, -22.0, 40.0],
            [-8.0, -22.0, 40.0],
            [-12.0, -18.0, 40.0],
            [-8.0, -18.0, 40.0],
        ];
        assert!(scale_quads(
            &mut positions,
            &[1, 2, 3, 3, 2, 4, 5, 6, 7, 7, 6, 8],
            0.5
        ));
        assert_eq!(positions[0], [99.0, 99.0, 99.0]);
        assert_eq!(positions[1], [9.0, 19.0, 30.0]);
        assert_eq!(positions[4], [11.0, 21.0, 30.0]);
        assert_eq!(positions[5], [-11.0, -21.0, 40.0]);
        assert_eq!(positions[8], [-9.0, -19.0, 40.0]);
    }

    #[test]
    fn rejects_shared_or_invalid_topology_without_partial_changes() {
        let original = [[1.0, 2.0, 3.0]; 8];
        for indices in [
            vec![0, 1, 2, 2, 1, 3, 3, 4, 5, 5, 4, 6],
            vec![0, 1, 2, 2, 1, 3, 4, 5, 6, 6, 5, 8],
            vec![0, 0, 1, 1, 2, 3],
        ] {
            let mut positions = original;
            assert!(!scale_quads(&mut positions, &indices, 0.5));
            assert_eq!(positions, original);
        }
    }
}
