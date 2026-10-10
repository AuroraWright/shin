//! Native SaveMenu's serial route replay and visible-row scene ownership.
use std::sync::Arc;

use anyhow::{Context, Result};
use shin_core::{
    format::{
        save::{GameDataEntry, SaveData},
        scenario::Scenario,
    },
    vm::command::RuntimeCommand,
};

use super::{
    reconstruction::ReconstructedScene,
    save_menu::{SLOT_COUNT, SaveMenuModel},
    save_menu_text::SaveSlotLabels,
};

pub(super) struct SavedSlotScene {
    pub scene: ReconstructedScene,
    pub labels: SaveSlotLabels,
}
impl SavedSlotScene {
    pub fn reconstruct(
        scenario: &Scenario,
        entry: &GameDataEntry,
        globals: &SaveData,
        command_limit: usize,
    ) -> Result<Self> {
        // Native replay SAVEINFO1049b0 writes current pointers60a0/60b0.
        // AUTOSAVE1049d0 snapshots them to60a8/60b8;102684 copies those
        // snapshots to SaveMenu scene5a38/5a40. Later SAVEINFO is irrelevant.
        let mut title = String::new();
        let mut subtitle = String::new();
        let scene = ReconstructedScene::from_save_with_observer(
            scenario,
            entry,
            globals,
            command_limit,
            |command, state, _| {
                if matches!(command, RuntimeCommand::AUTOSAVE(_)) {
                    title.clone_from(&state.save_info.info[0]);
                    subtitle.clone_from(&state.save_info.info[1]);
                }
            },
        )?;
        Ok(Self {
            scene,
            labels: SaveSlotLabels::Ready { title, subtitle },
        })
    }
}

struct ReplayJob {
    slot: usize,
    generation: u64,
    task: shin_tasks::ComputeTask<Result<Arc<SavedSlotScene>>>,
}

