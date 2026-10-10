//! ConfigMenu5b5c0 shell and authored Button/SliderMenuItem artwork.
use glam::{Mat4, Vec4, vec2, vec3, vec4};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        vertices::{BlendVertex, PosColTexVertex},
    },
};

use super::{
    character_text::CharacterText,
    character_view::CharacterTextures,
    config::{ConfigModel, ConfigPhase},
    config_widgets::{ConfigControlKind, ConfigWidget},
};

/// An authored atlas region, before the screen's content transform.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigArtwork {
    pub texture: &'static str,
    pub rect: Vec4,
    pub uv: Vec4,
    pub alpha: f32,
    /// Native Button shader's two successive white flashes.
    pub flashes: Option<[f32; 2]>,
}
impl ConfigArtwork {
    fn sprite(texture: &'static str, rect: Vec4, uv: Vec4) -> Self {
        Self {
            texture,
            rect,
            uv,
            alpha: 1.0,
            flashes: None,
        }
    }
    fn button(texture: &'static str, rect: Vec4, uv: Vec4, selection: f32, flash: f32) -> Self {
        Self {
            flashes: Some([selection * 0.5, flash]),
            ..Self::sprite(texture, rect, uv)
        }
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        atlas: &CharacterTextures,
        transform: Mat4,
        builder: RenderRequestBuilder,
    ) {
        let color = FloatColor4::from_rgba(1.0, 1.0, 1.0, self.alpha).into_unorm();
        let quad = QuadVertices::new()
            .with_box(self.rect.x, self.rect.y, self.rect.z, self.rect.w)
            .with_tex_box(self.uv.x, self.uv.y, self.uv.z, self.uv.w)
            .with_color(color);
        if let Some([first, second]) = self.flashes {
            quad.render_button(
                pass,
                builder,
                atlas.get(self.texture).as_source(),
                transform,
                vec4(1.0, 1.0, 1.0, first),
                vec4(1.0, 1.0, 1.0, second),
            );
        } else {
            quad.render_sprite(
                pass,
                builder,
                atlas.get(self.texture).as_source(),
                transform,
            );
        }
    }
}

/// Native focus57730 uses unusually wide512px caps and12px vertical caps.
fn focus_artwork(y: f32) -> impl Iterator<Item = ConfigArtwork> {
    let xs = [0.0, 512.0, 1136.0, 1648.0];
    let ys = [y, y + 12.0, y + 76.0, y + 88.0];
    let us = [0.0, 0.4, 0.6, 1.0];
    let vs = [0.0, 0.375, 0.625, 1.0];
    (0..3).flat_map(move |r| {
        (0..3).map(move |c| ConfigArtwork {
            alpha: 0.75,
            ..ConfigArtwork::sprite(
                "focus",
                vec4(xs[c], ys[r], xs[c + 1], ys[r + 1]),
                vec4(us[c], vs[r], us[c + 1], vs[r + 1]),
            )
        })
    })
}

