//! MusicMode completion label300bc..30210, SimpleText style1.
use super::music_gallery::MusicGalleryList;
use crate::{
    adv::DialogText,
    asset::{font::GpuFontLazy, system::AssetServer},
};
use glam::{Mat4, vec3};
use shin_core::primitives::color::FloatColor4;
use shin_render::{ColorBlendType, RenderRequestBuilder, render_pass::RenderPass};
use std::sync::Arc;

pub(super) struct MusicCompletionText {
    text: DialogText,
}
impl MusicCompletionText {
    pub fn new(server: &AssetServer, font: &Arc<GpuFontLazy>, list: &MusicGalleryList) -> Self {
        Self {
            text: DialogText::load_simple_text(server, font, &list.completion_label(), 40.0, 640.0),
        }
    }
    #[cfg(test)]
    pub(super) fn reference_text(&self) -> &DialogText {
        &self.text
    }
    pub fn render(&self, pass: &mut RenderPass) {
        self.text.render_with_builder(
            pass,
            crate::render::top_left_projection_matrix()
                * Mat4::from_translation(vec3(48.0, 1012.0, 0.0)),
            FloatColor4::WHITE,
            FloatColor4::from_rgba(47.0 / 255.0, 25.0 / 255.0, 20.0 / 255.0, 1.0),
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::format::{
        save::SaveData,
        scenario::{
            info::{BgmId, MusicBoxInfoItem},
            instruction_elements::FromNumber,
        },
    };
    #[test]
    fn completion_label_matches_native_decimal_floor_and_full_completion_literal() {
        let items: Vec<_> = (0..3)
            .map(|i| MusicBoxInfoItem {
                bgm_id: BgmId::from_number(i),
                name_index: i as u16 + 1,
                once_flag: 0,
            })
            .collect();
        let mut save = SaveData::default();
        assert_eq!(
            MusicGalleryList::new(&items, &save)
                .unwrap()
                .completion_label(),
            "達成率:0.0％"
        );
        save.save_vectors.unlocked_bgms.add(0);
        assert_eq!(
            MusicGalleryList::new(&items, &save)
                .unwrap()
                .completion_label(),
            "達成率:33.3％"
        );
        save.save_vectors.unlocked_bgms.add(1);
        assert_eq!(
            MusicGalleryList::new(&items, &save)
                .unwrap()
                .completion_label(),
            "達成率:66.6％"
        );
        save.save_vectors.unlocked_bgms.add(2);
        assert_eq!(
            MusicGalleryList::new(&items, &save)
                .unwrap()
                .completion_label(),
            "達成率:100％"
        );
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_music_completion_label_and_global_audio_dispatch() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, expected) in [
        (0x300cc, 0x1ada0d08),
        (0x300d0, 0x710fa11f),
        (0x300d8, 0x52860628),
        (0x300dc, 0x72a00608),
        (0x30164, 0xf2e84408),
        (0x30168, 0xb26003e9),
        (0x301c4, 0x97ffbaff),
        (0x30210, 0x940343e4),
        // Full-completion notification ignores achievement31: the whole
        // platform function only locks/unlocks a mutex and changes no save bit.
        (0x116930, 0xf81e0ff3),
        (0x116934, 0xa9017bfd),
        (0x116938, 0x910043fd),
        (0x11693c, 0x90000968),
        (0x116940, 0xf946cd13),
        (0x116944, 0xaa1303e0),
        (0x116948, 0x940018e6),
        (0x11694c, 0xa9417bfd),
        (0x116950, 0xaa1303e0),
        (0x116954, 0xf84207f3),
        (0x116958, 0x140018e6),
        (0x311c8, 0xbd458500),
        (0x311d8, 0x91072274),
        (0x311f8, 0x94033ff6),
        (0x100b00, 0x7100051f),
        (0x100b04, 0x1e201d22),
        (0x100b48, 0x1e283820),
        (0x33868, 0x54000760),
        (0x33880, 0x710012ff),
        (0x33954, 0x91004120),
        (0x3396c, 0x94022025),
        (0x339e8, 0x940000ba),
        (0x33d18, 0x2a1403e2),
        (0xf3b30, 0xb902c514),
        (0xf459c, 0xb942c669),
        (0xf45a8, 0x540001a2),
    ] {
        assert_eq!(word(a), expected, "native music completion/audio{a:x}");
    }
    for (a, value) in [
        (0x159a6c, 47.0f32 / 255.0),
        (0x1593e0, 25.0 / 255.0),
        (0x159478, 20.0 / 255.0),
        (0x159584, 48.0),
        (0x15965c, 1012.0),
    ] {
        assert_eq!(word(a), value.to_bits(), "native music text constant{a:x}");
    }
    for (a, text) in [(0x15d9ef, "達成率:"), (0x161e8b, "％"), (0x15eb8f, "%d.%d")] {
        assert_eq!(&exe[a + 256..a + 256 + text.len()], text.as_bytes());
        assert_eq!(exe[a + 256 + text.len()], 0);
    }
}
