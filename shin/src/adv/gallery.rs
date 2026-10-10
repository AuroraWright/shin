use shin_core::{format::scenario::info::ScenarioInfoTables, vm::command::types::LayerType};

use super::VmState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GalleryUnlock {
    Picture(i32),
    Music(i32),
    Movie(i32),
}

pub(super) fn played_bgm(info: &ScenarioInfoTables, id: i32) -> Vec<GalleryUnlock> {
    let Some(item) = usize::try_from(id)
        .ok()
        .and_then(|id| info.bgm_info.get(id))
    else {
        return Vec::new();
    };
    let mut result = vec![GalleryUnlock::Music(id)];
    if let Some(link) = item.linked_bgm_id.repr() {
        result.push(GalleryUnlock::Music(link.into_i32()));
    }
    result
}

pub(super) fn loaded_layer(
    info: &ScenarioInfoTables,
    kind: LayerType,
    id: i32,
) -> Vec<GalleryUnlock> {
    let Some(index) = usize::try_from(id).ok() else {
        return Vec::new();
    };
    match kind {
        LayerType::Picture => {
            let Some(item) = info.picture_info.get(index) else {
                return Vec::new();
            };
            let mut result = vec![GalleryUnlock::Picture(id)];
            if let Some(link) = item.linked_cg_id.repr() {
                result.push(GalleryUnlock::Picture(link.into_i32()));
            }
            result
        }
        LayerType::Movie => {
            let Some(item) = info.movie_info.get(index) else {
                return Vec::new();
            };
            let mut result = Vec::new();
            if let Some(link) = item.linked_bgm_id.repr() {
                let bgm = link.into_i32();
                result.extend(played_bgm(info, bgm));
                // Native records a valid link even if its info item is missing.
                result.push(GalleryUnlock::Music(bgm));
            }
            let picture = item.linked_picture_id.into_i32();
            if let Some(link) = info
                .picture_info
                .get(picture as usize)
                .and_then(|p| p.linked_cg_id.repr())
            {
                // Native 0x20560 uses global+0x80 (music), not the picture bitset.
                result.push(GalleryUnlock::Music(link.into_i32()));
            }
            result.push(GalleryUnlock::Picture(picture));
            result.push(GalleryUnlock::Movie(id));
            result
        }
        _ => Vec::new(),
    }
}

