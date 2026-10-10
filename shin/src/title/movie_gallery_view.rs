//! Native MovieMode883b0 artwork; completion text/captured frame belong to owner.
use super::{
    movie_gallery::MovieGalleryList,
    view::{rgba, sprite},
};
use crate::adv::character_view::CharacterTextures;
use glam::{Vec4, vec4};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};

pub(super) fn render_artwork(
    model: &MovieGalleryList,
    atlas: &CharacterTextures,
    pass: &mut RenderPass,
) {
    let transform = crate::render::top_left_projection_matrix();
    let full = vec4(0.0, 0.0, 1.0, 1.0);
    let opaque = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque);
    QuadVertices::new()
        .with_box(0.0, 0.0, 1920.0, 1080.0)
        .render_sprite(pass, opaque, atlas.get("bg").as_source(), transform);
    for (index, row) in model.rows().iter().enumerate() {
        let (focus, flash) = model.feedback(index);
        let [left, top, right, bottom] = row.thumbnail_bounds();
        let grow_x = (right - left) * 0.05 * focus * 0.5;
        let grow_y = (bottom - top) * 0.05 * focus * 0.5;
        let rect = vec4(left - grow_x, top - grow_y, right + grow_x, bottom + grow_y);
        let quad = QuadVertices::new().with_box(rect.x, rect.y, rect.z, rect.w);
        let name = format!("mv{:02}", row.movie.into_i32() + 1);
        quad.render_sprite(pass, opaque, atlas.get(&name).as_source(), transform);
        if !row.unlocked {
            sprite(
                pass,
                atlas.get("logo").as_source(),
                rect,
                full,
                UnormColor::WHITE,
                transform,
            );
        }
        //887bc/88804 use untextured c6d50, preserving neither texture alpha nor
        //the logo mask. Native byte-packed colors truncate both white overlays.
        for alpha in [focus * 0.5, flash] {
            if alpha > 0.0 {
                quad.with_color(rgba(1.0, 1.0, 1.0, alpha)).fill(
                    pass,
                    RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                    transform,
                );
            }
        }
    }
    atlas.render_button(
        pass,
        "close",
        vec4(1764.0, 32.0, 1848.0, 116.0),
        full,
        UnormColor::WHITE,
        Vec4::ZERO,
        vec4(1.0, 1.0, 1.0, model.close_flash()),
        transform,
    );
    //170b0c records key index, labelY and width. Native27320 keeps authored
    //3/4 keys because fb9f0 returns0.88bc0 returns iconLeft-10 for next hint.
    let hints = [(0, 206.0), (4, 72.0), (3, 76.0)];
    let mut right = 1872.0;
    for row in [1usize, 2, 0] {
        let (key, width) = hints[row];
        let tint = if row == 2 && !model.rows()[model.selected()].unlocked {
            UnormColor::from_rgba(128, 128, 128, 255)
        } else {
            UnormColor::WHITE
        };
        let left = right - width;
        sprite(
            pass,
            atlas.get("help_moviemode").as_source(),
            vec4(left, 1012.0, right, 1060.0),
            vec4(0.0, row as f32 / 3.0, width / 320.0, (row + 1) as f32 / 3.0),
            tint,
            transform,
        );
        let icon_left = left + 2.0 - 48.0;
        let u = (key & 7) as f32 / 8.0;
        let v = (key >> 3) as f32 / 2.0;
        sprite(
            pass,
            atlas.get("key").as_source(),
            vec4(icon_left, 1012.0, icon_left + 48.0, 1060.0),
            vec4(u, v, u + 0.125, v + 0.5),
            tint,
            transform,
        );
        right = icon_left - 10.0;
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires supplied ROM/executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_movie_artwork_geometry_programs_and_footer() {
    use shin_core::{
        format::{rom::RomReader, texture_archive::read_texture_archive},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
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
        (0x88444, 0x320003e0),
        (0x88454, 0x91125821),
        (0x88540, 0x9400cbd8),
        (0x8854c, 0x9400cbd5),
        (0x88558, 0x1e200920),
        (0x8855c, 0x1e2d2809),
        (0x8859c, 0x1e2009c0),
        (0x885a0, 0x1e2109e1),
        (0x885c0, 0x940066ac),
        (0x886c4, 0x11000502),
        (0x886f4, 0x320003e0),
        (0x88760, 0x321f03e0),
        (0x8876c, 0x34000148),
        (0x88790, 0x9400f9a0),
        (0x887c0, 0x1e200823),
        (0x887f4, 0x9400f957),
        (0x88834, 0x9400f947),
        (0x888e4, 0x9400f8f3),
        (0x8893c, 0x9400f975),
        (0x8895c, 0x320003e2),
        (0x88994, 0x94000023),
        (0x889a8, 0x9400001e),
        (0x88a4c, 0x912c3129),
        (0x88ab8, 0x9400f87e),
        (0x88adc, 0x1e201982),
        (0x88af0, 0x910003e0),
        (0x88b0c, 0x1e201000),
        (0x88b18, 0x1e202940),
        (0x88b20, 0x1e21280a),
        (0x88bb8, 0x1e349000),
        (0x88bbc, 0x1e202940),
        (0xc6dd8, 0x320003e0),
    ] {
        assert_eq!(word(a), expected, "native MovieMode draw{a:x}");
    }
    for (a, value) in [
        (0x159724, 0.05f32),
        (0x159548, 384.0),
        (0x15982c, 216.0),
        (0x1599d0, 1872.0),
        (0x15965c, 1012.0),
        (0x159a7c, 144.0),
        (0x159444, 320.0),
        (0x15949c, 1.0 / 3.0),
        (0x1599c4, -48.0),
    ] {
        assert_eq!(
            word(a),
            value.to_bits(),
            "native MovieMode artwork constant{a:x}"
        );
    }
    let hints = [0i32, 0, 206, 4, 48, 72, 3, 96, 76];
    for (i, value) in hints.into_iter().enumerate() {
        assert_eq!(word(0x170b0c + i * 4), value as u32);
    }
    for (a, text) in [
        (0x15a496, "bg"),
        (0x15e2b8, "mv%02d"),
        (0x15c68f, "logo"),
        (0x15df82, "help_moviemode"),
        (0x15c0bf, "key"),
    ] {
        assert_eq!(&exe[a + 256..a + 256 + text.len()], text.as_bytes());
        assert_eq!(exe[a + 256 + text.len()], 0);
    }
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap()).unwrap();
    let file = rom.find_file("/moviemode.txa").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let atlas = read_texture_archive(&bytes).unwrap();
    for (name, size) in [
        ("bg", (1920, 1080)),
        ("logo", (384, 216)),
        ("close", (84, 84)),
        ("key", (384, 96)),
        ("help_moviemode", (320, 144)),
    ] {
        assert_eq!(atlas.textures[atlas.name_to_index[name]].dimensions(), size);
    }
    for id in 1..=10 {
        assert_eq!(
            atlas.textures[atlas.name_to_index[&format!("mv{id:02}")]].dimensions(),
            (384, 216)
        );
    }
}