/// Draw order matters: focus, title, then right-to-left choices or slider caps,
/// track and knob. Focus appears immediately; only control flashes interpolate.
pub fn row_artwork(widget: &ConfigWidget) -> Vec<ConfigArtwork> {
    let spec = widget.spec;
    let y = spec.y as f32;
    let mut result = Vec::new();
    if widget.focused() {
        result.extend(focus_artwork(y));
    }
    let v = spec.title_band as f32 / 20.0;
    result.push(ConfigArtwork::sprite(
        "left",
        vec4(24.0, y + 8.0, 776.0, y + 80.0),
        vec4(0.0, v, 1.0, v + 0.05),
    ));
    match spec.kind {
        ConfigControlKind::Buttons(labels) => {
            for choice in [1, 0] {
                let x = 1132.0 + choice as f32 * 252.0;
                let selected = choice == widget.value() as usize;
                let selection = widget.selection(choice);
                let flash = widget.flash(choice);
                result.push(ConfigArtwork::button(
                    if selected { "button_on" } else { "button_off" },
                    vec4(x, y, x + 240.0, y + 88.0),
                    Vec4::new(0.0, 0.0, 1.0, 1.0),
                    selection,
                    flash,
                ));
                let u = if selected { 0.5 } else { 0.0 };
                let v = labels[choice] as f32 / 10.0;
                result.push(ConfigArtwork::button(
                    "right",
                    vec4(x, y + 12.0, x + 240.0, y + 76.0),
                    vec4(u, v, u + 0.5, v + 0.1),
                    selection,
                    flash,
                ));
            }
        }
        ConfigControlKind::Slider(subtype) => {
            let v = (subtype as f32 + 1.0) * 0.25;
            result.push(ConfigArtwork::sprite(
                "slider",
                vec4(1504.0, y + 4.0, 1624.0, y + 52.0),
                vec4(312.0 / 648.0, v, 432.0 / 648.0, v + 0.25),
            ));
            result.push(ConfigArtwork::sprite(
                "slider",
                vec4(976.0, y + 4.0, 1096.0, y + 52.0),
                vec4(120.0 / 648.0, v, 240.0 / 648.0, v + 0.25),
            ));
            result.push(ConfigArtwork::sprite(
                "slider",
                vec4(976.0, y + 40.0, 1624.0, y + 88.0),
                vec4(0.0, 0.0, 1.0, 0.25),
            ));
            let x = 976.0 + widget.value() as f32 * 6.0;
            result.push(ConfigArtwork::button(
                "slider",
                vec4(x, y + 5.0, x + 48.0, y + 77.0),
                vec4(48.0 / 648.0, 0.625, 96.0 / 648.0, 1.0),
                widget.selection(0),
                0.0,
            ));
        }
    }
    result
}

/// Divider5c630 shares the row stencil and has8px end caps.
pub fn divider_artwork(y: f32) -> impl Iterator<Item = ConfigArtwork> {
    let xs = [0.0, 8.0, 1640.0, 1648.0];
    let us = [0.0, 0.25, 0.75, 1.0];
    (0..3).map(move |c| {
        ConfigArtwork::sprite(
            "line",
            vec4(xs[c], y, xs[c + 1], y + 14.0),
            vec4(us[c], 0.0, us[c + 1], 1.0),
        )
    })
}

pub fn render_rows(
    widgets: &[ConfigWidget],
    atlas: &CharacterTextures,
    pass: &mut RenderPass,
    transform: Mat4,
) {
    render_rows_with_builder(
        widgets,
        atlas,
        pass,
        transform,
        RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
    );
}
fn render_rows_with_builder(
    widgets: &[ConfigWidget],
    atlas: &CharacterTextures,
    pass: &mut RenderPass,
    transform: Mat4,
    builder: RenderRequestBuilder,
) {
    for widget in widgets {
        for artwork in row_artwork(widget) {
            artwork.render(pass, atlas, transform, builder);
        }
    }
    for widget in widgets {
        for artwork in divider_artwork(widget.spec.y as f32 + 88.0) {
            artwork.render(pass, atlas, transform, builder);
        }
    }
}

pub const CONTENT_CLIP: Vec4 = vec4(136.0, 247.0, 1648.0, 676.0);

pub fn window_slices() -> Vec<(Vec4, Vec4, ColorBlendType)> {
    let xs = [0.0, 80.0, 1744.0, 1824.0];
    let ys = [0.0, 80.0, 746.0, 834.0];
    (0..3)
        .flat_map(|r| {
            (0..3).map(move |c| {
                (
                    vec4(
                        48.0 + xs[c],
                        176.0 + ys[r],
                        48.0 + xs[c + 1],
                        176.0 + ys[r + 1],
                    ),
                    vec4(
                        xs[c] / 1824.0,
                        ys[r] / 834.0,
                        xs[c + 1] / 1824.0,
                        ys[r + 1] / 834.0,
                    ),
                    if r == 1 && c == 1 {
                        ColorBlendType::Opaque
                    } else {
                        ColorBlendType::Layer1
                    },
                )
            })
        })
        .collect()
}