pub(super) fn apply(state: &mut VmState, unlocks: impl IntoIterator<Item = GalleryUnlock>) {
    if !state.persistent_commits_enabled {
        return;
    }
    for unlock in unlocks {
        let (set, id) = match unlock {
            GalleryUnlock::Picture(id) => (&mut state.save_vectors.unlocked_cgs, id),
            GalleryUnlock::Music(id) => (&mut state.save_vectors.unlocked_bgms, id),
            GalleryUnlock::Movie(id) => (&mut state.save_vectors.unlocked_movies, id),
        };
        if let Ok(id) = usize::try_from(id) {
            if set.add(id) {
                state.save_revision = state.save_revision.wrapping_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::format::{
        scenario::{info::*, instruction_elements::FromNumber},
        text::U16String,
    };

    fn info() -> ScenarioInfoTables {
        ScenarioInfoTables {
            mask_info: vec![],
            picture_info: vec![PictureInfoItem {
                name: U16String::new("picture"),
                linked_cg_id: PictureIdOpt::from_number(32),
            }],
            bustup_info: vec![],
            bgm_info: vec![BgmInfoItem {
                name: U16String::new("track"),
                display_name: U16String::new("Track"),
                linked_bgm_id: MusicBoxIdOpt::from_number(65),
            }],
            se_info: vec![],
            movie_info: vec![MovieInfoItem {
                name: U16String::new("movie"),
                linked_picture_id: PictureIdOpt::from_number(0),
                volume_source: MovieVolumeSource::Bgm,
                transparency: MovieTransparencyMode::Opaque,
                linked_bgm_id: BgmIdOpt::from_number(0),
            }],
            voice_mapping_info: vec![],
            picture_box_info: vec![],
            music_box_info: vec![],
            character_box_info: vec![],
            chars_sprite_info: vec![],
            chars_grid_info: vec![],
            tips_info: vec![],
        }
    }

    #[test]
    fn linked_unlocks_are_idempotent_and_reconstruction_does_not_commit() {
        let info = info();
        let mut state = VmState::new();
        state.persistent_commits_enabled = false;
        apply(&mut state, loaded_layer(&info, LayerType::Picture, 0));
        assert_eq!(state.save_revision, 0);
        assert!(!state.save_vectors.unlocked_cgs.contains(0));
        state.persistent_commits_enabled = true;
        apply(&mut state, loaded_layer(&info, LayerType::Picture, 0));
        apply(&mut state, played_bgm(&info, 0));
        assert_eq!(state.save_revision, 4);
        apply(&mut state, loaded_layer(&info, LayerType::Picture, 0));
        apply(&mut state, played_bgm(&info, 0));
        assert_eq!(state.save_revision, 4);
        assert!(state.save_vectors.unlocked_cgs.contains(32));
        assert!(state.save_vectors.unlocked_bgms.contains(65));
        assert!(!state.save_vectors.unlocked_cgs.contains(65));
        for id in [-1, 1000] {
            assert!(loaded_layer(&info, LayerType::Picture, id).is_empty());
            assert!(loaded_layer(&info, LayerType::Movie, id).is_empty());
            assert!(played_bgm(&info, id).is_empty());
        }
        assert!(loaded_layer(&info, LayerType::Tile, 0).is_empty());
    }

    #[test]
    fn movie_records_native_cross_bitset_link_and_missing_metadata_links() {
        let mut info = info();
        let mut state = VmState::new();
        apply(&mut state, loaded_layer(&info, LayerType::Movie, 0));
        assert_eq!(state.save_revision, 5);
        assert!(state.save_vectors.unlocked_movies.contains(0));
        assert!(state.save_vectors.unlocked_cgs.contains(0));
        assert!(!state.save_vectors.unlocked_cgs.contains(32));
        assert!(state.save_vectors.unlocked_bgms.contains(32));
        assert!(state.save_vectors.unlocked_bgms.contains(65));
        info.movie_info[0].linked_bgm_id = BgmIdOpt::from_number(80);
        info.movie_info[0].linked_picture_id = PictureIdOpt::from_number(90);
        apply(&mut state, loaded_layer(&info, LayerType::Movie, 0));
        assert!(state.save_vectors.unlocked_cgs.contains(90));
        assert!(state.save_vectors.unlocked_bgms.contains(80));
        assert_eq!(state.save_revision, 7);
    }

    #[test]
    #[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); verifies gallery metadata"]
    fn reference_gallery_links_are_applied_from_supplied_scenario() {
        use shin_core::{
            format::{rom::RomReader, scenario::Scenario},
            primitives::stateless_reader::StatelessFile,
        };
        use std::{fs::File, io::Read};
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        // NSO text file offset is virtual address + 0x100. Keep the unusual
        // movie-linked CG destination grounded in the supplied binary.
        for (address, instruction) in [
            (0x20560, 0x91020100u32),
            (0x20574, 0x9101a100),
            (0x20588, 0x91026100),
        ] {
            let actual = u32::from_le_bytes(
                executable[address + 0x100..address + 0x104]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(actual, instruction);
        }
        let mut rom =
            RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
        let file = rom.find_file("/main.snr").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let info = scenario.info_tables();
        let mut state = VmState::new();
        let mut pictures = 0;
        let mut tracks = 0;
        for (id, item) in info.picture_info.iter().enumerate() {
            if let Some(link) = item.linked_cg_id.repr() {
                apply(
                    &mut state,
                    loaded_layer(info, LayerType::Picture, id as i32),
                );
                assert!(
                    state
                        .save_vectors
                        .unlocked_cgs
                        .contains(link.into_i32() as usize)
                );
                pictures += 1;
            }
        }
        for (id, item) in info.bgm_info.iter().enumerate() {
            if let Some(link) = item.linked_bgm_id.repr() {
                apply(&mut state, played_bgm(info, id as i32));
                assert!(
                    state
                        .save_vectors
                        .unlocked_bgms
                        .contains(link.into_i32() as usize)
                );
                tracks += 1;
            }
        }
        assert!(pictures > 0 && tracks > 0);
        for id in 0..info.movie_info.len() {
            apply(&mut state, loaded_layer(info, LayerType::Movie, id as i32));
            assert!(state.save_vectors.unlocked_movies.contains(id));
        }
    }
}
