//! Updated message windows and wait cursors (84f6c/85660). Authored geometry
//! remains 1920x1080; the renderer can target handheld or docked dimensions.
use std::sync::Arc;

use anyhow::Result;
use glam::{Mat4, Vec2, vec2, vec3};
use shin_core::{layout::HigurashiMessageStyle, primitives::color::FloatColor4};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

use super::{
    message_glyphs::message_offset,
    message_playback::{HigurashiMessagePlayback, OutgoingWindow, WaitKind},
};
use crate::{
    asset::{system::AssetServer, texture_archive::TextureArchive},
    layer::render_params::higurashi::HigurashiTransform,
};

#[derive(TextureArchive)]
pub struct HigurashiMessageTextures {
    #[txa(name = "msgwnd")]
    pub window: GpuTexture,
    #[txa(name = "cursor0")]
    pub cursor_regular: GpuTexture,
    #[txa(name = "cursor1")]
    pub cursor_last: GpuTexture,
    #[txa(name = "keywait")]
    pub key_wait: GpuTexture,
    #[txa(name = "select")]
    pub selection: GpuTexture,
    #[txa(name = "fselect")]
    pub focused_selection: GpuTexture,
    #[txa(name = "navi_icon")]
    pub navigation_icons: GpuTexture,
    #[txa(name = "navi_arrow")]
    pub navigation_arrows: GpuTexture,
}
impl HigurashiMessageTextures {
    pub async fn load(assets: &AssetServer) -> Result<Arc<Self>> {
        assets.load("/msgtex.txa").await
    }
}

