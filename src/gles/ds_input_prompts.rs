//! CC0 Kenney input sprites, packed once and cached by the owning GL context.
//! Geometry follows verified native widget bounds in 480x320 coordinates.
use super::{gles11_raw as gl, GLES};
use crate::{ds_frontend_controller as ui, image::Image};

const ATLAS: &[u8] = include_bytes!("../../tools/pc-runtime/assets/input-prompts/atlas.png");
const SIZE: (u32, u32) = (512, 384);

#[derive(Clone, Copy)]
enum Icon {
    E,
    Q,
    Escape,
    Enter,
    Alt,
    A,
    X,
    Arrows,
    Dpad,
    Select,
    Delete,
    Start,
    Press,
    Continue,
}
impl Icon {
    fn pixels(self) -> [f32; 4] {
        match self {
            Self::E => [8., 8., 96., 96.],
            Self::Q => [120., 8., 96., 96.],
            Self::Escape => [448., 8., 64., 96.],
            Self::Enter => [232., 8., 96., 96.],
            Self::Alt => [448., 264., 64., 60.],
            Self::A => [344., 8., 96., 96.],
            Self::X => [8., 120., 96., 96.],
            Self::Arrows => [120., 120., 192., 128.],
            Self::Dpad => [320., 120., 96., 96.],
            Self::Select => [420., 142., 88., 60.],
            Self::Delete => [8., 264., 96., 96.],
            Self::Start => [120., 264., 96., 64.],
            Self::Press => [232.,264.,40.,60.],
            // The label starts at x=232. The old x=280 crop dropped the
            // first glyph, which rendered the pause hint as "以继续".
            Self::Continue => [232.,264.,216.,60.],
        }
    }
    fn width(self, height: f32) -> f32 {
        let r = self.pixels();
        height * r[2] / r[3]
    }
}

fn sprite(vertices: &mut Vec<f32>, uv: &mut Vec<f32>, icon: Icon, x: f32, y: f32, h: f32) {
    let w = icon.width(h);
    let r = icon.pixels();
    let (left, right) = (x / 240. - 1., (x + w) / 240. - 1.);
    let (top, bottom) = (1. - y / 160., 1. - (y + h) / 160.);
    vertices.extend_from_slice(&[
        left, top, right, top, right, bottom, left, top, right, bottom, left, bottom,
    ]);
    let (u0, u1) = (
        (r[0] + 0.5) / SIZE.0 as f32,
        (r[0] + r[2] - 0.5) / SIZE.0 as f32,
    );
    let (v0, v1) = (
        (r[1] + 0.5) / SIZE.1 as f32,
        (r[1] + r[3] - 0.5) / SIZE.1 as f32,
    );
    uv.extend_from_slice(&[u0, v0, u1, v0, u1, v1, u0, v0, u1, v1, u0, v1]);
}

