//! Chapter-list drawing in native ScenarioSelect de540..de8ac.
//! The owner supplies evaluated focus/flash clocks and scroll, and composes
//! the separate details capture, tabs, footer and curtain around this list.
use super::{ScenarioGroup, ScenarioSelectSetup};
use crate::{
    adv::characters::view::CharacterTextures,
    layer::render_params::{DrawableClipMode, DrawableClipParams, TransformParams},
};
use glam::{Mat4, Vec4, vec2, vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, DepthStencilState, DrawPrimitive, RenderProgramWithArguments,
    RenderRequestBuilder, StencilFunction, StencilMask, StencilPipelineState, StencilState,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        texture::TextureSource,
        vertices::{BlendVertex, PosColTexVertex},
    },
};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ChapterFeedback {
    pub focus: f32,
    pub flash: f32,
}

struct ChapterSprite {
    name: &'static str,
    rect: Vec4,
    uv: Vec4,
    feedback: ChapterFeedback,
}

fn sprites(group: &ScenarioGroup, scroll: f32, feedback: &[ChapterFeedback]) -> Vec<ChapterSprite> {
    assert_eq!(group.entries.len(), feedback.len());
    let mut sprites = Vec::new();
    for (entry, &feedback) in group.entries.iter().zip(feedback) {
        let top = 246.0 + entry.y as f32 - scroll;
        let index = entry.menu_index;
        sprites.push(ChapterSprite {
            name: "menu",
            rect: vec4(86.0, top, 526.0, top + 110.0),
            uv: vec4(
                (index >> 3) as f32 * 0.25,
                (index & 7) as f32 * 0.125,
                ((index >> 3) + 1) as f32 * 0.25,
                ((index & 7) + 1) as f32 * 0.125,
            ),
            feedback,
        });
        let badge = match entry.state {
            0 => Some(("lock", 62.0, top + 10.0, 90.0, 90.0)),
            1 => Some(("new", 62.0, top + 35.0, 96.0, 40.0)),
            3 => Some(("star", 76.0, top + 33.0, 44.0, 44.0)),
            _ => None,
        };
        if let Some((name, x, y, width, height)) = badge {
            sprites.push(ChapterSprite {
                name,
                rect: vec4(x, y, x + width, y + height),
                uv: vec4(0.0, 0.0, 1.0, 1.0),
                feedback,
            });
        }
    }
    sprites
}

pub(crate) fn render_chapters(
    group: &ScenarioGroup,
    scroll: f32,
    feedback: &[ChapterFeedback],
    atlas: &CharacterTextures,
    pass: &mut RenderPass,
    transform: Mat4,
) {
    // Native stencil rectangle is five pixels larger than the input viewport
    // at each edge: de4a8..de4f4 encodes 0,241..1920,921.
    let clip = DrawableClipParams {
        mode: DrawableClipMode::Clip,
        area: vec4(0.0, 241.0, 1920.0, 680.0),
    };
    clip.begin(
        pass,
        &TransformParams {
            transform: crate::render::centered_projection_matrix().inverse() * transform,
            ..Default::default()
        },
    );
    let builder = RenderRequestBuilder::new()
        .color_blend_type(ColorBlendType::Layer1)
        .depth_stencil(DepthStencilState {
            stencil: StencilState {
                stencil_reference: 0,
                pipeline: StencilPipelineState {
                    function: StencilFunction::Equal,
                    stencil_read_mask: StencilMask::ClipOnly,
                    ..Default::default()
                },
            },
            ..Default::default()
        });
    for sprite in sprites(group, scroll, feedback) {
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
            position: glam::vec3(
                sprite.rect.x + (sprite.rect.z - sprite.rect.x) * corner.x,
                sprite.rect.y + (sprite.rect.w - sprite.rect.y) * corner.y,
                0.0,
            ),
            texture_position: vec2(
                sprite.uv.x + (sprite.uv.z - sprite.uv.x) * corner.x,
                sprite.uv.y + (sprite.uv.w - sprite.uv.y) * corner.y,
            ),
            color: UnormColor::WHITE,
        });
        pass.run(builder.build(
            RenderProgramWithArguments::Button {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture: atlas.get(sprite.name).as_source(),
                transform,
                flash1: vec4(1.0, 1.0, 1.0, sprite.feedback.focus * 0.5),
                flash2: vec4(1.0, 1.0, 1.0, sprite.feedback.flash),
            },
            DrawPrimitive::TrianglesStrip,
        ));
    }
    clip.end(pass);
}

