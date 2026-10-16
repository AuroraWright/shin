//! Updated77254 header/footer and79410 help artwork. The Switch's27560/cc940
//! keeps A/B icon IDs unchanged. Global clocks are outside this draw function.
use crate::render::NATIVE_UI_PROJECTION;
use super::{
    browser_controls::{BrowserControls, RowMetadata},
    browser_decorations,
};
use crate::{
    adv::characters::view::CharacterTextures, layer::render_params::higurashi::HigurashiTransform,
};
use glam::{Mat4, Vec4, vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

#[derive(Clone, Copy)]
pub(super) struct State {
    pub slide: f32,
    pub close_flash: f32,
    pub selected: RowMetadata,
    pub navigation: bool,
}
impl State {
    pub fn from_controls(slide: f32, controls: &BrowserControls) -> Self {
        Self {
            slide,
            close_flash: controls.close_flash(),
            selected: controls.selected_metadata(),
            navigation: controls.navigation() != 0,
        }
    }
}
struct Part {
    name: &'static str,
    rect: Vec4,
    uv: Vec4,
    color: UnormColor,
    flash: f32,
}
impl Part {
    fn new(name: &'static str, rect: Vec4, uv: Vec4, color: UnormColor) -> Self {
        Self {
            name,
            rect,
            uv,
            color,
            flash: 0.0,
        }
    }
    fn vertices(&self) -> [PosColTexVertex; 4] {
        browser_decorations::quad(self.rect, self.uv, self.color)
    }
}
fn help(parts: &mut Vec<Part>, index: usize, color: UnormColor, mut x: f32, y: f32) -> f32 {
    const TABLE: [(i32, i32, i32, i32); 5] = [
        (5, 0, 0, 214),
        (2, -1, 48, 76),
        (1, -1, 96, 146),
        (4, -1, 144, 250),
        (3, -1, 192, 280),
    ];
    let (first, second, top, width) = TABLE[index];
    let width = width as f32;
    x -= width;
    let bottom = y + 48.0;
    let v = top as f32 / 288.0;
    let sixth = f32::from_bits(0x3e2aaaab);
    parts.push(Part::new(
        "help_logview",
        vec4(x, y, x + width, bottom),
        vec4(0.0, v, width / 320.0 + 0.0, v + sixth),
        color,
    ));
    for key in [first, second].into_iter().filter(|k| *k >= 0) {
        x += -48.0;
        // Original27560 leaves IDs unchanged because Switch cc940 returns0.
        let u = key.wrapping_mul(48) as f32 / 288.0;
        parts.push(Part::new(
            "key",
            vec4(x, y, x + 48.0, bottom),
            vec4(u, 0.0, u + sixth, 1.0),
            color,
        ));
    }
    x + -10.0
}
fn parts(state: State) -> Vec<Part> {
    let mut parts = Vec::with_capacity(15);
    let opaque = UnormColor::WHITE;
    let obi = UnormColor(0xd8ffffff);
    let header = state.slide * 150.0 + -150.0;
    parts.push(Part::new(
        "obi",
        vec4(0.0, header, 1920.0, header + 150.0),
        vec4(0.0, 0.0, 1.0, 0.625),
        obi,
    ));
    parts.push(Part::new(
        "caption",
        vec4(72.0, header, 672.0, header + 144.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        opaque,
    ));
    let close_top = header + 30.0;
    let mut close = Part::new(
        "close",
        vec4(1764.0, close_top, 1848.0, close_top + 84.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        opaque,
    );
    close.flash = state.close_flash;
    parts.push(close);
    let footer = 1080.0 - state.slide * 88.0;
    parts.push(Part::new(
        "obi",
        vec4(0.0, footer, 1920.0, footer + 88.0),
        vec4(0.0, f32::from_bits(0x3f222222), 1.0, 1.0),
        obi,
    ));
    let y = footer + 20.0;
    let mut x = help(&mut parts, 1, opaque, 1872.0, y);
    let voice = if state.selected.kind == 0 && state.selected.voice_count != 0 {
        opaque
    } else {
        UnormColor(0xff808080)
    };
    x = help(&mut parts, 2, voice, x, y);
    if !state.navigation {
        x = help(&mut parts, 4, opaque, x, y);
    }
    x = help(&mut parts, 3, opaque, x, y);
    help(&mut parts, 0, opaque, x, y);
    parts
}
pub(super) fn render(pass: &mut RenderPass, atlas: &CharacterTextures, state: State) {
    let matrix = NATIVE_UI_PROJECTION;
    for part in parts(state) {
        let vertices = part.vertices();
        if part.flash > 0.0 {
            pass.run(
                RenderRequestBuilder::new()
                    .color_blend_type(ColorBlendType::Layer1)
                    .build(
                        RenderProgramWithArguments::Button {
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
                            texture: atlas.get(part.name).as_source(),
                            transform: HigurashiTransform::convert_clip_depth(matrix),
                            flash1: Vec4::ZERO,
                            flash2: vec4(1.0, 1.0, 1.0, part.flash),
                        },
                        DrawPrimitive::TrianglesStrip,
                    ),
            );
        } else {
            browser_decorations::sprite(pass, atlas, part.name, &vertices, matrix);
        }
    }
}

#[cfg(test)]
mod tests;
