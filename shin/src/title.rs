//! Native title scene10a870 saved-progression snapshot and authored menu setup.
//! Owns setup, animation, input, assets and drawing for the app scene host.
use shin_core::format::save::SaveData;
#[allow(dead_code)]
mod bustup_catalog;
#[allow(dead_code)]
mod bustup_input;
#[allow(dead_code)]
mod bustup_menu;
#[cfg(test)]
mod bustup_render_reference;
#[allow(dead_code)]
mod bustup_resources;
mod bustup_scene;
#[allow(dead_code)]
mod bustup_view;
#[allow(dead_code)]
mod bustup_viewer;
pub(crate) use bustup_scene::BustupScene;
mod captured_scene;
#[allow(dead_code)]
mod cg_gallery;
#[allow(dead_code)]
mod cg_gallery_input;
#[cfg(test)]
mod cg_gallery_render_reference;
mod cg_gallery_scene;
#[allow(dead_code)]
mod cg_gallery_text;
#[cfg(test)]
mod gallery_completion_reference;
pub(crate) use cg_gallery_scene::CgGalleryScene;
#[allow(dead_code)]
mod cg_gallery_view;
#[allow(dead_code)]
mod cg_viewer;
#[allow(dead_code)]
mod cg_viewer_input;
#[allow(dead_code)]
mod cg_viewer_motion;
#[allow(dead_code)]
mod cg_viewer_scene;
#[allow(dead_code)]
mod cg_viewer_view;
mod child;
mod episode_chooser;
mod episode_chooser_scene;
mod episode_chooser_view;
mod episode_transition;
mod episode_transition_backdrop;
mod episode_transition_scene;
pub(crate) use episode_transition_scene::EpisodeScene;
mod episode_transition_view;
mod input;
mod motion;
// Native MovieMode retains its gallery beneath independently owned movie children.
#[allow(dead_code)]
mod movie_gallery;
#[allow(dead_code)]
mod movie_gallery_input;
mod movie_gallery_scene;
mod movie_gallery_text;
#[allow(dead_code)]
mod movie_gallery_view;
#[allow(dead_code)]
mod movie_playback;
#[allow(dead_code)]
mod movie_playback_scene;
pub(crate) use movie_gallery_scene::MovieGalleryScene;
// Native MusicMode replaces Title and retains global audio/save ownership.
#[allow(dead_code)]
mod music_gallery;
#[allow(dead_code)]
mod music_gallery_audio;
#[allow(dead_code)]
mod music_gallery_input;
mod music_gallery_scene;
#[allow(dead_code)]
mod music_gallery_text;
#[allow(dead_code)]
mod music_gallery_view;
pub(crate) use music_gallery_scene::MusicGalleryScene;
#[cfg(test)]
mod pending_save_reference;
#[cfg(test)]
mod render_reference;
#[cfg(test)]
pub(crate) use pending_save_reference::reference_pending_config_save;
mod scenario_select;
mod scenario_select_details;
#[cfg(test)]
mod scenario_select_frame_reference;
mod scenario_select_input;
mod scenario_select_opening;
#[cfg(test)]
mod scenario_select_render_reference;
mod scenario_select_scene;
mod scenario_select_selection;
pub(crate) use scenario_select_scene::{ScenarioDestination, ScenarioSelectScene};
#[cfg(test)]
mod scenario_select_tab_reference;
mod scenario_select_view;
mod scene;
mod title_demo;
mod title_demo_scene;
#[cfg(test)]
pub(crate) use motion::Phase as TitlePhase;
pub(crate) use scene::TitleScene;
pub(crate) use title_demo_scene::TitleDemoScene;
mod view;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum TitleItem {
    Start = 0,
    Continue = 1,
    Load = 2,
    Characters = 3,
    Tips = 4,
    Config = 5,
    Cg = 6,
    Bgm = 7,
    Movie = 8,
    Manual = 9,
    Bustup = 10,
}
impl TitleItem {
    pub fn caption(self) -> &'static str {
        match self {
            Self::Start => "text_start",
            Self::Continue => "text_continue",
            Self::Load => "text_load",
            Self::Characters => "text_chars",
            Self::Tips => "text_tips",
            Self::Config => "text_config",
            Self::Cg => "text_cg",
            Self::Bgm => "text_bgm",
            Self::Movie => "text_movie",
            Self::Manual => "text_manual",
            Self::Bustup => "text_bup",
        }
    }
}