fn geometry() -> (Vec<f32>, Vec<f32>) {
    let (controls, navigation) = ui::prompt_controls();
    let (mut vertices, mut uv) = (Vec::new(), Vec::new());
    let keyboard=ui::keyboard_hints();
    if ui::pause_menu() {
        let icons: &[Icon] = if keyboard {
            &[Icon::Escape, Icon::Continue, Icon::Alt]
        } else {
            &[Icon::Start, Icon::Continue, Icon::Alt]
        };
        let height = 12.;
        let width: f32 = icons.iter().map(|icon| icon.width(height) + 3.).sum::<f32>() - 3.;
        let mut x = 240. - width / 2.;
        for icon in icons {
            // Leave a safe-area margin below the prompt row so the full
            // Continue/Alt labels remain visible on the native pause HUD.
            sprite(&mut vertices, &mut uv, *icon, x, 294., height);
            x += icon.width(height) + 3.;
        }
        return (vertices, uv);
    }
    let confirm:&[Icon]=if keyboard {&[Icon::E,Icon::Enter]}else{&[Icon::A]};
    if ui::loading_ready() {
        let mut icons=vec![Icon::Press];icons.extend_from_slice(confirm);icons.push(Icon::Continue);
        let width:f32=icons.iter().map(|icon|icon.width(14.)+4.).sum::<f32>()-4.;
        let mut x=240.-width/2.;
        for icon in icons {sprite(&mut vertices,&mut uv,icon,x,296.,14.);x+=icon.width(14.)+4.;}
        return (vertices,uv);
    }
    for (rect,cancel) in controls {
        let icons:&[Icon]=if cancel {if keyboard {&[Icon::Q]}else{&[Icon::X]}}else{confirm};
        let width=icons.iter().map(|i|i.width(14.)+4.).sum::<f32>()-4.;
        let at=ui::key_row_anchor_width(rect,cancel,width);let mut x=at.x;
        for &icon in icons {sprite(&mut vertices,&mut uv,icon,x,at.y,14.);x+=icon.width(14.)+4.;}
    }
    if let Some(rect)=ui::delete_rect() {
        let icon=if keyboard {Icon::Delete}else{Icon::Start};
        let at=ui::key_row_anchor_width(rect,false,icon.width(14.));
        sprite(&mut vertices,&mut uv,icon,at.x,at.y,14.);
    }
    if let Some(rect) = navigation {
        let h = 12.;
        let direction=if keyboard {Icon::Arrows}else{Icon::Dpad};
        let width = direction.width(h) + Icon::Select.width(h) + 5.;
        let (x, y) = if ui::home_page() {(420.,216.)}
        else if ui::modal_active() { (170.,292.) }
        else if rect[1]>=250. && rect[2]<=160. { (210.,249.) }
        else if rect[3] > 65. {
            (
                rect[0] + rect[2] / 2. - width / 2.,
                if rect[1] + rect[3] + h + 6. <= 317. {
                    rect[1] + rect[3] + 6.
                } else {
                    rect[1] - h - 6.
                },
            )
        } else if rect[2] >= 180. {
            (
                rect[0] + rect[2] - width - 12.,
                rect[1] + rect[3] / 2. - h / 2.,
            )
        } else if rect[0] + rect[2] + width + 6. <= 477. {
            (rect[0] + rect[2] + 6., rect[1] + rect[3] / 2. - h / 2.)
        } else {
            (rect[0] - width - 6., rect[1] + rect[3] / 2. - h / 2.)
        };
        let mut x = x.clamp(3., 477. - width);
        let y = y.clamp(3., 305.);
        for icon in [direction, Icon::Select] {
            sprite(&mut vertices, &mut uv, icon, x, y, h);
            x += icon.width(h) + 5.;
        }
    }
    (vertices, uv)
}

fn image() -> Result<Image, String> {
    let external = std::env::var_os("TOUCHHLE_DS_INPUT_ATLAS")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.join("assets/input-prompts/atlas.png")))
        });
    if let Some(path) = external.filter(|p| p.metadata().is_ok_and(|m| m.len() <= 4 * 1024 * 1024))
    {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(image) = Image::from_bytes(&bytes) {
                if image.dimensions() == SIZE {
                    return Ok(image);
                }
            }
        }
    }
    Image::from_bytes(ATLAS)
}

