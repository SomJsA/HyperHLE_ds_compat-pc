//! Android-only GLES -> AHardwareBuffer bridge used by the FSR3 path.

use std::ffi::c_void;

extern "C" {
    fn ds_ahb_bridge_init(width: i32, height: i32, output_width: i32, output_height: i32) -> u8;
    fn ds_ahb_bridge_copy_from_texture(texture: u32, width: i32, height: i32) -> u8;
    fn ds_ahb_bridge_verify_pixel(out_rgba: *mut u8) -> u8;
    fn ds_ahb_bridge_handle() -> *mut c_void;
    fn ds_ahb_bridge_output_texture() -> u32;
    fn ds_ahb_bridge_output_size(width: *mut i32, height: *mut i32);
    fn ds_ahb_bridge_destroy();
    fn ds_depth_image_create_from_renderbuffer(
        depth_rb: u32,
        width: i32,
        height: i32,
    ) -> *mut c_void;
    fn ds_depth_image_read(image: *mut c_void, width: i32, height: i32, depth: *mut f32) -> u8;
    fn ds_depth_ahb_probe(width: i32, height: i32);
    fn ds_depth_image_copy_to_ahb(image: *mut c_void, width: i32, height: i32) -> u8;
    fn ds_depth_ahb_handle() -> *mut c_void;
    fn ds_depth_image_cache_reset();
}

pub unsafe fn init(width: u32, height: u32, output_width: u32, output_height: u32) -> bool {
    ds_ahb_bridge_init(
        width as i32,
        height as i32,
        output_width as i32,
        output_height as i32,
    ) != 0
}

pub unsafe fn copy_from_texture(texture: u32, width: u32, height: u32) -> bool {
    ds_ahb_bridge_copy_from_texture(texture, width as i32, height as i32) != 0
}

pub unsafe fn verify_pixel() -> Option<[u8; 4]> {
    let mut rgba = [0u8; 4];
    (ds_ahb_bridge_verify_pixel(rgba.as_mut_ptr()) != 0).then_some(rgba)
}

pub unsafe fn handle() -> *mut c_void {
    ds_ahb_bridge_handle()
}

pub unsafe fn output_texture() -> u32 {
    ds_ahb_bridge_output_texture()
}

pub unsafe fn output_size() -> (u32, u32) {
    let mut width = 0;
    let mut height = 0;
    ds_ahb_bridge_output_size(&mut width, &mut height);
    (width.max(0) as u32, height.max(0) as u32)
}

pub unsafe fn depth_image_create(depth_rb: u32, width: u32, height: u32) -> *mut c_void {
    ds_depth_image_create_from_renderbuffer(depth_rb, width as i32, height as i32)
}

pub unsafe fn depth_image_read(
    image: *mut c_void,
    width: u32,
    height: u32,
    depth: &mut [f32],
) -> bool {
    if width == 0 || height == 0 || depth.len() != (width as usize).saturating_mul(height as usize)
    {
        return false;
    }
    ds_depth_image_read(image, width as i32, height as i32, depth.as_mut_ptr()) != 0
}

pub unsafe fn depth_ahb_probe(width: u32, height: u32) {
    ds_depth_ahb_probe(width as i32, height as i32);
}

pub unsafe fn depth_image_copy_to_ahb(image: *mut c_void, width: u32, height: u32) -> bool {
    ds_depth_image_copy_to_ahb(image, width as i32, height as i32) != 0
}

pub unsafe fn depth_ahb_handle() -> *mut c_void {
    ds_depth_ahb_handle()
}

pub unsafe fn depth_image_cache_reset() {
    ds_depth_image_cache_reset();
}

pub unsafe fn vulkan_probe(handle: *mut c_void) -> bool {
    ds_ahb_vulkan_probe(handle) != 0
}

pub unsafe fn fsr3_upscale() -> Option<[u8; 4]> {
    fsr3_upscale_with_depth(None, std::ptr::null_mut())
}

pub unsafe fn fsr3_wait_pending() -> Option<(u32, u32, u32)> {
    let texture = ds_ahb_bridge_fsr3_wait_pending();
    if texture == 0 {
        return None;
    }
    let (width, height) = output_size();
    (width != 0 && height != 0).then_some((texture, width, height))
}

pub unsafe fn fsr3_upscale_with_depth(
    depth: Option<&[f32]>,
    depth_ahb: *mut c_void,
) -> Option<[u8; 4]> {
    let mut rgba = [0u8; 4];
    let depth_ptr = depth.map_or(std::ptr::null(), |values| values.as_ptr());
    (ds_ahb_bridge_fsr3_upscale(depth_ptr, depth_ahb, rgba.as_mut_ptr()) != 0).then_some(rgba)
}

pub unsafe fn fsr3_copy_fg() -> bool {
    ds_ahb_bridge_fsr3_copy_fg() != 0
}

extern "C" {
    fn ds_ahb_vulkan_probe(handle: *mut c_void) -> u8;
    fn ds_ahb_bridge_fsr3_upscale(
        depth_data: *const f32,
        depth_ahb: *mut c_void,
        out_rgba: *mut u8,
    ) -> u8;
    fn ds_ahb_bridge_fsr3_copy_fg() -> u8;
    fn ds_ahb_bridge_fsr3_wait_pending() -> u32;
}

#[allow(dead_code)]
pub unsafe fn destroy() {
    ds_ahb_bridge_destroy();
}