/// Native sort5e6f0 draws lower selection values first, retaining equal order.
pub fn tab_order(model: &ConfigModel) -> [usize; 3] {
    let mut result = [0, 1, 2];
    result.sort_by(|&a, &b| {
        model.tabs()[a]
            .selection()
            .total_cmp(&model.tabs()[b].selection())
    });
    result
}
pub fn tab_geometry(index: usize, selection: f32) -> (Vec4, Vec4) {
    let x = 510.0 + index as f32 * 300.0;
    let extra_x = 150.0 * 0.15 * selection;
    let extra_y = 33.75 * 0.15 * selection;
    (
        vec4(
            x - extra_x,
            144.0 - extra_y,
            x + 300.0 + extra_x,
            211.5 + extra_y,
        ),
        vec4(0.0, index as f32 / 3.0, 1.0, index as f32 / 3.0 + 1.0 / 3.0),
    )
}
fn render_tabs(
    model: &ConfigModel,
    atlas: &CharacterTextures,
    pass: &mut RenderPass,
    transform: Mat4,
) {
    for index in tab_order(model) {
        let selection = model.tabs()[index].selection();
        let (rect, uv) = tab_geometry(index, selection);
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| {
            let p = vec2(
                uv.x + (uv.z - uv.x) * corner.x,
                uv.y + (uv.w - uv.y) * corner.y,
            );
            BlendVertex {
                position: vec3(
                    rect.x + (rect.z - rect.x) * corner.x,
                    rect.y + (rect.w - rect.y) * corner.y,
                    0.0,
                ),
                texture_position: vec4(p.x, p.y, p.x, p.y),
                color: UnormColor::WHITE,
            }
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(
                    RenderProgramWithArguments::Blend {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        texture1: atlas.get("tab_off").as_source(),
                        texture2: atlas.get("tab_on").as_source(),
                        transform,
                        blend: Vec4::splat(selection),
                        flash: vec4(1.0, 1.0, 1.0, model.tabs()[index].flash()),
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}
fn render_edge_fades(atlas: &CharacterTextures, pass: &mut RenderPass, transform: Mat4) {
    for (y1, y2, v1, v2, a, b) in [
        (247.0, 252.0, 71.0 / 834.0, 76.0 / 834.0, 1.0, 0.0),
        (918.0, 923.0, 742.0 / 834.0, 747.0 / 834.0, 0.0, 1.0),
    ] {
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
            position: vec3(136.0 + 1648.0 * corner.x, y1 + (y2 - y1) * corner.y, 0.0),
            texture_position: vec2(
                88.0 / 1824.0 + (1648.0 / 1824.0) * corner.x,
                v1 + (v2 - v1) * corner.y,
            ),
            color: FloatColor4::from_rgba(1.0, 1.0, 1.0, a + (b - a) * corner.y).into_unorm(),
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: atlas.get("window").as_source(),
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}
fn hint(
    atlas: &CharacterTextures,
    pass: &mut RenderPass,
    right: f32,
    y: f32,
    row: usize,
    canvas: Mat4,
) -> f32 {
    let (key, width) = [(1, 206.0), (4, 72.0), (7, 236.0), (6, 174.0), (2, 174.0)][row];
    let left = right - width;
    let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
    ConfigArtwork::sprite(
        "help_config",
        vec4(left, y, right, y + 48.0),
        vec4(0.0, row as f32 / 5.0, width / 320.0, (row + 1) as f32 / 5.0),
    )
    .render(pass, atlas, canvas, builder);
    let icon_left = left - 46.0;
    ConfigArtwork::sprite(
        "key",
        vec4(icon_left, y, icon_left + 48.0, y + 48.0),
        vec4(
            (key & 7) as f32 / 8.0,
            (key >> 3) as f32 / 2.0,
            (key & 7) as f32 / 8.0 + 0.125,
            (key >> 3) as f32 / 2.0 + 0.5,
        ),
    )
    .render(pass, atlas, canvas, builder);
    icon_left - 16.0
}

/// Native artwork shell. Child dialogs belong to ConfigScreen.
pub fn render(model: &ConfigModel, atlas: &CharacterTextures, pass: &mut RenderPass, canvas: Mat4) {
    if model.phase() == ConfigPhase::Closed {
        return;
    }
    let p = model.progress();
    let top = -160.0 + 160.0 * p;
    let normal = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
    ConfigArtwork {
        alpha: p,
        ..ConfigArtwork::sprite(
            "bg",
            vec4(0.0, 0.0, 1920.0, 1080.0),
            vec4(0.0, 0.0, 1.0, 1.0),
        )
    }
    .render(pass, atlas, canvas, normal);
    for (rect, uv) in [
        (
            vec4(0.0, top, 1920.0, top + 160.0),
            vec4(0.0, 0.0, 1.0, 0.5),
        ),
        (
            vec4(0.0, 1080.0 - 160.0 * p, 1920.0, 1240.0 - 160.0 * p),
            vec4(0.0, 0.5, 1.0, 1.0),
        ),
    ] {
        ConfigArtwork {
            alpha: 0.25,
            ..ConfigArtwork::sprite("obi", rect, uv)
        }
        .render(
            pass,
            atlas,
            canvas,
            normal.color_blend_type(ColorBlendType::Layer2),
        );
    }
    let body = canvas * Mat4::from_translation(vec3(1920.0 * (1.0 - p), 0.0, 0.0));
    for (rect, uv, blend) in window_slices() {
        ConfigArtwork::sprite("window", rect, uv).render(
            pass,
            atlas,
            body,
            normal.color_blend_type(blend),
        );
    }
    let (clip, builder) = CharacterText::begin_clip(pass, body, CONTENT_CLIP);
    render_rows_with_builder(
        &model.tabs()[model.selected_tab()].widgets,
        atlas,
        pass,
        body * Mat4::from_translation(vec3(136.0, 252.0 - model.scroll(), 0.0)),
        builder,
    );
    clip.end(pass);
    render_edge_fades(atlas, pass, body);
    render_tabs(model, atlas, pass, body);
    ConfigArtwork::sprite(
        "caption",
        vec4(480.0, top, 1440.0, top + 128.0),
        vec4(0.0, 0.0, 1.0, 1.0),
    )
    .render(pass, atlas, canvas, normal);
    for (texture, x1, x2, flash) in [
        ("close", 1764.0, 1848.0, model.close_flash()),
        ("default", 1446.0, 1752.0, model.reset_flash()),
    ] {
        ConfigArtwork::button(
            texture,
            vec4(x1, top + 32.0, x2, top + 116.0),
            vec4(0.0, 0.0, 1.0, 1.0),
            0.0,
            flash,
        )
        .render(pass, atlas, canvas, normal);
    }
    let y = 1100.0 - 88.0 * p;
    let mut right = 1872.0;
    // Both native widget classes add hints4 then0 at vtable offset104.
    for row in [1, 3, 2, 4, 0] {
        right = hint(atlas, pass, right, y, row, canvas);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adv::config_widgets::CONFIG_ROWS;
    use shin_core::{format::save::Settings, time::Ticks};

    #[test]
    fn authored_rows_preserve_subtype_endpoints_choice_columns_and_animation() {
        let mut settings = Settings::default();
        for (page, rows) in CONFIG_ROWS.into_iter().enumerate() {
            for &spec in rows {
                let mut widget = ConfigWidget::new(spec, &settings);
                widget.focus(true, true);
                widget.update(Ticks::from_f32(2.0));
                let artwork = row_artwork(&widget);
                assert_eq!(artwork.len(), 14);
                assert_eq!(
                    artwork[4].rect,
                    vec4(512.0, spec.y as f32 + 12.0, 1136.0, spec.y as f32 + 76.0)
                );
                assert_eq!(artwork[4].alpha, 0.75);
                assert_eq!(artwork[9].texture, "left");
                assert_eq!(artwork[9].uv.y, spec.title_band as f32 / 20.0);
                match spec.kind {
                    ConfigControlKind::Slider(subtype) => {
                        assert_eq!(artwork[10].uv.y, (subtype as f32 + 1.0) / 4.0);
                        assert_eq!(artwork[13].flashes, Some([0.25, 0.0]));
                        for value in [0, 50, 100] {
                            let mut s = Settings::default();
                            match page {
                                0 => s.v3_sysvol = value,
                                1 => s.v9_msgspeed = value,
                                2 => s.v10_skipspeed = value,
                                _ => unreachable!(),
                            }
                            let knob = row_artwork(&ConfigWidget::new(rows[0], &s)).pop().unwrap();
                            assert_eq!(knob.rect.x, 976.0 + value as f32 * 6.0);
                            assert_eq!(knob.rect.z, 1024.0 + value as f32 * 6.0);
                        }
                    }
                    ConfigControlKind::Buttons(labels) => {
                        for (offset, choice) in [(10, 1), (12, 0)] {
                            let selected = widget.value() as usize == choice;
                            assert_eq!(artwork[offset].rect.x, 1132.0 + choice as f32 * 252.0);
                            assert_eq!(
                                artwork[offset].texture,
                                if selected { "button_on" } else { "button_off" }
                            );
                            assert_eq!(artwork[offset + 1].uv.x, if selected { 0.5 } else { 0.0 });
                            assert_eq!(artwork[offset + 1].uv.y, labels[choice] as f32 / 10.0);
                        }
                        widget.probe(1140.0, spec.y as f32 + 40.0);
                        widget.press_captured(1140.0, spec.y as f32 + 40.0);
                        widget.update(Ticks::from_f32(1.0));
                        assert_eq!(row_artwork(&widget)[12].flashes.unwrap()[1], 0.2);
                        widget.release(&mut settings);
                    }
                }
                widget.focus(false, false);
                assert_eq!(row_artwork(&widget).len(), 5);
            }
        }
        let divider: Vec<_> = divider_artwork(598.0).collect();
        assert_eq!(divider[1].rect, vec4(8.0, 598.0, 1640.0, 612.0));
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_config_row_artwork_constants_and_draw_calls() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        // Focus gate/alpha, title band strides, knob positioning, nine-slice
        // caps and divider geometry; fixed words also retain native draw calls.
        for (a, expected) in [
            (0x5738c, 0x39414268),
            (0x57390, 0x340002e8),
            (0x573c4, 0x1e2d1003),
            (0x57444, 0x1e271000),
            (0x57454, 0x0b160ec8),
            (0x57458, 0x531d7108),
            (0x58470, 0x1e271000),
            (0x58480, 0x0b150ea8),
            (0x58484, 0x531d7108),
            (0x5863c, 0x321f07e9),
            (0x58640, 0x1b097d08),
            (0x58644, 0x110f4108),
            (0x58650, 0x11001508),
            (0x58678, 0x1e2c9001),
            (0x586cc, 0x9401ba25),
            (0x5777c, 0xbd4a2d02),
            (0x5778c, 0x1e251002),
            (0x57794, 0xbd463100),
            (0x577c0, 0x1e351000),
            (0x5782c, 0x1e2b1003),
            (0x578f0, 0x1e2c9003),
            (0x57934, 0x1e2c9003),
            (0x57974, 0x4eaf1de3),
            (0x5c668, 0x1e241000),
            (0x5c678, 0x1e341001),
            (0x5c6b4, 0x1e259000),
        ] {
            assert_eq!(word(a), expected, "Native Config artwork{a:x}");
        }
        for (a, value) in [
            (0x159a74, 1648.0_f32),
            (0x159940, 88.0),
            (0x159ab4, 72.0),
            (0x1597f4, 776.0),
            (0x1599d4, 1440.0),
            (0x159724, 0.05),
            (0x159388, 1504.0),
            (0x1593f4, 1624.0),
            (0x159a1c, 192.0),
            (0x159510, 312.0 / 648.0),
            (0x1593ec, 432.0 / 648.0),
            (0x159674, 976.0),
            (0x15972c, 1096.0),
            (0x159418, 120.0 / 648.0),
            (0x1595f0, 240.0 / 648.0),
            (0x1599a0, 48.0 / 648.0),
            (0x159838, 96.0 / 648.0),
            (0x159a2c, 512.0),
            (0x159630, -512.0),
            (0x15990c, 0.4),
            (0x159538, 0.6),
        ] {
            assert_eq!(word(a), value.to_bits(), "Native Config scalar{a:x}");
        }
        for (a, name) in [
            (0x15a4a3, b"focus\0".as_slice()),
            (0x162f8b, b"slider\0".as_slice()),
            (0x160cc9, b"left\0".as_slice()),
            (0x15df6d, b"line\0".as_slice()),
        ] {
            assert_eq!(&exe[a + 256..a + 256 + name.len()], name);
        }
    }

    #[test]
    fn shell_window_slices_and_tab_overlap_follow_native_transitions() {
        let slices = window_slices();
        assert_eq!(slices.len(), 9);
        assert_eq!(
            slices[4],
            (
                vec4(128.0, 256.0, 1792.0, 922.0),
                vec4(80.0 / 1824.0, 80.0 / 834.0, 1744.0 / 1824.0, 746.0 / 834.0),
                ColorBlendType::Opaque
            )
        );
        assert_eq!(slices[8].0, vec4(1792.0, 922.0, 1872.0, 1010.0));
        assert_eq!(CONTENT_CLIP, vec4(136.0, 247.0, 1648.0, 676.0));
        let (rect, uv) = tab_geometry(2, 1.0);
        assert_eq!(rect, vec4(1087.5, 138.9375, 1432.5, 216.5625));
        assert_eq!(uv, vec4(0.0, 2.0 / 3.0, 1.0, 1.0));
        let mut model = ConfigModel::new(Settings::default());
        assert_eq!(tab_order(&model), [1, 2, 0]);
        model.update(10.0);
        model.change_tab(2);
        model.update(2.0);
        assert_eq!(tab_order(&model), [1, 0, 2]);
        model.update(2.0);
        assert_eq!(tab_order(&model), [0, 1, 2]);
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_config_window_tabs_clip_and_footer() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, expected) in [
            (0x5b898, 0x940002b2),
            (0x5b900, 0xd2a86108),
            (0x5b904, 0xf2e86ee8),
            (0x5b90c, 0xd2a89be8),
            (0x5b910, 0xf2d80008),
            (0x5b914, 0xf2e88cc8),
            (0x5b924, 0xf2e86ee8),
            (0x5b928, 0x5298000a),
            (0x5b92c, 0x72a88cca),
            (0x5b994, 0x1e214121),
            (0x5b9f0, 0xf9403108),
            (0x5bc88, 0x94000a9a),
            (0x5bd1c, 0x1e2e09a0),
            (0x5bd20, 0x1e282800),
            (0x5bed8, 0x97ffea16),
            (0x5bef4, 0x97ffea17),
            (0x5bf0c, 0x9401ac99),
            (0x5edc0, 0x1e2a2120),
            (0x5edc4, 0x54000105),
            (0x5c1e4, 0x320003e2),
            (0x5c1f8, 0x320007e2),
            (0x5c20c, 0x321f03e2),
            (0x5c234, 0xf9403508),
            (0x57a80, 0x321e03e2),
            (0x57a9c, 0x2a1f03e2),
            (0x58724, 0x4ea11c28),
            (0x5873c, 0x2a1f03e2),
            (0x57ba8, 0x1e201000),
            (0x57bac, 0x1e20294a),
            (0x57bbc, 0xbd49c500),
            (0x57ce4, 0x1e361000),
            (0x5685c, 0x6ea1b800),
        ] {
            assert_eq!(word(a), expected, "Native Config shell{a:x}");
        }
        for (a, value) in [
            (0x1595d4, 1744.0_f32),
            (0x159574, 1824.0),
            (0x1595ac, 834.0),
            (0x1599e4, 80.0 / 1824.0),
            (0x159b00, 1744.0 / 1824.0),
            (0x159ac4, 176.0),
            (0x15935c, 0.15),
            (0x15949c, 1.0 / 3.0),
            (0x159340, 136.0),
            (0x159398, 88.0 / 1824.0),
            (0x159868, 1736.0 / 1824.0),
            (0x1595f4, 71.0 / 834.0),
            (0x159924, 76.0 / 834.0),
            (0x159358, 742.0 / 834.0),
            (0x1595f8, 747.0 / 834.0),
            (0x159a24, -88.0),
            (0x1599d0, 1872.0),
            (0x1599c4, -48.0),
        ] {
            assert_eq!(word(a), value.to_bits(), "Native Config shell scalar{a:x}");
        }
        for (i, y) in [0.0_f32, 80.0, 746.0, 834.0].into_iter().enumerate() {
            assert_eq!(word(0x16dd90 + i * 4), y.to_bits());
        }
        for (i, row) in [
            [1_i32, -1, 0, 206],
            [4, -1, 48, 72],
            [7, -1, 96, 236],
            [6, -1, 144, 174],
            [2, -1, 192, 174],
        ]
        .into_iter()
        .enumerate()
        {
            for (j, value) in row.into_iter().enumerate() {
                assert_eq!(word(0x16e47c + i * 16 + j * 4), value as u32);
            }
        }
    }
}
