//! Later-engine window/cursor/fullscreen packets, native8efb0/8f740.
use std::sync::Arc;

use anyhow::{Result, ensure};
use glam::{Mat4, Vec2, vec2, vec3};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

use super::message_clock::{MessageClock, OutgoingWindow};
use crate::{
    asset::{
        font::gerokasu2::GpuGerokasu2Font, system::AssetServer, texture_archive::TextureArchive,
    },
    layer::render_params::multiply_transforms,
};

#[derive(TextureArchive)]
pub struct MessageTextures {
    #[txa(name = "msgwnd")]
    pub window: GpuTexture,
    #[txa(name = "keywait")]
    pub key_wait: GpuTexture,
    #[txa(name = "select")]
    pub selection: GpuTexture,
}

/// Gerokasu ships one FNT4 message font under the Umineko name.
pub fn medium_font_path(game: crate::cli::Game) -> &'static str {
    if game == crate::cli::Game::Gerokasu {
        "/newrodin.fnt"
    } else {
        "/newrodin-medium.fnt"
    }
}
pub struct MessageResources {
    pub medium: Arc<GpuGerokasu2Font>,
    pub textures: Arc<MessageTextures>,
}
impl MessageResources {
    pub async fn load(assets: &AssetServer) -> Result<Self> {
        let font = medium_font_path(assets.context().game);
        let (medium, textures) = futures::try_join!(
            assets.load(font),
            assets.load("/msgtex.txa")
        )?;
        Ok(Self { medium, textures })
    }
    /// Native5c894..5c8bc clones the medium slot into the bold slot. Both
    /// layout roles retain one font/cache owner, unlike Umineko's bold asset.
    pub fn fonts(&self) -> [&Arc<GpuGerokasu2Font>; 2] {
        [&self.medium, &self.medium]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MessageUiSettings {
    pub window_opacity: u8,
}
impl Default for MessageUiSettings {
    fn default() -> Self {
        Self {
            window_opacity: 100,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiTexture {
    Window,
    KeyWait,
}
pub struct UiDraw {
    pub texture: Option<UiTexture>,
    pub vertices: [PosColTexVertex; 4],
    pub transform: Mat4,
    pub stencil: u8,
}
impl UiDraw {
    pub fn render(&self, pass: &mut RenderPass, textures: &MessageTextures) {
        let builder = RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .depth_stencil_shorthand(self.stencil, true, false);
        if let Some(texture) = self.texture {
            let texture = match texture {
                UiTexture::Window => &textures.window,
                UiTexture::KeyWait => &textures.key_wait,
            };
            pass.run(builder.build(
                RenderProgramWithArguments::Sprite {
                    vertices: VertexSource::VertexData {
                        vertices: &self.vertices,
                    },
                    sprite: texture.as_source(),
                    transform: self.transform,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        } else {
            let vertices = self.vertices.map(|v| v.pos_col());
            pass.run(builder.build(
                RenderProgramWithArguments::Fill {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    transform: self.transform,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
}
pub struct MessageUiPlan {
    pub draws: Vec<UiDraw>,
    pub text_transform: Option<Mat4>,
    pub text_stencil: u8,
}

// Native7e8a8 with(0,1,1,0,-1,1). Later engine projection already maps
// depth into0..1; do not apply another clip-depth conversion.
fn normalized_projection() -> Mat4 {
    Mat4::from_cols_array(&[
        2.0, 0.0, 0.0, 0.0, 0.0, -2.0, 0.0, 0.0, 0.0, 0.0, -0.5, 0.0, -1.0, 1.0, 0.5, 1.0,
    ])
}
fn translated(base: Mat4, position: Vec2) -> Mat4 {
    multiply_transforms(base, Mat4::from_translation(position.extend(0.0)))
}
fn quad(
    texture: Option<UiTexture>,
    rect: [f32; 4],
    uv: [f32; 4],
    color: UnormColor,
    transform: Mat4,
    stencil: u8,
) -> UiDraw {
    let [left, top, right, bottom] = rect;
    let [u0, v0, u1, v1] = uv;
    UiDraw {
        texture,
        transform,
        stencil,
        vertices: [
            (left, top, u0, v0),
            (right, top, u1, v0),
            (left, bottom, u0, v1),
            (right, bottom, u1, v1),
        ]
        .map(|(x, y, u, v)| PosColTexVertex {
            position: vec3(x, y, 0.0),
            color,
            texture_position: vec2(u, v),
        }),
    }
}
fn background(alpha: f32, stencil: u8) -> UiDraw {
    quad(
        None,
        [0.0, 0.0, 1.0, 1.0],
        [0.0, 0.0, 1.0, 1.0],
        FloatColor4::from_rgba(0.0, 0.0, 0.0, alpha).into_unorm(),
        normalized_projection(),
        stencil,
    )
}
fn window(
    draws: &mut Vec<UiDraw>,
    window: &OutgoingWindow,
    modal: f32,
    settings: MessageUiSettings,
    base: Mat4,
    stencil: u8,
) {
    if window.slide == 0.0 {
        return;
    }
    let progress = window.slide * modal;
    match window.mode {
        0 | 3 => {
            let y = (1080.0 - window.height) + (-20.0);
            let y = (1.0 - progress).mul_add(96.0, y);
            let alpha = progress * (f32::from(settings.window_opacity) * 0.01);
            draws.push(quad(
                Some(UiTexture::Window),
                [-64.0, -20.0, 1984.0, 940.0],
                [0.0, 0.0, 1.0, 1.0],
                FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).into_unorm(),
                translated(base, vec2(0.0, y)),
                stencil,
            ));
        }
        1 => draws.push(background(progress * 0.7, stencil)),
        _ => {}
    }
}

pub fn plan(
    clock: &MessageClock,
    has_ready_text: bool,
    settings: MessageUiSettings,
    parent: Mat4,
    local: Mat4,
    stencil: u8,
    pass: PassKind,
) -> Result<MessageUiPlan> {
    let mut result = MessageUiPlan {
        draws: Vec::new(),
        text_transform: None,
        text_stencil: stencil.wrapping_add(2),
    };
    if pass == PassKind::Opaque {
        return Ok(result);
    }
    let base = multiply_transforms(parent, local);
    if clock.background > 0.0 {
        result.draws.push(background(
            (clock.background * clock.modal_slide) * 0.7,
            stencil,
        ));
    }
    for descriptor in clock.outgoing.iter().rev() {
        window(
            &mut result.draws,
            descriptor,
            clock.modal_slide,
            settings,
            base,
            stencil,
        );
    }
    window(
        &mut result.draws,
        &OutgoingWindow {
            mode: clock.params.mode,
            slide: clock.slide,
            height: clock.height,
        },
        clock.modal_slide,
        settings,
        base,
        stencil,
    );
    if clock.slide * clock.modal_slide < 1.0 || !has_ready_text {
        return Ok(result);
    }
    ensure!(
        (0..=4).contains(&clock.params.mode),
        "Gerokasu2 message renderer has unassigned raw window mode{}",
        clock.params.mode
    );
    let offset = if matches!(clock.params.mode, 0 | 2 | 3) {
        (1.0 - clock.slide).mul_add(64.0, (1080.0 - clock.height) + (-32.0))
    } else {
        ((1080.0 - clock.text_size.y) * f32::from_bits(0x3eb33333)).max(32.0)
    };
    if clock.wait_kind >= 0 {
        let phase = (clock.ticks_since_wait / 60.0) * f32::from_bits(0x40490fd8);
        let alpha = phase.sin().mul_add(0.4, 0.6);
        let (rect, uv, position, alpha) = if clock.wait_kind == 0 {
            (
                [0.0, 0.0, 30.0, 30.0],
                [
                    0.0,
                    f32::from_bits(0x3e2aaaab),
                    0.25,
                    f32::from_bits(0x3f555556),
                ],
                vec2(clock.cursor.x + 360.0, (offset + clock.cursor.y) + (-30.0)),
                alpha,
            )
        } else {
            let position = if matches!(clock.params.mode, 1 | 2 | 4) {
                vec2(1800.0, 980.0)
            } else {
                vec2(1562.0, (offset + clock.height) + (-60.0))
            };
            (
                [0.0, 0.0, 53.0, 60.0],
                [0.4, 0.0, f32::from_bits(0x3f3b3334), 1.0],
                position,
                if alpha > 0.5 { 1.0 } else { 0.0 },
            )
        };
        result.draws.push(quad(
            Some(UiTexture::KeyWait),
            rect,
            uv,
            FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).into_unorm(),
            translated(base, position),
            stencil,
        ));
    }
    if clock.params.mode != 2 {
        result.text_transform = Some(translated(base, vec2(360.0, offset)));
    }
    Ok(result)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use tests::gpu_tests::frame as reference_frame;
