//! Global save/clock ownership retained while the app replaces a story scene.
use super::{VmState, persistence::SaveStore, play_clock::PlayClock};
use shin_core::{format::save::SaveData, time::Ticks};

pub(crate) struct GlobalData {
    pub(super) data: SaveData,
    pub(super) persistence: Option<SaveStore>,
    pub(super) clock: PlayClock,
    pub(super) revision: u64,
}
impl Default for GlobalData {
    fn default() -> Self {
        Self {
            data: SaveData::default(),
            persistence: None,
            clock: PlayClock::default(),
            revision: 0,
        }
    }
}
impl GlobalData {
    pub fn is_save_read_error(error: &anyhow::Error) -> bool {
        error.is::<super::persistence::SaveReadError>()
    }
    pub fn is_save_recovery_backup_error(error: &anyhow::Error) -> bool {
        error.is::<super::persistence::SaveRecoveryBackupError>()
    }
    pub fn save_recovery(error: &anyhow::Error) -> Option<super::SaveRecovery> {
        error
            .downcast_ref::<super::persistence::SaveDecodeError>()
            .map(|error| error.recovery.clone())
    }
    pub fn recover_save(recovery: super::SaveRecovery) -> anyhow::Result<Self> {
        let persistence = SaveStore::recover(recovery)?;
        let data = persistence.data.clone();
        Ok(Self {
            clock: PlayClock::from_saved_seconds(data.play_seconds),
            data,
            persistence: Some(persistence),
            revision: 0,
        })
    }
    #[cfg(test)]
    pub(crate) fn revision_for_reference(&self) -> u64 {
        self.revision
    }
    ///33800 records the accepted BGM and its linked gallery ID in global+80.
    /// Matching active music requests only retarget volume and do not record it.
    pub fn unlock_bgm(
        &mut self,
        info: &shin_core::format::scenario::info::ScenarioInfoTables,
        id: i32,
    ) {
        let mut changed = false;
        for unlock in super::gallery::played_bgm(info, id) {
            if let super::gallery::GalleryUnlock::Music(id) = unlock {
                if let Ok(index) = usize::try_from(id) {
                    changed |= self.data.save_vectors.unlocked_bgms.add(index);
                }
            }
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub fn open(
        path: Option<std::path::PathBuf>,
        imported: Option<SaveData>,
    ) -> anyhow::Result<Self> {
        let persistence = path
            .map(|path| SaveStore::open(path, imported.clone()))
            .transpose()?;
        let data = persistence
            .as_ref()
            .map(|store| store.data.clone())
            .or(imported)
            .unwrap_or_default();
        Ok(Self {
            clock: PlayClock::from_saved_seconds(data.play_seconds),
            data,
            persistence,
            revision: 0,
        })
    }

    /// TitleDemo114698 records these unlocks before its movie worker finishes.
    pub fn unlock_startup_movie(
        &mut self,
        info: &shin_core::format::scenario::info::MovieInfoItem,
    ) {
        let vectors = &mut self.data.save_vectors;
        let mut changed = vectors.unlocked_movies.add(0);
        if let Some(picture) = info.linked_picture_id.repr() {
            changed |= vectors.unlocked_cgs.add(picture.into_i32() as usize);
        }
        if let Some(bgm) = info.linked_bgm_id.repr() {
            changed |= vectors.unlocked_bgms.add(bgm.into_i32() as usize);
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub fn apply_config_settings(&mut self, settings: &shin_core::format::save::Settings) {
        self.data.settings = settings.clone();
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn prepare_config_save(&mut self) -> anyhow::Result<()> {
        let state = self.state();
        if let Some(store) = &mut self.persistence {
            store.data = self.data.clone();
            store.prepare_explicit(&state, super::persistence::ExplicitSave::Globals)?;
        }
        Ok(())
    }
    pub fn update_save_task(
        &mut self,
        task: &mut super::save_game::SaveGameTask,
        delta: Ticks,
    ) -> Option<anyhow::Result<()>> {
        task.update(delta, self.persistence.as_mut())
    }
    pub fn remember_load_slot(&mut self, slot: u8) {
        // db1a0 remembers Load selection in memory without a SaveGame boundary.
        self.data.save_menu_position = slot;
        if let Some(store) = &mut self.persistence {
            store.data.save_menu_position = slot;
        }
    }
    pub fn save(&self) -> &SaveData {
        &self.data
    }
    /// Updated e3050 changes only zero chapter flags, then queues a new picker.
    /// Its callback changes global memory without an immediate SaveGame call.
    pub fn unlock_main_scenarios(&mut self) {
        self.data.persist_data.set(0, 24);
        for slot in 1..=25 {
            if self.data.persist_data.get(slot) == 0 {
                self.data.persist_data.set(slot, 1);
            }
        }
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn play_ticks(&self) -> f64 {
        self.clock.ticks()
    }
    /// The app advances this on title frames, outside scene/input/loading gates.
    pub fn advance_play_clock(&mut self, delta: Ticks) {
        self.clock.advance(delta.as_f32());
        self.data.play_seconds = self.clock.saved_seconds();
    }
    fn state(&self) -> VmState {
        let mut state = VmState::new();
        state.settings = self.data.settings.clone();
        state.persist = self.data.persist_data.clone();
        state.save_vectors = self.data.save_vectors.clone();
        state.auto_save_slot = self.data.auto_save_slot.clone();
        state.save_revision = self.revision;
        state
    }
    pub(crate) fn flush_shutdown(&mut self) -> anyhow::Result<()> {
        let state = self.state();
        if let Some(store) = &mut self.persistence {
            store.data = self.data.clone();
            store.flush_final(&state)?;
        }
        Ok(())
    }
}
impl Drop for GlobalData {
    fn drop(&mut self) {
        if let Err(error) = self.flush_shutdown() {
            tracing::warn!(%error, "Saving global progress on shutdown failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_load_preserves_globals_and_records_movie_links_once() {
        use shin_core::format::{
            scenario::{
                info::{
                    BgmIdOpt, MovieInfoItem, MovieTransparencyMode, MovieVolumeSource, PictureIdOpt,
                },
                instruction_elements::FromNumber,
            },
            text::U16String,
        };
        let mut saved = SaveData::default();
        saved.play_seconds = 93;
        saved.settings.v0_bgmvol = 37;
        saved.persist_data.set(24, 3);
        let mut globals = GlobalData::open(None, Some(saved.clone())).unwrap();
        assert_eq!(globals.save().encode(), saved.encode());
        assert_eq!(globals.play_ticks(), 93.0 * 60.0);
        let mut info = MovieInfoItem {
            name: U16String::new("opening"),
            linked_picture_id: PictureIdOpt::from_number(7),
            volume_source: MovieVolumeSource::Ignore,
            transparency: MovieTransparencyMode::Opaque,
            linked_bgm_id: BgmIdOpt::from_number(215),
        };
        globals.unlock_startup_movie(&info);
        saved.save_vectors.unlocked_movies.add(0);
        saved.save_vectors.unlocked_cgs.add(7);
        saved.save_vectors.unlocked_bgms.add(215);
        assert_eq!(globals.save().encode(), saved.encode());
        assert_eq!(globals.revision, 1);
        globals.unlock_startup_movie(&info);
        assert_eq!(globals.revision, 1);
        info.linked_bgm_id = BgmIdOpt::from_number(-1);
        globals.unlock_startup_movie(&info);
        assert_eq!(globals.revision, 1);
        assert_eq!(globals.save().encode(), saved.encode());
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_saved_factory_mode_and_deferred_scene_replacement() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x16434, 0xb9006bf4),
            (0x16468, 0xb9006a88),
            (0x19518, 0xb9406a82),
            (0x19524, 0x97ffd983),
            (0xfd30, 0xb9002b78),
            (0x273d8, 0x940000aa),
            (0x26f30, 0xf9449a80),
            (0x26f40, 0xf9046a7f),
            (0x26f54, 0xd63f0120),
            (0x26f68, 0xd63f0100),
            (0x26f74, 0xf9046a68),
            (0x26f94, 0xf900111f),
            (0x26fc8, 0xd63f0100),
            (0x26fdc, 0x940033b9),
            (0x27004, 0x1e22c000),
            (0x27010, 0xfd000100),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        let callback = (0x11e040usize..0x152eb8)
            .step_by(24)
            .find(|&a| u64::from_le_bytes(exe[a + 256..a + 264].try_into().unwrap()) == 0x1d23f8)
            .unwrap();
        assert_eq!(
            u64::from_le_bytes(exe[callback + 272..callback + 280].try_into().unwrap()),
            0x19500
        );
    }
    #[test]
    fn title_frames_preserve_fractional_clock_and_shutdown_writes_global_save() {
        let dir = std::env::temp_dir().join(format!(
            "shin-title-global-clock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("progress.dat");
        let mut data = SaveData::default();
        data.play_seconds = 123;
        data.settings.v3_sysvol = 65;
        data.persist_data.set(0, 31);
        let mut globals = GlobalData {
            persistence: Some(SaveStore::open(path.clone(), Some(data.clone())).unwrap()),
            clock: PlayClock::from_saved_seconds(123),
            data,
            revision: 0,
        };
        globals.advance_play_clock(Ticks::from_f32(59.5));
        assert_eq!(globals.save().play_seconds, 123);
        assert_eq!(globals.play_ticks(), 123.0 * 60.0 + 59.5);
        globals.advance_play_clock(Ticks::from_f32(0.5));
        drop(globals);
        let saved = SaveData::decode(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved.play_seconds, 124);
        assert_eq!(saved.settings.v3_sysvol, 65);
        assert_eq!(saved.persist_data.get(0), 31);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
#[test]
#[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM)"]
fn reference_native_global_bgm_unlock_links_and_revision() {
    use shin_core::{
        format::{rom::RomReader, scenario::Scenario},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap()).unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Scenario::new(bytes.into()).unwrap();
    let info = scenario.info_tables();
    let (id, item) = info
        .bgm_info
        .iter()
        .enumerate()
        .find(|(_, item)| item.linked_bgm_id.repr().is_some())
        .unwrap();
    let link = item.linked_bgm_id.repr().unwrap().into_i32() as usize;
    let mut globals = GlobalData::default();
    globals.unlock_bgm(info, id as i32);
    assert!(globals.save().save_vectors.unlocked_bgms.contains(id));
    assert!(globals.save().save_vectors.unlocked_bgms.contains(link));
    assert_eq!(globals.revision, 1);
    let original = globals.save().encode();
    globals.unlock_bgm(info, id as i32);
    globals.unlock_bgm(info, -1);
    assert_eq!(globals.revision, 1);
    assert_eq!(globals.save().encode(), original);
    println!(
        "Global original BGM{id}: linked unlock{link}, idempotent revision and invalid ID pass"
    );
}