/// Existing present callers restore guest GL state. Atlas names are never
/// shared across unshared contexts and disappear when their context is freed.
pub(super) unsafe fn draw(gles: &mut dyn GLES) {
    let (vertices, uv) = geometry();
    if vertices.is_empty() {
        return;
    }
    let Some(cached) = gles.input_prompt_atlas().map(|slot| *slot) else {
        return;
    };
    if cached == Some(0) {
        return;
    }
    let mut binding = 0;
    let mut unpack = 0;
    gles.GetIntegerv(gl::TEXTURE_BINDING_2D, &mut binding);
    gles.GetIntegerv(gl::UNPACK_ALIGNMENT, &mut unpack);
    gles.PixelStorei(gl::UNPACK_ALIGNMENT, 1);
    let texture = if let Some(texture) = cached {
        texture
    } else {
        let image = match image() {
            Ok(image) => image,
            Err(error) => {
                echo!("[DSinput-atlas] decode failed: {error}");
                *gles.input_prompt_atlas().unwrap() = Some(0);
                gles.PixelStorei(gl::UNPACK_ALIGNMENT, unpack);
                return;
            }
        };
        let mut texture = 0;
        gles.GenTextures(1, &mut texture);
        gles.BindTexture(gl::TEXTURE_2D, texture);
        for p in [gl::TEXTURE_MIN_FILTER, gl::TEXTURE_MAG_FILTER] {
            gles.TexParameteri(gl::TEXTURE_2D, p, gl::LINEAR as _);
        }
        for p in [gl::TEXTURE_WRAP_S, gl::TEXTURE_WRAP_T] {
            gles.TexParameteri(gl::TEXTURE_2D, p, gl::CLAMP_TO_EDGE as _);
        }
        gles.TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RGBA as _,
            SIZE.0 as _,
            SIZE.1 as _,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            image.pixels().as_ptr().cast(),
        );
        let error = gles.GetError();
        if error != gl::NO_ERROR {
            gles.DeleteTextures(1, &texture);
            *gles.input_prompt_atlas().unwrap() = Some(0);
            gles.BindTexture(gl::TEXTURE_2D, binding as _);
            gles.PixelStorei(gl::UNPACK_ALIGNMENT, unpack);
            echo!("[DSinput-atlas] upload failed error={error:#x}");
            return;
        }
        *gles.input_prompt_atlas().unwrap() = Some(texture);
        echo!("[DSinput-atlas] cached 512x256 texture; native control anchors");
        texture
    };
    gles.BindTexture(gl::TEXTURE_2D, texture);
    gles.DisableClientState(gl::COLOR_ARRAY);
    gles.EnableClientState(gl::VERTEX_ARRAY);
    gles.EnableClientState(gl::TEXTURE_COORD_ARRAY);
    gles.Enable(gl::TEXTURE_2D);
    gles.Enable(gl::BLEND);
    gles.BlendFunc(gl::ONE, gl::ONE_MINUS_SRC_ALPHA);
    gles.MatrixMode(gl::TEXTURE);
    gles.LoadIdentity();
    gles.MatrixMode(gl::MODELVIEW);
    let mode = [gl::MODULATE as i32];
    gles.TexEnviv(gl::TEXTURE_ENV, gl::TEXTURE_ENV_MODE, mode.as_ptr());
    gles.Color4f(1., 1., 1., 1.);
    gles.VertexPointer(2, gl::FLOAT, 0, vertices.as_ptr().cast());
    gles.TexCoordPointer(2, gl::FLOAT, 0, uv.as_ptr().cast());
    gles.DrawArrays(gl::TRIANGLES, 0, (vertices.len() / 2) as _);
    gles.BindTexture(gl::TEXTURE_2D, binding as _);
    gles.PixelStorei(gl::UNPACK_ALIGNMENT, unpack);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_atlas_and_sprite_coordinates_agree() {
        let image = Image::from_bytes(ATLAS).unwrap();
        assert_eq!(image.dimensions(), SIZE);
        for icon in [
            Icon::E,
            Icon::Q,
            Icon::Escape,
            Icon::Enter,
            Icon::Alt,
            Icon::A,
            Icon::X,
            Icon::Arrows,
            Icon::Dpad,
            Icon::Select,
            Icon::Delete,
            Icon::Start,
        ] {
            let r = icon.pixels();
            assert!(r[0] + r[2] <= SIZE.0 as f32 && r[1] + r[3] <= SIZE.1 as f32);
            let (mut vertices, mut uv) = (Vec::new(), Vec::new());
            sprite(&mut vertices, &mut uv, icon, 10., 20., 14.);
            assert_eq!(vertices.len(), uv.len());
            assert_eq!(vertices.len(), 12);
            assert!(uv.iter().all(|n| *n >= 0. && *n <= 1.));
        }
    }
}
