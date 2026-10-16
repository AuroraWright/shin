//! Native ScrollBar eee50: system artwork with eight-pixel caps.
use glam::{Mat4, Vec4, vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};

use super::characters::view::CharacterTextures;

pub(crate) fn slices(
    height: f32,
    scroll: f32,
    left: f32,
    top: f32,
    extent: f32,
) -> Vec<(Vec4, Vec4)> {
    let height = height.ceil();
    let scroll = scroll.trunc();
    if height <= extent || scroll < 0.0 || scroll + extent > height {
        return Vec::new();
    }
    let track = extent - 16.0;
    let start = track * scroll / height;
    let end = track * (scroll + extent) / height;
    let mut result = Vec::with_capacity(6);
    for (u, ys) in [
        (0.0, [top, top + 8.0, top + extent - 8.0, top + extent]),
        (
            0.5,
            [
                top + start,
                top + start + 8.0,
                top + end + 8.0,
                top + end + 16.0,
            ],
        ),
    ] {
        for band in 0..3 {
            result.push((
                vec4(left, ys[band], left + 16.0, ys[band + 1]),
                vec4(u, band as f32 * 0.25, u + 0.5, (band + 1) as f32 * 0.25),
            ));
        }
    }
    result
}

pub(crate) fn render(
    system: &CharacterTextures,
    pass: &mut RenderPass,
    transform: Mat4,
    geometry: Vec<(Vec4, Vec4)>,
) {
    for (rect, uv) in geometry {
        QuadVertices::new()
            .with_box(rect.x, rect.y, rect.z, rect.w)
            .with_tex_box(uv.x, uv.y, uv.z, uv.w)
            .with_color(UnormColor::WHITE)
            .render_sprite(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                system.get("scroll").as_source(),
                transform,
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_only_and_caps_follow_top_middle_and_bottom_in_both_viewports() {
        for (left, top, extent) in [(1788.0, 188.0, 730.0), (536.0, 246.0, 670.0)] {
            assert!(slices(extent, 0.0, left, top, extent).is_empty());
            assert!(slices(2000.0, -1.0, left, top, extent).is_empty());
            assert!(slices(2000.0, 2001.0 - extent, left, top, extent).is_empty());
            for scroll in [0.0, 400.9, 2000.0 - extent] {
                let bands = slices(1999.2, scroll, left, top, extent);
                assert_eq!(bands.len(), 6);
                assert_eq!(bands[0].0, vec4(left, top, left + 16.0, top + 8.0));
                assert_eq!(
                    bands[2].0,
                    vec4(left, top + extent - 8.0, left + 16.0, top + extent)
                );
                assert!(
                    (bands[3].0.y - (top + (extent - 16.0) * scroll.trunc() / 2000.0)).abs()
                        < 0.001
                );
                assert!(
                    (bands[5].0.w
                        - (top + 16.0 + (extent - 16.0) * (scroll.trunc() + extent) / 2000.0))
                        .abs()
                        < 0.001
                );
                assert_eq!(bands, slices(2000.0, scroll.trunc(), left, top, extent));
            }
        }
    }
}