/// Native d6994 scans rows in ascending order against the full canvas. It owns
/// one replay job, releases offscreen scenes, and publishes completion after
/// the scan. A write invalidates the old slot even if its replay is still busy.
pub(super) struct SaveMenuSceneCache {
    scenario: Arc<Scenario>,
    globals: Arc<SaveData>,
    scenes: [Option<Arc<SavedSlotScene>>; SLOT_COUNT],
    generations: [u64; SLOT_COUNT],
    replay: Option<ReplayJob>,
}
impl SaveMenuSceneCache {
    pub fn new(scenario: Arc<Scenario>, globals: SaveData) -> Self {
        Self {
            scenario,
            globals: Arc::new(globals),
            scenes: std::array::from_fn(|_| None),
            generations: [0; SLOT_COUNT],
            replay: None,
        }
    }
    pub fn scene(&self, slot: usize) -> Option<&Arc<SavedSlotScene>> {
        self.scenes[slot].as_ref()
    }
    pub fn pending_slot(&self) -> Option<usize> {
        self.replay.as_ref().map(|job| job.slot)
    }
    pub fn invalidate(&mut self, slot: usize) {
        self.generations[slot] = self.generations[slot].wrapping_add(1);
        self.scenes[slot] = None;
    }
    pub fn labels(&self, model: &SaveMenuModel, slot: usize) -> SaveSlotLabels {
        if model.slots()[slot].is_none() {
            SaveSlotLabels::Empty
        } else if let Some(scene) = self.scene(slot) {
            scene.labels.clone()
        } else {
            SaveSlotLabels::Loading
        }
    }
    /// Returns rows whose reconstructed scene became ready this frame. GPU
    /// preview work is a separate serial stage; text can become ready first.
    pub fn update(&mut self, model: &SaveMenuModel) -> Result<Vec<usize>> {
        let mut visible = [false; SLOT_COUNT];
        for slot in model.visible_slots() {
            visible[slot] = true;
        }
        for (slot, shown) in visible.into_iter().enumerate() {
            if !shown {
                self.scenes[slot] = None;
            } else if self.replay.is_none() && self.scenes[slot].is_none() {
                if let Some(data) = &model.slots()[slot] {
                    let entry = data.entry.clone();
                    let scenario = self.scenario.clone();
                    let globals = self.globals.clone();
                    self.replay = Some(ReplayJob {
                        slot,
                        generation: self.generations[slot],
                        task: shin_tasks::compute::spawn(move || {
                            SavedSlotScene::reconstruct(&scenario, &entry, &globals, 1_000_000)
                                .map(Arc::new)
                        }),
                    });
                }
            }
        }
        let Some(job) = &mut self.replay else {
            return Ok(Vec::new());
        };
        let Some(result) = job.task.poll_naive() else {
            return Ok(Vec::new());
        };
        let job = self.replay.take().unwrap();
        if job.generation != self.generations[job.slot] || model.slots()[job.slot].is_none() {
            // An overwritten/deleted slot cannot accept its former result.
            return Ok(Vec::new());
        }
        let scene =
            result.with_context(|| format!("Reconstructing SaveMenu slot {}", job.slot + 1))?;
        self.scenes[job.slot] = Some(scene);
        Ok(vec![job.slot])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shin_core::format::save::{GameData, RouteRecorder};
    fn scenario() -> Arc<Scenario> {
        let header: [u32; 47] = [
            0x20524e53, 0xbc, 0, 6, 19, 0, 0, 0, 0xbc, 0x58, 0x60, 0x68, 0x70, 0x78, 0x80, 0x88,
            0x90, 0x94, 0x98, 0x9c, 0xa4, 0xa8, 4, 0, 4, 0, 4, 0, 4, 0, 4, 0, 4, 0, 4, 0, 0, 0, 0,
            4, 0, 0, 4, 0, 0, 0, 0,
        ];
        Arc::new(
            Scenario::new(
                header
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>()
                    .into(),
            )
            .unwrap(),
        )
    }
    fn entry(seed: u32) -> GameDataEntry {
        GameDataEntry {
            scenario_id: 0,
            random_seed: seed,
            save_position: 0,
            route_data: RouteRecorder::default().snapshot(),
        }
    }
    fn gated_job(
        cache: &SaveMenuSceneCache,
        slot: usize,
        seed: u32,
    ) -> (ReplayJob, std::sync::mpsc::Sender<()>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let scenario = cache.scenario.clone();
        let globals = cache.globals.clone();
        (
            ReplayJob {
                slot,
                generation: cache.generations[slot],
                task: shin_tasks::compute::spawn(move || {
                    receiver.recv()?;
                    SavedSlotScene::reconstruct(&scenario, &entry(seed), &globals, 10).map(Arc::new)
                }),
            },
            sender,
        )
    }
    fn complete(cache: &mut SaveMenuSceneCache, model: &SaveMenuModel) -> Vec<usize> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let ready = cache.update(model).unwrap();
            if cache.pending_slot().is_none() {
                return ready;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "SaveMenu replay timed out"
            );
            std::thread::yield_now();
        }
    }
    #[test]
    fn serial_worker_retains_requested_slot_and_evicts_offscreen_scenes() {
        crate::init_test_task_pools();
        let mut data = SaveData::default();
        for slot in [0, 1, 99] {
            data.manual_save_slots[slot] = Some(GameData::from_entry(entry(slot as u32)));
        }
        let model = SaveMenuModel::new(&data, None);
        let mut cache = SaveMenuSceneCache::new(scenario(), data.clone());
        let (job, release) = gated_job(&cache, 0, 0);
        cache.replay = Some(job);
        assert_eq!(cache.labels(&model, 0), SaveSlotLabels::Loading);
        assert_eq!(cache.labels(&model, 2), SaveSlotLabels::Empty);
        assert!(cache.update(&model).unwrap().is_empty());
        assert_eq!(cache.pending_slot(), Some(0));
        data.save_menu_position = 99;
        let end_model = SaveMenuModel::new(&data, None);
        release.send(()).unwrap();
        assert_eq!(
            complete(&mut cache, &end_model),
            [0],
            "completion belongs to original row despite scrolling"
        );
        assert_eq!(
            cache.scene(0).unwrap().scene.scripter.initial_values(),
            (0, 0)
        );
        cache.update(&end_model).unwrap();
        assert!(
            cache.scene(0).is_none(),
            "offscreen replay result released on next scan"
        );
        if cache.scene(99).is_none() {
            complete(&mut cache, &end_model);
        }
        assert_eq!(
            cache.scene(99).unwrap().scene.scripter.initial_values(),
            (0, 99)
        );
        assert!(cache.scene(1).is_none(), "offscreen row never scheduled");
        assert_eq!(
            cache.labels(&end_model, 99),
            SaveSlotLabels::Ready {
                title: String::new(),
                subtitle: String::new()
            }
        );
    }
    #[test]
    fn overwrite_or_delete_during_replay_cannot_publish_former_scene() {
        crate::init_test_task_pools();
        for delete in [false, true] {
            let mut data = SaveData::default();
            data.manual_save_slots[0] = Some(GameData::from_entry(entry(1)));
            let mut cache = SaveMenuSceneCache::new(scenario(), data.clone());
            let (job, release) = gated_job(&cache, 0, 1);
            cache.replay = Some(job);
            cache.invalidate(0);
            data.manual_save_slots[0] = (!delete).then(|| GameData::from_entry(entry(99)));
            let model = SaveMenuModel::new(&data, None);
            release.send(()).unwrap();
            assert!(complete(&mut cache, &model).is_empty());
            assert!(cache.scene(0).is_none());
            if delete {
                assert_eq!(cache.labels(&model, 0), SaveSlotLabels::Empty);
            } else {
                assert_eq!(cache.labels(&model, 0), SaveSlotLabels::Loading);
                cache.update(&model).unwrap();
                if cache.scene(0).is_none() {
                    complete(&mut cache, &model);
                }
                assert_eq!(
                    cache.scene(0).unwrap().scene.scripter.initial_values(),
                    (0, 99)
                );
            }
        }
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_save_slot_label_snapshot_and_serial_replay_ownership() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let executable = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        // Replay SAVEINFO current pointers, AUTOSAVE snapshots, scene copies,
        // serial busy guard, and completion's retained slot destination.
        for (address, expected) in [
            (0x1049b0, 0xb9800049),
            (0x1049b4, 0xf9400448),
            (0x1049b8, 0x8b091009),
            (0x1049bc, 0xf9305128),
            (0x1049fc, 0xf9705008),
            (0x104a00, 0xf9305408),
            (0x104a04, 0xf9705808),
            (0x104a08, 0xf9305c08),
            (0x102684, 0xf9705668),
            (0x102688, 0xf92d1e88),
            (0x10268c, 0xf9705e68),
            (0x102694, 0xf92d2288),
            (0xd69d0, 0xb9400348),
            (0xd69d4, 0x37f80a28),
            (0xd6f88, 0xb9800353),
            (0xd6fb0, 0xf9012928),
            (0xd6fb4, 0xb4000040),
        ] {
            assert_eq!(
                u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native anchor at {address:x}"
            );
        }
    }
}