/// Native dee9c..def0c calls the shared system ScrollBar at (536,246).
pub(crate) fn render_scrollbar(
    group: &ScenarioGroup,
    scroll: f32,
    system: &CharacterTextures,
    pass: &mut RenderPass,
    transform: Mat4,
) {
    crate::adv::scrollbar::render(
        system,
        pass,
        transform,
        crate::adv::scrollbar::slices(group.content_height as f32, scroll, 536.0, 246.0, 670.0),
    );
}

/// Native deb94..dee88: tabs overlap as focus scales them about their centers.
/// Static pointer rectangles are deliberately kept in the input component.
pub(crate) fn render_tabs(
    setup: &ScenarioSelectSetup,
    focus: &[f32],
    flash: &[f32],
    atlas: &CharacterTextures,
    pass: &mut RenderPass,
    transform: Mat4,
) {
    assert_eq!(setup.groups.len(), focus.len());
    assert_eq!(setup.groups.len(), flash.len());
    if setup.groups.len() <= 1 {
        return;
    }
    let mut order: Vec<_> = (0..setup.groups.len()).collect();
    // Native e1980 uses ascending focus. With at most six groups its short
    // insertion-sort path retains original order for equal clocks.
    order.sort_by(|&a, &b| focus[a].partial_cmp(&focus[b]).unwrap());
    for index in order {
        let group = &setup.groups[index];
        let x = (1920.0 - setup.groups.len() as f32 * 300.0) * 0.5 + index as f32 * 300.0;
        let local = Mat4::from_translation(glam::vec3(x + 150.0, 177.75, 0.0))
            * Mat4::from_scale(glam::vec3(
                1.0 + focus[index] * 0.15,
                1.0 + focus[index] * 0.15,
                1.0,
            ))
            * Mat4::from_translation(glam::vec3(-150.0, -33.75, 0.0));
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| {
            let uv = vec2(corner.x, (group.id as f32 + corner.y) / 6.0);
            BlendVertex {
                position: (corner * vec2(300.0, 67.5)).extend(0.0),
                color: UnormColor::WHITE,
                texture_position: vec4(uv.x, uv.y, uv.x, uv.y),
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
                        transform: transform * local,
                        blend: Vec4::splat(focus[index]),
                        flash: vec4(1.0, 1.0, 1.0, flash[index]),
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
        if group.has_new {
            // Native dedf4..dee0c (updated dede0..dedf8): width-96, -8 .. width, 32.
            let vertices =
                shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
                    position: glam::vec3(204.0 + corner.x * 96.0, -8.0 + corner.y * 40.0, 0.0),
                    texture_position: corner,
                    color: UnormColor::WHITE,
                });
            pass.run(
                RenderRequestBuilder::new()
                    .color_blend_type(ColorBlendType::Layer1)
                    .build(
                        RenderProgramWithArguments::Button {
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
                            texture: atlas.get("new").as_source(),
                            transform: transform * local,
                            flash1: Vec4::ZERO,
                            flash2: vec4(1.0, 1.0, 1.0, flash[index]),
                        },
                        DrawPrimitive::TrianglesStrip,
                    ),
            );
        }
    }
}

fn sprite(
    atlas: &CharacterTextures,
    name: &str,
    rect: Vec4,
    uv: Vec4,
    tint: UnormColor,
    pass: &mut RenderPass,
    transform: Mat4,
    blend: ColorBlendType,
) {
    shin_render::quad_vertices::QuadVertices::new()
        .with_box(rect.x, rect.y, rect.z, rect.w)
        .with_tex_box(uv.x, uv.y, uv.z, uv.w)
        .with_color(tint)
        .render_sprite(
            pass,
            RenderRequestBuilder::new().color_blend_type(blend),
            atlas.get(name).as_source(),
            transform,
        );
}

pub(crate) fn render_background(atlas: &CharacterTextures, pass: &mut RenderPass, transform: Mat4) {
    sprite(
        atlas,
        "bg",
        vec4(0.0, 0.0, 1920.0, 1080.0),
        vec4(0.0, 0.0, 1.0, 1.0),
        UnormColor::WHITE,
        pass,
        transform,
        ColorBlendType::Opaque,
    );
}