/// Exact five-byte records at186646, addressed through native226448.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TitleStage {
    pub first: u8,
    pub last: u8,
    pub background: u8,
    pub bgm: u8,
    pub bgm_volume: u8,
}
pub(super) const STAGES: [TitleStage; 14] = [
    stage(0, 2, 1, 0, 75),
    stage(3, 5, 2, 0, 75),
    stage(6, 8, 3, 0, 75),
    stage(9, 11, 4, 0, 75),
    stage(12, 14, 6, 0, 75),
    stage(15, 17, 7, 1, 75),
    stage(18, 18, 8, 0, 75),
    stage(19, 20, 9, 0, 75),
    stage(21, 21, 10, 0, 75),
    stage(22, 23, 11, 0, 75),
    stage(24, 26, 13, 2, 100),
    stage(27, 28, 14, 3, 100),
    stage(29, 30, 15, 4, 100),
    stage(31, 255, 16, 8, 100),
];
const fn stage(first: u8, last: u8, background: u8, bgm: u8, bgm_volume: u8) -> TitleStage {
    TitleStage {
        first,
        last,
        background,
        bgm,
        bgm_volume,
    }
}

#[derive(Debug, Clone)]
pub(super) struct TitleSetup {
    pub progress: u8,
    pub stage_index: usize,
    pub completed_episodes: u16,
    pub items: Vec<TitleItem>,
    pub selected: usize,
    /// Native10aa60..10aa74 stores1 for nonnegative factory arguments.
    pub returning_from_child: bool,
    pub first_row_y: f32,
    pub logo_x: f32,
}
impl TitleSetup {
    pub fn from_save(save: &SaveData, selected_item: i32) -> Self {
        // The persisted accessor returns signed16bit; constructor clamps0..255.
        let progress = save.persist_data.get(0).clamp(0, 255) as u8;
        let stage_index = STAGES
            .iter()
            .position(|stage| progress <= stage.last)
            .unwrap();
        let mut completed_episodes = 0u16;
        for (bit, index) in (1..=8).chain(17..=24).enumerate() {
            if save.persist_data.get(index) >= 3 {
                completed_episodes |= 1 << bit;
            }
        }
        let mut items = vec![TitleItem::Start];
        if save.auto_save_slot.is_some() {
            items.push(TitleItem::Continue);
        }
        items.extend([TitleItem::Load, TitleItem::Config]);
        if completed_episodes & 0xff00 != 0 {
            items.push(TitleItem::Characters);
        }
        if completed_episodes & 0x7f00 != 0 {
            items.push(TitleItem::Tips);
        }
        if progress >= 1 {
            items.extend([TitleItem::Cg, TitleItem::Bgm, TitleItem::Movie]);
        }
        if progress >= 31 {
            items.push(TitleItem::Bustup);
        }
        items.push(TitleItem::Manual);
        let preferred = if selected_item < 0 {
            TitleItem::Continue as i32
        } else {
            selected_item
        };
        let selected = items
            .iter()
            .position(|item| *item as i32 == preferred)
            .unwrap_or(0);
        // Native10b090 inverse-stride multiply yields count*82, not count.
        let overflow = (450.0 + items.len() as f32 * 82.0 - 1032.0).max(0.0);
        Self {
            progress,
            stage_index,
            completed_episodes,
            items,
            selected,
            returning_from_child: selected_item >= 0,
            first_row_y: 450.0 - overflow,
            logo_x: if overflow > 0.0 { 96.0 } else { 840.0 },
        }
    }
    pub fn stage(&self) -> &TitleStage {
        &STAGES[self.stage_index]
    }
    pub fn background_path(&self) -> String {
        format!("/title/bg{:02}.pic", self.stage().background)
    }
    pub fn row_origin(&self, index: usize) -> Option<glam::Vec2> {
        (index < self.items.len())
            .then(|| glam::vec2(1184.0, self.first_row_y + index as f32 * 82.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_uses_signed_persistence_and_all_fourteen_stages() {
        let mut save = SaveData::default();
        for value in i16::MIN..=i16::MAX {
            save.persist_data.set(0, value as i32);
            let setup = TitleSetup::from_save(&save, -1);
            assert_eq!(setup.progress, (value as i32).clamp(0, 255) as u8);
            assert!((setup.stage().first..=setup.stage().last).contains(&setup.progress));
            assert_eq!(
                setup.stage_index,
                STAGES
                    .iter()
                    .position(|stage| setup.progress <= stage.last)
                    .unwrap()
            );
        }
        assert_eq!(
            STAGES
                .iter()
                .map(|stage| stage.background)
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 13, 14, 15, 16]
        );
    }
    #[test]
    fn row_unlocks_preserve_native_order_and_distinct_episode_masks() {
        let mut save = SaveData::default();
        assert_eq!(
            TitleSetup::from_save(&save, -1).items,
            [
                TitleItem::Start,
                TitleItem::Load,
                TitleItem::Config,
                TitleItem::Manual
            ]
        );
        for index in (1..=8).chain(17..=24) {
            for value in [-1, 0, 2, 3, 4] {
                save.persist_data = Default::default();
                save.persist_data.set(index, value);
                let setup = TitleSetup::from_save(&save, -1);
                assert_eq!(
                    setup.items.contains(&TitleItem::Characters),
                    index >= 17 && value >= 3
                );
                assert_eq!(
                    setup.items.contains(&TitleItem::Tips),
                    (17..=23).contains(&index) && value >= 3
                );
                assert_eq!(setup.completed_episodes.count_ones(), u32::from(value >= 3));
            }
        }
        save.persist_data.set(0, 31);
        save.persist_data.set(17, 3);
        assert_eq!(
            TitleSetup::from_save(&save, -1).items,
            [
                TitleItem::Start,
                TitleItem::Load,
                TitleItem::Config,
                TitleItem::Characters,
                TitleItem::Tips,
                TitleItem::Cg,
                TitleItem::Bgm,
                TitleItem::Movie,
                TitleItem::Bustup,
                TitleItem::Manual
            ]
        );
    }
    #[test]
    fn startup_prefers_autosave_and_child_returns_select_by_item_not_row() {
        let mut save = SaveData::default();
        assert_eq!(TitleSetup::from_save(&save, -1).selected, 0);
        save.manual_save_slots[0] = Some(shin_core::format::save::GameData::from_entry(
            shin_core::format::save::GameDataEntry {
                scenario_id: 0,
                random_seed: 42,
                save_position: 1,
                route_data: Default::default(),
            },
        ));
        assert_eq!(
            TitleSetup::from_save(&save, -1).selected,
            0,
            "manual slot cannot enable Continue"
        );
        save.auto_save_slot = save.manual_save_slots[0].clone();
        let setup = TitleSetup::from_save(&save, -1);
        assert_eq!(setup.items[setup.selected], TitleItem::Continue);
        assert!(!setup.returning_from_child);
        for item in [TitleItem::Load, TitleItem::Config, TitleItem::Manual] {
            let setup = TitleSetup::from_save(&save, item as i32);
            assert_eq!(setup.items[setup.selected], item);
            assert!(setup.returning_from_child);
        }
        assert_eq!(
            TitleSetup::from_save(&save, TitleItem::Bustup as i32).selected,
            0
        );
        assert_eq!(TitleSetup::from_save(&save, i32::MAX).selected, 0);
    }
    #[test]
    fn crowded_rows_move_up_and_move_logo_to_left() {
        let mut save = SaveData::default();
        let small = TitleSetup::from_save(&save, -1);
        assert_eq!((small.first_row_y, small.logo_x), (450.0, 840.0));
        assert_eq!(small.row_origin(3), Some(glam::vec2(1184.0, 696.0)));
        assert_eq!(small.row_origin(4), None);
        save.persist_data.set(0, 1);
        let seven = TitleSetup::from_save(&save, -1);
        assert_eq!(
            (seven.items.len(), seven.first_row_y, seven.logo_x),
            (7, 450.0, 840.0)
        );
        save.persist_data.set(17, 3);
        let nine = TitleSetup::from_save(&save, -1);
        assert_eq!(
            (nine.items.len(), nine.first_row_y, nine.logo_x),
            (9, 294.0, 96.0)
        );
        assert_eq!(nine.row_origin(8), Some(glam::vec2(1184.0, 950.0)));
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_title_progression_records_captions_and_constructor_gates() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        // RELA pointers are authoritative; table locations are recorded from
        // native226448 and title-caption vtable221e20.
        let mut relocations = std::collections::HashMap::new();
        for offset in (0x11e040..0x152eb8).step_by(24) {
            let address = u64::from_le_bytes(exe[offset + 256..offset + 264].try_into().unwrap());
            let value = u64::from_le_bytes(exe[offset + 272..offset + 280].try_into().unwrap());
            relocations.insert(address, value);
        }
        let records = relocations[&0x226448] as usize;
        assert_eq!(records, 0x186646);
        for (index, stage) in STAGES.iter().enumerate() {
            assert_eq!(
                &exe[records + 256 + index * 5..records + 256 + (index + 1) * 5],
                &[
                    stage.first,
                    stage.last,
                    stage.background,
                    stage.bgm,
                    stage.bgm_volume
                ]
            );
        }
        let mut save = SaveData::default();
        for progress in 0..=255 {
            save.persist_data.set(0, progress);
            let setup = TitleSetup::from_save(&save, -1);
            let record = exe[records + 256..records + 326]
                .chunks_exact(5)
                .find(|record| (record[0] as i32..=record[1] as i32).contains(&progress))
                .unwrap();
            assert_eq!(setup.stage().background, record[2]);
            assert_eq!(setup.stage().bgm, record[3]);
            assert_eq!(setup.stage().bgm_volume, record[4]);
        }
        for item in [
            TitleItem::Start,
            TitleItem::Continue,
            TitleItem::Load,
            TitleItem::Characters,
            TitleItem::Tips,
            TitleItem::Config,
            TitleItem::Cg,
            TitleItem::Bgm,
            TitleItem::Movie,
            TitleItem::Manual,
            TitleItem::Bustup,
        ] {
            let address = relocations[&(0x221e20 + item as u64 * 8)] as usize + 256;
            let end = exe[address..].iter().position(|byte| *byte == 0).unwrap() + address;
            assert_eq!(item.caption().as_bytes(), &exe[address..end]);
        }
        for (address, expected) in [
            (0x10aa88, 0x97ff1a52),
            (0x10aa8c, 0x13003c08),
            (0x10aa98, 0x1a89b108),
            (0x10aa9c, 0x0aa87d08),
            (0x10ad9c, 0x39432388),
            (0x10ae74, 0x72181d1f),
            (0x10aec4, 0x7218191f),
            (0x10af14, 0x7100051f),
            (0x10afec, 0x71007d1f),
            (0x10b038, 0x52800121),
            (0x10b098, 0x52869f4b),
            (0x10b0d8, 0xb9023aca),
            (0x10d628, 0xb940052a),
            (0x10d664, 0xeb09011f),
            (0x10d754, 0x1a8903e8),
            (0x113540, 0x39400902),
            (0x10d788, 0x3d401100),
            (0x10d790, 0x39400d01),
            (0x10d7b8, 0x321e0fe3),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, value) in [
            (0x159378, 450.0f32),
            (0x159644, 1032.0),
            (0x1598f0, -1032.0),
            (0x159690, 82.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                value
            );
        }
        for (address, value) in [
            (0x15b563, "/title.txa"),
            (0x15e59e, "/default.msk"),
            (0x15b56e, "/title/bg%02d.pic"),
        ] {
            assert_eq!(
                &exe[address + 256..address + 256 + value.len()],
                value.as_bytes()
            );
        }
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); decodes all title artwork"]
    fn reference_rom_title_captions_and_all_fourteen_progression_backgrounds_decode() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut read = |path: &str| {
            let file = rom.find_file(path).unwrap();
            let mut bytes = Vec::new();
            rom.open_file(file)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            bytes
        };
        let atlas =
            shin_core::format::texture_archive::read_texture_archive(&read("/title.txa")).unwrap();
        let mut save = SaveData::default();
        save.persist_data.set(0, 31);
        save.persist_data.set(17, 3);
        save.auto_save_slot = Some(shin_core::format::save::GameData::from_entry(
            shin_core::format::save::GameDataEntry {
                scenario_id: 0,
                random_seed: 42,
                save_position: 1,
                route_data: Default::default(),
            },
        ));
        let setup = TitleSetup::from_save(&save, -1);
        assert_eq!(setup.items.len(), 11);
        for item in setup.items {
            assert!(
                atlas.name_to_index.contains_key(item.caption()),
                "missing {}",
                item.caption()
            );
        }
        let mask = shin_core::format::mask::read_mask(&read("/default.msk")).unwrap();
        drop(mask);
        for stage in STAGES {
            save.persist_data.set(0, stage.first as i32);
            let setup = TitleSetup::from_save(&save, -1);
            let path = setup.background_path();
            let picture = shin_core::format::picture::read_picture::<
                shin_core::format::picture::SimplePicture,
            >(&read(&path), ())
            .unwrap();
            assert!(!picture.blocks.is_empty(), "{path}");
            assert!(picture.effective_width > 0 && picture.effective_height > 0);
            println!(
                "Title stage{}: {} decoded{}x{}, BGM{} at{}%",
                setup.stage_index,
                path,
                picture.effective_width,
                picture.effective_height,
                stage.bgm,
                stage.bgm_volume
            );
        }
    }
}

#[cfg(test)]
mod scenario_unlock_reference;

#[cfg(test)]
mod unlocked_save_reference;