/// Native settings bytes f5 and101. The ordinary preferences owner will supply
/// these; defaults below are host initialization, not a saved-settings claim.
#[derive(Clone, Copy)]
pub struct MessageUiSettings {
    pub window_opacity: u8,
    pub transparent_window_setting: u8,
}
impl Default for MessageUiSettings {
    fn default() -> Self {
        Self {
            window_opacity: 100,
            transparent_window_setting: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiTexture {
    Window,
    CursorRegular,
    CursorLast,
    KeyWait,
}
pub struct MessageUiDraw {
    pub texture: Option<UiTexture>,
    pub vertices: Vec<PosColTexVertex>,
    pub indices: Vec<u16>,
    pub native_transform: Mat4,
    /// Windows replace the caller's stencil reference. Cursors draw after the
    /// native reset and use the renderer's default depth/stencil state.
    pub stencil: Option<u8>,
}
impl MessageUiDraw {
    pub fn render(&self, pass: &mut RenderPass, textures: &HigurashiMessageTextures) {
        let mut builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        if let Some(stencil) = self.stencil {
            builder = builder.depth_stencil_shorthand(stencil, true, false);
        }
        let transform = HigurashiTransform::convert_clip_depth(self.native_transform);
        match self.texture {
            Some(texture) => {
                let texture = match texture {
                    UiTexture::Window => &textures.window,
                    UiTexture::CursorRegular => &textures.cursor_regular,
                    UiTexture::CursorLast => &textures.cursor_last,
                    UiTexture::KeyWait => &textures.key_wait,
                }
                .as_source();
                let vertices = if self.indices.is_empty() {
                    VertexSource::VertexData {
                        vertices: &self.vertices,
                    }
                } else {
                    VertexSource::VertexAndIndexData {
                        vertices: &self.vertices,
                        indices: &self.indices,
                    }
                };
                pass.run(builder.build(
                    RenderProgramWithArguments::Sprite {
                        vertices,
                        sprite: texture,
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
            None => {
                let vertices: Vec<_> = self.vertices.iter().map(|v| v.pos_col()).collect();
                pass.run(builder.build(
                    RenderProgramWithArguments::Fill {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
        }
    }
}

fn quad(size: f32, u0: f32, u1: f32, alpha: f32, white: bool) -> Vec<PosColTexVertex> {
    let rgb = if white { 1.0 } else { 0.0 };
    let color = FloatColor4::from_rgba(rgb, rgb, rgb, alpha).into_unorm();
    [
        (0.0, 0.0, u0, 0.0),
        (size, 0.0, u1, 0.0),
        (0.0, size, u0, 1.0),
        (size, size, u1, 1.0),
    ]
    .into_iter()
    .map(|(x, y, u, v)| PosColTexVertex {
        position: vec3(x, y, 0.0),
        color,
        texture_position: vec2(u, v),
    })
    .collect()
}

fn window(
    out: &mut Vec<MessageUiDraw>,
    descriptor: OutgoingWindow,
    modal: f32,
    name_width: f32,
    settings: MessageUiSettings,
    parent: &HigurashiTransform,
    stencil: u8,
) {
    if descriptor.slide == 0.0 {
        return;
    }
    let slide = descriptor.slide * modal;
    if descriptor.style == 1 || descriptor.style == 3 {
        let opacity = if descriptor.style == 1 {
            0.7
        } else {
            ((100 - i32::from(settings.transparent_window_setting)) as f32 * 0.01) * 0.7
        };
        if opacity <= 0.0 {
            return;
        }
        // Native74d34 normalized projection, independent of parent properties.
        let matrix = Mat4::from_cols_array(&[
            2.0, 0.0, 0.0, 0.0, 0.0, -2.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, -1.0, 1.0, -0.0, 1.0,
        ]);
        out.push(MessageUiDraw {
            texture: None,
            vertices: quad(1.0, 0.0, 1.0, slide * opacity, false),
            indices: Vec::new(),
            native_transform: matrix,
            stencil: Some(stencil),
        });
        return;
    }
    if descriptor.style != 0 {
        return;
    }
    let offset = (1.0 - slide) * 96.0 + ((1080.0 - descriptor.height) - 32.0);
    let matrix = parent.compute_native_message_transform(vec2(0.0, offset));
    let color = FloatColor4::from_rgba(
        1.0,
        1.0,
        1.0,
        slide * (f32::from(settings.window_opacity) * 0.01),
    )
    .into_unorm();
    let make = |x: f32, y: f32, u: f32, v: f32| PosColTexVertex {
        position: vec3(x, y, 1.0),
        color,
        // Native85b10 embeds this UV one ULP above f32 division160/1648.
        texture_position: vec2(
            if u == 160.0 {
                f32::from_bits(0x3dc6d5c0)
            } else {
                u / 1648.0
            },
            v / 288.0,
        ),
    };
    let columns = if name_width == 0.0 {
        vec![(130.0, 0.0), (178.0, 48.0), (1742.0, 64.0), (1790.0, 112.0)]
    } else {
        let edge = (name_width + 40.0) + 178.0;
        vec![
            (130.0, 0.0),
            (178.0, 48.0),
            (edge, 64.0),
            (edge + 96.0, 160.0),
            (1742.0, 176.0),
            (1790.0, 224.0),
        ]
    };
    let (v0, v1) = if name_width == 0.0 {
        (144.0, 256.0)
    } else {
        (0.0, 112.0)
    };
    let vertices = columns
        .into_iter()
        .flat_map(|(x, u)| [make(x, -32.0, u, v0), make(x, 80.0, u, v1)])
        .collect();
    out.push(MessageUiDraw {
        texture: Some(UiTexture::Window),
        vertices,
        indices: Vec::new(),
        native_transform: matrix,
        stencil: Some(stencil),
    });
    let high = (descriptor.height + 0.0) + 32.0;
    let vertices = [(80.0, 16.0), (high - 256.0, 32.0), (high, 288.0)]
        .into_iter()
        .flat_map(|(y, v)| {
            [
                (130.0, 240.0),
                (178.0, 288.0),
                (446.0, 304.0),
                (1790.0, 1648.0),
            ]
            .into_iter()
            .map(move |(x, u)| make(x, y, u, v))
        })
        .collect();
    out.push(MessageUiDraw {
        texture: Some(UiTexture::Window),
        vertices,
        indices: vec![0, 4, 1, 5, 2, 6, 3, 7, 11, 6, 10, 5, 9, 4, 8],
        native_transform: matrix,
        stencil: Some(stencil),
    });
}

pub fn message_ui_draws(
    state: &HigurashiMessagePlayback,
    name_width: f32,
    text_size: Vec2,
    has_glyph_buffer: bool,
    settings: MessageUiSettings,
    parent: &HigurashiTransform,
    stencil: u8,
) -> Vec<MessageUiDraw> {
    let mut out = Vec::new();
    if state.input_blocked {
        return out;
    }
    // Native walks the retained descriptor vector backwards.
    for &descriptor in state.outgoing.iter().rev() {
        window(
            &mut out,
            descriptor,
            state.modal_slide,
            name_width,
            settings,
            parent,
            stencil,
        );
    }
    window(
        &mut out,
        OutgoingWindow {
            style: state.style,
            slide: state.natural_slide,
            height: state.height,
        },
        state.modal_slide,
        name_width,
        settings,
        parent,
        stencil,
    );
    if state.natural_slide * state.modal_slide < 1.0 || !has_glyph_buffer {
        return out;
    }
    let Some(wait) = state.wait else {
        return out;
    };
    let offset = message_offset(
        HigurashiMessageStyle::from_msginit(state.style as i32),
        state.options,
        state.natural_slide,
        state.height,
        text_size,
    );
    let animated = state.style == 3 || state.style == 4;
    let (size, u0, u1, alpha, texture, position) = if animated {
        let frame = ((state.ticks_since_wait * 0.125) as i32 as u32 & 3) as usize;
        let u0 = [0, 1, 2, 1][frame] as f32 / 3.0;
        (
            40.0,
            u0,
            u0 + 1.0 / 3.0,
            1.0,
            if wait == WaitKind::Regular {
                UiTexture::CursorRegular
            } else {
                UiTexture::CursorLast
            },
            vec2(
                offset.x + state.cursor.x,
                (offset.y + state.cursor.y) - 40.0,
            ),
        )
    } else {
        let u0 = if wait == WaitKind::Regular { 0.0 } else { 0.5 };
        let alpha =
            ((state.ticks_since_wait / 60.0) * f32::from_bits(0x40490fd8)).sin() * 0.4 + 0.6;
        let position = if state.style == 2 {
            vec2(1870.0, 1030.0)
        } else {
            vec2(
                offset.x + state.cursor.x,
                (offset.y + state.cursor.y) - 30.0,
            )
        };
        (30.0, u0, u0 + 0.5, alpha, UiTexture::KeyWait, position)
    };
    out.push(MessageUiDraw {
        texture: Some(texture),
        vertices: quad(size, u0, u1, alpha, true),
        indices: Vec::new(),
        native_transform: parent.compute_native_message_transform(position),
        stencil: None,
    });
    out
}

#[cfg(test)]
pub(super) mod tests;