pub(crate) fn render_close(
    atlas: &CharacterTextures,
    flash: f32,
    pass: &mut RenderPass,
    transform: Mat4,
) {
    let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
        position: glam::vec3(1764.0 + corner.x * 84.0, 32.0 + corner.y * 84.0, 0.0),
        texture_position: corner,
        color: UnormColor::WHITE,
    });
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .build(
                RenderProgramWithArguments::Button {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: atlas.get("close").as_source(),
                    transform,
                    flash1: Vec4::ZERO,
                    flash2: vec4(1.0, 1.0, 1.0, flash),
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}

/// Native1738e8 help rows, called in order1,3,2(if multiple groups),0.
pub(crate) fn render_footer(
    atlas: &CharacterTextures,
    group_count: usize,
    selected_state: i16,
    unlock_available: bool,
    pass: &mut RenderPass,
    transform: Mat4,
) {
    let height = atlas.get("help_snrsel").size().height as f32;
    let rows = [
        (1usize, 206.0),
        (4, 72.0),
        (7, 236.0),
        (3, 74.0),
        (14, 134.0),
    ];
    let mut right = 1872.0;
    for row in [1usize, 3, 2, 0, 4] {
        if row == 2 && group_count < 2 {
            continue;
        }
        if row == 4 && (height != 240.0 || !unlock_available) {
            continue;
        }
        let (key, width) = rows[row];
        let tint = if row == 3 && selected_state == 0 {
            UnormColor::from_rgba(128, 128, 128, 255)
        } else {
            UnormColor::WHITE
        };
        let left = right - width;
        sprite(
            atlas,
            "help_snrsel",
            vec4(left, 1012.0, right, 1060.0),
            vec4(
                0.0,
                row as f32 * 48.0 / height,
                width / 320.0,
                (row + 1) as f32 * 48.0 / height,
            ),
            tint,
            pass,
            transform,
            ColorBlendType::Layer1,
        );
        // fb9f0 returns0 in this executable, so27320 retains authored icons.
        let icon_left = left + 2.0 - 48.0;
        let u = (key & 7) as f32 / 8.0;
        let v = (key >> 3) as f32 / 2.0;
        sprite(
            atlas,
            "key",
            vec4(icon_left, 1012.0, icon_left + 48.0, 1060.0),
            vec4(u, v, u + 0.125, v + 0.5),
            tint,
            pass,
            transform,
            ColorBlendType::Layer1,
        );
        right = icon_left - 16.0;
    }
}

/// Native de8e8..de9c4 overlays the composed panel at both list edges.
pub(crate) fn render_chapter_edges(
    capture: TextureSource<'_>,
    pass: &mut RenderPass,
    transform: Mat4,
) {
    for (top, bottom, invert) in [(241.0, 246.0, false), (916.0, 921.0, true)] {
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| {
            let x = 54.0 + corner.x * 472.0;
            let y = top + corner.y * (bottom - top);
            PosColTexVertex {
                position: glam::vec3(x, y, 0.0),
                texture_position: vec2((x - 48.0) / 1824.0, (y - 218.0) / 726.0),
                color: UnormColor::from_rgba(
                    255,
                    255,
                    255,
                    if (corner.y == 0.0) ^ invert { 255 } else { 0 },
                ),
            }
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: capture,
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::title::scenario_select::ScenarioSelectSetup;
    use shin_core::format::save::SaveData;

    #[test]
    fn signed_details_fallback_and_badges_are_distinct_native_predicates() {
        let mut save = SaveData::default();
        for (slot, state) in [(2, -1), (3, 0), (4, 1), (5, 2), (6, 3), (7, 4)] {
            save.persist_data.set(slot, state);
        }
        let group = &ScenarioSelectSetup::from_save(&save).groups[0];
        assert_eq!(group.entries[1].details_name(), "details_48");
        assert_eq!(group.entries[2].details_name(), "details_48");
        assert_eq!(group.entries[3].details_name(), "details_03");
        let sprites = sprites(group, 37.5, &[ChapterFeedback::default(); 8]);
        let badges: Vec<_> = sprites
            .iter()
            .filter(|sprite| sprite.name != "menu")
            .map(|sprite| sprite.name)
            .collect();
        assert_eq!(badges, ["lock", "new", "star", "lock"]);
        assert_eq!(sprites[0].rect, vec4(86.0, 208.5, 526.0, 318.5));
        assert_eq!(sprites[0].uv, vec4(0.0, 0.0, 0.25, 0.125));
    }
}
