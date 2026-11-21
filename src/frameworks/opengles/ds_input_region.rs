/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

//! Dead Space input geometry. Circles are 24 bytes, boxes 28 bytes.
//! Keep reads shape-specific: offset 0x18 is NOT a circle height.

#[derive(Clone, Copy, Debug)]
pub(super) struct Layout {
    pub circle: u32,
    pub rectangle: u32,
    pub extent: [i32; 2],
}

pub(super) fn visible_bounds(
    layout: Layout,
    mut read: impl FnMut(u32) -> Option<u32>,
) -> Option<(i32, i32, i32, i32)> {
    let vptr = read(0)?;
    if vptr != layout.circle && vptr != layout.rectangle {
        return None;
    }
    let [width, height] = layout.extent;
    let on_screen = |x, y| (0..width).contains(&x) && (0..height).contains(&y);
    let x = read(0xc)? as i32;
    let y = read(0x10)? as i32;
    let size = read(0x14)? as i32;
    let (left, top, w, h, cx, cy) = if vptr == layout.circle {
        if !(1..=200 * width / 480).contains(&size) || !on_screen(x, y) {
            return None;
        }
        (x - size, y - size, size * 2, size * 2, x, y)
    } else {
        let h = read(0x18)? as i32;
        if !(1..=400 * width / 480).contains(&size) || !(1..=400 * height / 320).contains(&h) {
            return None;
        }
        (x, y, size, h, x.checked_add(size / 2)?, y.checked_add(h / 2)?)
    };
    if !on_screen(cx, cy) {
        return None;
    }
    // The iPad layouts project into the native 1024x768 surface. Expose
    // the same 480x320 coordinates to interaction ranking and touch fallback.
    Some((
        left * 480 / width,
        top * 320 / height,
        (w * 480 / width).max(1),
        (h * 320 / height).max(1),
    ))
}

#[cfg(test)]
mod tests {
    use super::{visible_bounds, Layout};

    const CURRENT: Layout = Layout {
        circle: 0x00a6_2ca8,
        rectangle: 0x00a6_3278,
        extent: [480, 320],
    };

    fn decode(words: &[u32]) -> Option<(i32, i32, i32, i32)> {
        visible_bounds(CURRENT, |offset| words.get(offset as usize / 4).copied())
    }

    #[test]
    fn ipad_door_bounds_use_the_native_surface() {
        let ipad = Layout {
            circle: 0x0075_b368,
            rectangle: 0x0075_b918,
            extent: [1024, 768],
        };
        // Active game-owned interaction box at the first story door.
        let door = [ipad.rectangle, 1, 1, 309, 352, 154, 154];
        assert_eq!(decode(&door), None);
        assert_eq!(
            visible_bounds(ipad, |offset| door.get(offset as usize / 4).copied()),
            Some((144, 146, 72, 64)),
        );
        assert_eq!(
            visible_bounds(ipad, |offset| door[..6].get(offset as usize / 4).copied()),
            None,
        );
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
