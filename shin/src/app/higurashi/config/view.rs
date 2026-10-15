//! Updated50c58/4d140/4d9c0 Config parent, independently compared with original
//! packets. The older menu has one scrollable flat list; titles/help are baked
//! artwork. Private First/Question/SaveGame children remain the owner's draws.
use super::{Control, RowState, ViewState};
use crate::{
    adv::{character_view::CharacterTextures, higurashi::browser_decorations},
    app::higurashi::kakera_menu::view::Packet,
    layer::render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
};
use anyhow::{Result, ensure};
use glam::{Mat4, vec3};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

pub(super) enum Draw {
    Quad(Packet),
    Scrollbar {
        matrix: Mat4,
        vertices: [PosColTexVertex; 8],
    },
}
const FULL: [f32; 4] = [0., 0., 1., 1.];
fn projection() -> Mat4 {
    Mat4::from_cols_array(&[
        2. / 1920.,
        0.,
        0.,
        0.,
        0.,
        -2. / 1080.,
        0.,
        0.,
        0.,
        0.,
        -1.,
        0.,
        -1.,
        1.,
        -0.,
        1.,
    ])
}
fn quad(
    out: &mut Vec<Draw>,
    texture: &str,
    matrix: Mat4,
    rect: [f32; 4],
    uv: [f32; 4],
    color: u32,
) {
    out.push(Draw::Quad(Packet {
        texture: texture.to_owned(),
        matrix,
        rect,
        uv,
        color,
        opaque: false,
        flash: 0.,
    }));
}
fn flash(out: &mut Vec<Draw>, texture: &str, matrix: Mat4, rect: [f32; 4], value: f32) {
    quad(out, texture, matrix, rect, FULL, u32::MAX);
    let Some(Draw::Quad(packet)) = out.last_mut() else {
        unreachable!()
    };
    // Actual9ba18 selects5 only when the flash compares strictly above zero.
    packet.flash = value;
}
fn pulse(frame: u32) -> f32 {
    (((frame as i32 as f32) / 60.) * f32::from_bits(0x4049_0fd8)).sin()
        * f32::from_bits(0x3e99_999a)
        + 0.5
}
fn packed_alpha(alpha: f32, rgb: u32) -> u32 {
    //4cc90 first clamps low, truncates *255 and saturates to an unsigned byte.
    ((alpha.max(0.) * 255.) as u32).min(255) << 24 | rgb
}
fn focus(out: &mut Vec<Draw>, matrix: Mat4, y: f32) {
    let top = y + 12.;
    let bottom = (y + 100.) - 12.;
    for (t, b, v, w) in [
        (y, top, 0., 0.375),
        (top, bottom, 0.375, 0.625),
        (bottom, bottom + 12., 0.625, 1.),
    ] {
        quad(
            out,
            "focus",
            matrix,
            [150., t, 1750., b],
            [0., v, 1., w],
            0xc0ff_ffff,
        );
    }
}
fn row(out: &mut Vec<Draw>, row: &RowState, matrix: Mat4, frame: u32) {
    let y = row.y as f32;
    if row.focused {
        focus(out, matrix, y);
    }
    let title_y = y + 14.;
    let title_v = row.title_band.wrapping_mul(72) as i32 as f32 / 1440.;
    quad(
        out,
        "left",
        matrix,
        [190., title_y, 942., title_y + 72.],
        [0., title_v, 1., title_v + f32::from_bits(0x3d4c_cccd)],
        u32::MAX,
    );
    match &row.kind {
        Control::Buttons {
            labels,
            value,
            cursor,
        } => {
            let mut x = 1494i32;
            for (index, label) in labels.iter().enumerate().rev() {
                let left = x as f32;
                let right = left + 240.;
                let selected = index as i32 == *value;
                let u = if selected { 0. } else { 240. } / 720.;
                let t = y + 6.;
                let rect = [left, t, right, t + 88.];
                quad(
                    out,
                    "button",
                    matrix,
                    rect,
                    [u, 0., u + f32::from_bits(0x3eaa_aaab), 1.],
                    u32::MAX,
                );
                if row.focused && index as i32 == *cursor {
                    quad(
                        out,
                        "button",
                        matrix,
                        rect,
                        [f32::from_bits(0x3f2a_aaab), 0., 1., 1.],
                        packed_alpha(pulse(frame), 0x00ff_ffff),
                    );
                }
                let u = if selected { 0. } else { 240. } / 480.;
                let v = label.wrapping_mul(64) as i32 as f32 / 640.;
                let t = y + 18.;
                quad(
                    out,
                    "right",
                    matrix,
                    [left, t, right, t + 64.],
                    [u, v, u + 0.5, v + f32::from_bits(0x3dcc_cccd)],
                    u32::MAX,
                );
                x = x.wrapping_sub(252);
            }
        }
        Control::Slider { subtype, value } => {
            let v = subtype.wrapping_mul(48).wrapping_add(48) as i32 as f32 * (1. / 256.);
            let t = row.y.wrapping_add(10) as f32;
            quad(
                out,
                "slider",
                matrix,
                [1596., t, 1716., t + 48.],
                [
                    f32::from_bits(0x3ef6_84be),
                    v,
                    f32::from_bits(0x3f2a_aaab),
                    v + 0.1875,
                ],
                u32::MAX,
            );
            quad(
                out,
                "slider",
                matrix,
                [1068., t, 1188., t + 48.],
                [
                    f32::from_bits(0x3e3d_a12f),
                    v,
                    f32::from_bits(0x3ebd_a12f),
                    v + 0.1875,
                ],
                u32::MAX,
            );
            let t = row.y.wrapping_add(44) as f32;
            quad(
                out,
                "slider",
                matrix,
                [1068., t, 1716., t + 48.],
                [0., 0., 1., 0.1875],
                u32::MAX,
            );
            let x = value.wrapping_mul(6).wrapping_add(1068) as f32;
            let t = row.y.wrapping_add(9) as f32;
            let rect = [x, t, x + 48., t + 72.];
            quad(
                out,
                "slider",
                matrix,
                rect,
                [
                    f32::from_bits(0x3d97_b426),
                    0.46875,
                    f32::from_bits(0x3e17_b426),
                    0.75,
                ],
                u32::MAX,
            );
            if row.focused {
                quad(
                    out,
                    "slider",
                    matrix,
                    rect,
                    [0., 0.1875, f32::from_bits(0x3d97_b426), 0.46875],
                    packed_alpha(pulse(frame), 0x00ff_ffff),
                );
            }
        }
    }
}
fn hint(out: &mut Vec<Draw>, matrix: Mat4, right: f32, y: f32, id: usize, swapped: bool) -> f32 {
    let (button, band_y, width) = [(0, 0, 214), (2, 48, 76), (5, 96, 248), (4, 144, 180)][id];
    let left = right - width as f32;
    quad(
        out,
        "help_config",
        matrix,
        [left, y, right, y + 48.],
        [
            0.,
            band_y as f32 / 192.,
            width as f32 / 320.,
            band_y as f32 / 192. + 0.25,
        ],
        u32::MAX,
    );
    let key_left = left - 48.;
    let button = if swapped && button == 2 { 1 } else { button };
    let u = (button * 48) as f32 / 288.;
    quad(
        out,
        "key",
        matrix,
        [key_left, y, left, y + 48.],
        [u, 0., u + f32::from_bits(0x3e2a_aaab), 1.],
        u32::MAX,
    );
    key_left - 10.
}
pub(super) fn packets(state: &ViewState, button_layout_swapped: bool) -> Vec<Draw> {
    let mut out = Vec::new();
    if !state.ready {
        return out;
    }
    let projection = projection();
    let x = 1920. - state.progress * 1920.;
    let y = state.scroll + (-210.);
    let content = multiply(projection, Mat4::from_translation(vec3(x, -y, 0.)));
    quad(
        &mut out,
        "",
        content,
        [150., 0., 1750., state.content_height as f32],
        FULL,
        0xc0c0_c0c0,
    );
    let height = state.content_height as f32;
    quad(
        &mut out,
        "line",
        content,
        [150., height, 1750., height + 12.],
        FULL,
        u32::MAX,
    );
    for item in &state.rows {
        row(&mut out, item, content, state.frame);
    }
    for item in &state.headings {
        let y = item.y.wrapping_sub(60) as f32;
        let v = item.band.wrapping_mul(64) as i32 as f32 * (1. / 256.);
        quad(
            &mut out,
            "category",
            content,
            [150., y, 1750., y + 62.],
            [0., v, 1., v + 0.2421875],
            u32::MAX,
        );
    }
    for &y in &state.dividers {
        quad(
            &mut out,
            "separator",
            content,
            [150., y as f32, 1750., y as f32 + 6.],
            FULL,
            u32::MAX,
        );
    }
    let scrollbar_parent = multiply(
        projection,
        Mat4::from_translation(vec3(x + 1848., 210., 0.)),
    );
    let matrix = browser_decorations::scrollbar_transform(scrollbar_parent, 0., 0.);
    for vertices in browser_decorations::scrollbar(770., state.scrollbar) {
        out.push(Draw::Scrollbar { matrix, vertices });
    }
    let y = state.progress * 150. + (-150.);
    quad(
        &mut out,
        "obi",
        projection,
        [0., y, 1920., y + 150.],
        [0., 0., 1., 0.625],
        0xd8ff_ffff,
    );
    quad(
        &mut out,
        "caption",
        projection,
        [72., y, 672., y + 144.],
        FULL,
        u32::MAX,
    );
    let t = y + 30.;
    let b = t + 84.;
    flash(
        &mut out,
        "close",
        projection,
        [1764., t, 1848., b],
        state.close_flash,
    );
    flash(
        &mut out,
        "default",
        projection,
        [1446., t, 1752., b],
        state.reset_flash,
    );
    let y = 1080. - state.progress * 88.;
    quad(
        &mut out,
        "obi",
        projection,
        [0., y, 1920., y + 88.],
        [0., f32::from_bits(0x3f22_2222), 1., 1.],
        0xd8ff_ffff,
    );
    let y = y + 20.;
    let x = hint(&mut out, projection, 1872., y, 1, button_layout_swapped);
    let x = hint(&mut out, projection, x, y, 3, button_layout_swapped);
    hint(&mut out, projection, x, y, 0, button_layout_swapped);
    out
}

pub(super) struct View;
impl View {
    pub fn new() -> Self {
        Self
    }
    pub fn render(
        &self,
        state: &ViewState,
        atlas: Option<&CharacterTextures>,
        system: &CharacterTextures,
        button_layout_swapped: bool,
        pass: &mut RenderPass,
    ) -> Result<()> {
        ensure!(
            !state.ready || atlas.is_some(),
            "Ready Config has no published native atlas"
        );
        for draw in packets(state, button_layout_swapped) {
            match draw {
                Draw::Quad(packet) => packet.render(pass, atlas),
                Draw::Scrollbar { matrix, vertices } => pass.run(
                    RenderRequestBuilder::new()
                        .color_blend_type(ColorBlendType::Layer1)
                        .build(
                            RenderProgramWithArguments::Sprite {
                                vertices: VertexSource::VertexData {
                                    vertices: &vertices,
                                },
                                sprite: system.get("scroll").as_source(),
                                transform: HigurashiTransform::convert_clip_depth(matrix),
                            },
                            DrawPrimitive::TrianglesStrip,
                        ),
                ),
            }
        }
        Ok(())
    }
}
