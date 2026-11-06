//! A CPU readback remains reusable storage after direct GPU presentation, but
//! ceases to be a current image. Replaying it during UIKit transitions can
//! expose a menu from before the gameplay session.

#[derive(Clone, Default)]
pub(super) struct PresentedPixels {
    image: Option<(Vec<u8>, u32, u32)>,
    current: bool,
}

impl PresentedPixels {
    pub(super) fn present(&mut self, pixels: Vec<u8>, width: u32, height: u32) {
        self.image = Some((pixels, width, height));
        self.current = true;
    }

    pub(super) fn invalidate(&mut self) -> bool {
        let changed = self.is_current();
        self.current = false;
        changed
    }

    pub(super) fn is_current(&self) -> bool {
        self.current && self.image.is_some()
    }

    pub(super) fn current_mut(&mut self) -> Option<(&mut Vec<u8>, u32, u32)> {
        if !self.current { return None; }
        self.image.as_mut().map(|(pixels, width, height)| (pixels, *width, *height))
    }

    pub(super) fn dimensions(&self) -> Option<(u32, u32)> {
        self.image.as_ref().map(|(_, width, height)| (*width, *height))
    }

    pub(super) fn take(&mut self) -> Option<(Vec<u8>, u32, u32)> {
        self.current = false;
        self.image.take()
    }
}

#[cfg(test)]
mod tests {
    use super::PresentedPixels;

    #[test]
    fn direct_present_retires_menu_image_but_retains_readback_storage_and_dimensions() {
        let mut cache = PresentedPixels::default();
        let pixels = vec![0x42; 320 * 480 * 4];
        let allocation = pixels.as_ptr();
        cache.present(pixels, 320, 480);
        assert!(cache.is_current());
        assert!(cache.invalidate());
        assert!(!cache.is_current());
        assert!(cache.current_mut().is_none());
        assert_eq!(cache.dimensions(), Some((320, 480)));
        assert!(!cache.invalidate());
        let (mut recycled, width, height) = cache.take().unwrap();
        assert_eq!(recycled.as_ptr(), allocation);
        recycled.fill(0x91);
        cache.present(recycled, width, height);
        assert!(cache.is_current());
        let (fresh, width, height) = cache.current_mut().unwrap();
        assert_eq!((width, height), (320, 480));
        assert!(fresh.iter().all(|byte| *byte == 0x91));
    }
}
