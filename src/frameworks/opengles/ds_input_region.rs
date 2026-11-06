/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! Dead Space 1.3.34 input geometry. Circles are 24 bytes, boxes 28 bytes.
//! Keep reads shape-specific: offset 0x18 is NOT a circle height.

pub(crate) fn visible_bounds(mut read: impl FnMut(u32) -> u32) -> Option<(i32, i32, i32, i32)> {
    let vptr = read(0);
    if !matches!(vptr, 0x00a6_2ca8 | 0x00a6_3278) {
        return None;
    }
    let x = read(0xc) as i32;
    let y = read(0x10) as i32;
    let size = read(0x14) as i32;
    let (left, top, width, height, cx, cy) = match vptr {
        0x00a6_2ca8 => {
            if !(1..=200).contains(&size) || !(0..480).contains(&x) || !(0..320).contains(&y) {
                return None;
            }
            (x - size, y - size, size * 2, size * 2, x, y)
        }
        0x00a6_3278 => {
            let height = read(0x18) as i32;
            if !(1..=400).contains(&size) || !(1..=400).contains(&height) {
                return None;
            }
            (
                x,
                y,
                size,
                height,
                x.checked_add(size / 2)?,
                y.checked_add(height / 2)?,
            )
        }
        _ => unreachable!(),
    };
    if !(0..480).contains(&cx) || !(0..320).contains(&cy) {
        return None;
    }
    Some((left, top, width, height))
}

#[cfg(test)]
mod tests {
    use super::visible_bounds;

    fn decode(words: &[u32]) -> Option<(i32, i32, i32, i32)> {
        visible_bounds(|offset| words[offset as usize / 4])
    }

    #[test]
    fn captured_kinesis_circle_does_not_read_past_allocation() {
        // F5 frame 6506: InputRegionCircle at 0x36afbf30, center 293,204,
        // radius 30. The former box reader consumed allocator data as height.
        assert_eq!(
            decode(&[0xa62ca8, 2, 1, 293, 204, 30]),
            Some((263, 174, 60, 60))
        );
    }

    #[test]
    fn captured_door_box_keeps_rectangle_coordinates() {
        assert_eq!(
            decode(&[0xa63278, 1, 1, 295, 105, 48, 48]),
            Some((295, 105, 48, 48))
        );
    }

    #[test]
    fn hidden_and_unknown_regions_are_rejected() {
        assert_eq!(
            decode(&[0xa62ca8, 2, 1, (-100i32) as u32, (-100i32) as u32, 30]),
            None
        );
        assert_eq!(
            decode(&[0xa63278, 1, 1, (-120i32) as u32, (-120i32) as u32, 40, 40]),
            None
        );
        assert_eq!(decode(&[0xdeadbeef]), None);
    }

    #[test]
    fn corrupt_sizes_and_overflow_are_rejected() {
        assert_eq!(decode(&[0xa62ca8, 2, 1, 100, 100, u32::MAX]), None);
        assert_eq!(
            decode(&[0xa63278, 1, 1, i32::MAX as u32, 100, 48, 48]),
            None
        );
        assert_eq!(decode(&[0xa63278, 1, 1, 100, 100, 48, 0]), None);
    }
}
