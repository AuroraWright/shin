use super::SettingsGain;
use std::{collections::BTreeMap, sync::Arc};

use kira::track::TrackBuilder;
use shin_audio::AudioTrack as TrackHandle;
use shin_audio::{AudioData, AudioFile, AudioHandle, AudioManager, AudioSettings, AudioTweens};
use shin_core::{
    format::scenario::info::SeId,
    time::{Ticks, Tween, Tweener},
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};
use tracing::warn;

use crate::asset::system::AssetServer;
use shin_tasks::AsyncTask;

pub const SE_SLOT_COUNT: usize = 32;

pub struct SePlayer {
    audio_manager: Arc<AudioManager>,
    settings_gains: [SettingsGain; SE_SLOT_COUNT],
    once_settings_gain: SettingsGain,
    se_tracks: [TrackHandle; SE_SLOT_COUNT],
    se_slots: [Option<AudioHandle>; SE_SLOT_COUNT],
    repeating: [bool; SE_SLOT_COUNT],
    effect_ids: [Option<SeId>; SE_SLOT_COUNT],
    once_track: TrackHandle,
    one_shots: BTreeMap<i32, AudioHandle>,
    stopping: Vec<AudioHandle>,
    paused: bool,
    pending: BTreeMap<i32, PendingEffect>,
}

struct PendingEffect {
    task: AsyncTask<anyhow::Result<Arc<AudioFile>>>,
    effect_id: SeId,
    repeat: bool,
    tweens: AudioTweens,
}

fn pending_tweens(volume: Volume, pan: Pan, speed: f32, fade: Tween) -> AudioTweens {
    let mut volume_tween = Tweener::new(0.0);
    volume_tween.enqueue(volume.0, fade);
    AudioTweens {
        volume: volume_tween,
        panning: Tweener::new(pan.0),
        playback_rate: Tweener::new(native_se_play_speed(speed)),
    }
}

fn pending_status(tweens: &AudioTweens) -> AudioWaitStatus {
    let mut status = AudioWaitStatus::LOADING | AudioWaitStatus::PLAYING;
    for (tween, flag) in [
        (&tweens.volume, AudioWaitStatus::VOLUME_TWEENING),
        (&tweens.panning, AudioWaitStatus::PANNING_TWEENING),
        (&tweens.playback_rate, AudioWaitStatus::PLAY_SPEED_TWEENING),
    ] {
        if !tween.is_idle() {
            status |= flag;
        }
    }
    status
}

impl SePlayer {
    pub fn new(audio_manager: Arc<AudioManager>) -> Self {
        let mut manager = audio_manager.kira_manager().lock();

        let se_tracks = [(); SE_SLOT_COUNT].map(|_| {
            manager
                .add_sub_track(TrackBuilder::new())
                .expect("Failed to create se track")
                .into()
        });

        let once_track = manager
            .add_sub_track(TrackBuilder::new())
            .expect("Failed to create one-shot SE track")
            .into();

        drop(manager);

        Self {
            settings_gains: std::array::from_fn(|_| SettingsGain::default()),
            once_settings_gain: SettingsGain::default(),
            audio_manager,
            se_tracks,
            se_slots: [(); SE_SLOT_COUNT].map(|_| None),
            repeating: [false; SE_SLOT_COUNT],
            effect_ids: [None; SE_SLOT_COUNT],
            once_track,
            one_shots: BTreeMap::new(),
            stopping: Vec::new(),
            paused: false,
            pending: BTreeMap::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn reference_hold_loading(&mut self) -> futures::channel::oneshot::Sender<()> {
        let pending = self.pending.remove(&0).expect("Pending SE slot0");
        let (sender, receiver) = futures::channel::oneshot::channel();
        self.pending.insert(
            0,
            PendingEffect {
                task: shin_tasks::async_io::spawn(async move {
                    receiver
                        .await
                        .expect("Publication gate released or task canceled");
                    pending.task.await
                }),
                effect_id: pending.effect_id,
                repeat: pending.repeat,
                tweens: pending.tweens,
            },
        );
        sender
    }

    #[cfg(test)]
    pub(crate) fn reference_verify_async_loading(
        server: Arc<AssetServer>,
        scenario: &shin_core::format::scenario::Scenario,
    ) {
        use shin_core::format::scenario::instruction_elements::FromNumber;
        let id = SeId::from_number(0);
        let path = scenario.info_tables().se_info(id).path().to_owned();
        let mut player = Self::new(Arc::new(AudioManager::new_for_tests()));
        let fade = Tween::linear(Ticks::from_f32(60.0));
        player.request(
            server.clone(),
            path.clone(),
            0,
            id,
            true,
            Volume(0.8),
            Pan(0.0),
            1.0,
            fade,
        );
        assert_eq!(
            player.get_wait_status(0),
            AudioWaitStatus::LOADING | AudioWaitStatus::PLAYING | AudioWaitStatus::VOLUME_TWEENING
        );
        assert!(player.get_wait_status(-1).is_empty());
        // Matching repeat must reuse the valid load, ignoring this invalid path.
        player.request(
            server.clone(),
            "/missing-repeat-reuse.nxa".into(),
            0,
            id,
            true,
            Volume(0.6),
            Pan(0.5),
            1.5,
            fade,
        );
        assert_eq!(player.pending.len(), 1);
        assert_eq!(player.get_wait_status(0).bits(), 31);
        player.set_volume(0, Volume(0.4), fade);
        player.set_panning(0, Pan(-0.5), fade);
        let once = player.request_once(
            server.clone(),
            path.clone(),
            id,
            Volume::default(),
            Pan::default(),
        );
        let once2 = player.request_once(
            server.clone(),
            path.clone(),
            id,
            Volume::default(),
            Pan::default(),
        );
        assert_eq!((once, once2), (-1, -2));
        assert_eq!(player.get_once_wait_status(once).bits(), 3);
        player.set_paused(true);
        // The file may complete on a worker, but pause must prevent installation.
        let ready: Arc<AudioFile> = server.load_sync(&path).unwrap();
        player.play(0, id, ready, true, Volume(0.4), Pan(-0.5), 1.5, fade);
        assert_eq!(player.pending.len(), 3);
        assert!(player.se_slots[0].is_none());
        player.update();
        assert_eq!(player.pending.len(), 3);
        assert_eq!(player.get_once_wait_status(once).bits(), 3);
        assert!(player.se_slots[0].is_none());
        player.set_paused(false);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !player.pending.is_empty() {
            player.update();
            assert!(
                std::time::Instant::now() < deadline,
                "SE loading did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            player.se_slots[0].is_some(),
            "repeat request replaced its original load"
        );
        assert!(!player.get_wait_status(0).contains(AudioWaitStatus::LOADING));
        assert!(player.get_wait_status(0).contains(AudioWaitStatus::PLAYING));
        // Stopped or replaced loading entries must never install late sources.
        player.request(
            server.clone(),
            path.clone(),
            1,
            id,
            false,
            Volume::default(),
            Pan::default(),
            1.0,
            Tween::IMMEDIATE,
        );
        player.stop(1, Tween::IMMEDIATE);
        assert!(player.get_wait_status(1).is_empty());
        player.update();
        assert!(player.se_slots[1].is_none());
        player.request(
            server.clone(),
            path.clone(),
            2,
            id,
            false,
            Volume::default(),
            Pan::default(),
            1.0,
            Tween::IMMEDIATE,
        );
        player.request(
            server.clone(),
            "/missing-replacement.nxa".into(),
            2,
            id,
            false,
            Volume::default(),
            Pan::default(),
            1.0,
            Tween::IMMEDIATE,
        );
        while player.pending.contains_key(&2) {
            player.update();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(player.get_wait_status(2).is_empty());
        assert!(player.effect_ids[2].is_none());
        player.request(
            server.clone(),
            path,
            3,
            id,
            false,
            Volume::default(),
            Pan::default(),
            1.0,
            Tween::IMMEDIATE,
        );
        player.stop_all(Tween::IMMEDIATE);
        assert!(player.pending.is_empty());
        assert!(player.get_wait_status(-1).is_empty());
        assert!(player.get_wait_status(0).is_empty());
    }

    #[cfg(test)]
    pub(crate) fn reference_is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, paused: bool) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        for handle in self
            .se_slots
            .iter()
            .flatten()
            .chain(self.one_shots.values())
            .chain(self.stopping.iter())
        {
            handle.set_paused(paused);
        }
    }

    pub fn set_settings_volume(&mut self, percent: u8) {
        for (gain, track) in self.settings_gains.iter_mut().zip(&mut self.se_tracks) {
            gain.apply(track, percent);
        }
        self.once_settings_gain.apply(&mut self.once_track, percent);
    }

    /// Queue native-style loading without blocking the ADV thread. Matching
    /// repeats reuse both ready sources and requests that have not finished.
    pub fn request(
        &mut self,
        server: Arc<AssetServer>,
        path: String,
        slot: i32,
        effect_id: SeId,
        repeat: bool,
        volume: Volume,
        pan: Pan,
        speed: f32,
        fade: Tween,
    ) {
        if let Some(pending) = self.pending.get_mut(&slot) {
            if pending.repeat && repeat && pending.effect_id == effect_id {
                pending.tweens.volume.enqueue_now(volume.0, fade);
                pending.tweens.panning.enqueue_now(pan.0, fade);
                pending
                    .tweens
                    .playback_rate
                    .enqueue_now(native_se_play_speed(speed), fade);
                return;
            }
        }
        if slot >= 0
            && retarget_matching_repeat(
                &mut self.se_slots[slot as usize],
                self.effect_ids[slot as usize],
                self.repeating[slot as usize],
                effect_id,
                repeat,
                volume,
                pan,
                speed,
                fade,
            )
        {
            return;
        }
        if slot >= 0 {
            stop_effect(
                &mut self.se_slots[slot as usize],
                &mut self.stopping,
                Tween::IMMEDIATE,
            );
            self.effect_ids[slot as usize] = Some(effect_id);
            self.repeating[slot as usize] = repeat;
        }
        let task = shin_tasks::async_io::spawn(async move { server.load(&path).await });
        self.pending.insert(
            slot,
            PendingEffect {
                task,
                effect_id,
                repeat,
                tweens: pending_tweens(volume, pan, speed, fade),
            },
        );
    }

    /// SEONCE reserves independent negative slots before loading (native 0x7320).
    pub fn request_once(
        &mut self,
        server: Arc<AssetServer>,
        path: String,
        effect_id: SeId,
        volume: Volume,
        pan: Pan,
    ) -> i32 {
        self.one_shots
            .retain(|_, handle| !handle.get_wait_status().is_empty());
        let slot = (i32::MIN..=-1)
            .rev()
            .find(|slot| !self.one_shots.contains_key(slot) && !self.pending.contains_key(slot))
            .expect("No free one-shot SE slot");
        self.request(
            server,
            path,
            slot,
            effect_id,
            false,
            volume,
            pan,
            1.0,
            Tween::IMMEDIATE,
        );
        slot
    }

    pub fn play(
        &mut self,
        slot: i32,
        effect_id: SeId,
        se: Arc<AudioFile>,
        repeat: bool,
        volume: Volume,
        pan: Pan,
        play_speed: f32,
        fade_in: Tween,
    ) {
        if let Some(pending) = self.pending.get_mut(&slot) {
            if pending.repeat && repeat && pending.effect_id == effect_id {
                pending.tweens.volume.enqueue_now(volume.0, fade_in);
                pending.tweens.panning.enqueue_now(pan.0, fade_in);
                pending
                    .tweens
                    .playback_rate
                    .enqueue_now(native_se_play_speed(play_speed), fade_in);
                return;
            }
        }
        self.pending.remove(&slot);
        let slot = slot as usize;

        if retarget_matching_repeat(
            &mut self.se_slots[slot],
            self.effect_ids[slot],
            self.repeating[slot],
            effect_id,
            repeat,
            volume,
            pan,
            play_speed,
            fade_in,
        ) {
            return;
        }

        // Replacement requests zero ticks;native stop still clamps to four.
        stop_effect(
            &mut self.se_slots[slot],
            &mut self.stopping,
            Tween::IMMEDIATE,
        );
        let loop_start = repeat.then_some(se.info().loop_start);
        let kira_data = match AudioData::from_audio_file(
            se,
            AudioSettings {
                paused: self.paused,
                playback_rate: native_se_play_speed(play_speed),

                fade_in,
                loop_start,
                loop_count: 0,
                volume,
                pan,
            },
        ) {
            Ok(data) => data,
            Err(error) => {
                warn!(%error, slot, "Failed to prepare sound effect");
                self.effect_ids[slot] = None;
                self.repeating[slot] = false;
                return;
            }
        };

        let handle = self
            .audio_manager
            .play_on_track(&mut self.se_tracks[slot], kira_data);

        self.se_slots[slot] = Some(handle);
        self.repeating[slot] = repeat;
        self.effect_ids[slot] = Some(effect_id);
    }

    /// Native0xf1c40 shortens inactive entries to four ticks; active negative
    /// one-shot slots are outside the32-slot restoration loop and remain owned.
    pub fn prepare_restore(&mut self) {
        normalize_stopping_effects(&mut self.stopping);
    }

    #[cfg(test)]
    pub(crate) fn reference_pause_once(&self, slot: i32, paused: bool) {
        if let Some(handle) = self.one_shots.get(&slot) {
            handle.set_paused(paused);
        }
    }

    pub fn restore_empty_slot(&mut self, slot: usize) {
        if self.se_slots[slot].is_some() || self.pending.contains_key(&(slot as i32)) {
            self.stop(slot as i32, Tween::IMMEDIATE);
        }
    }

    #[cfg(test)]
    pub(crate) fn reference_source(&self, slot: usize) -> (Option<SeId>, u32, u64) {
        (
            self.effect_ids[slot],
            self.se_slots[slot]
                .as_ref()
                .map_or(0, AudioHandle::position_millis),
            self.se_tracks[slot].id(),
        )
    }
    #[cfg(test)]
    pub(crate) fn reference_pause_source(&self, slot: usize, paused: bool) {
        if let Some(handle) = &self.se_slots[slot] {
            handle.set_paused(paused);
        }
    }

    pub fn get_once_wait_status(&self, slot: i32) -> AudioWaitStatus {
        if let Some(pending) = self.pending.get(&slot) {
            return pending_status(&pending.tweens);
        }
        self.one_shots
            .get(&slot)
            .map_or(AudioWaitStatus::empty(), AudioHandle::get_wait_status)
    }

    pub fn set_volume(&mut self, slot: i32, volume: Volume, tween: Tween) {
        if let Some(pending) = self.pending.get_mut(&slot) {
            pending.tweens.volume.enqueue_now(volume.0, tween);
            return;
        }
        if slot < 0 {
            if let Some(handle) = self.one_shots.get_mut(&slot) {
                handle.set_volume(volume, tween).unwrap();
            }
            return;
        }
        let slot = slot as usize;

        if let Some(handle) = self.se_slots[slot].as_mut() {
            handle.set_volume(volume, tween).unwrap();
        } else {
            warn!(
                "Tried to set volume of se slot {}, but there was no se playing",
                slot
            );
        }
    }

    pub fn set_panning(&mut self, slot: i32, pan: Pan, tween: Tween) {
        if let Some(pending) = self.pending.get_mut(&slot) {
            pending.tweens.panning.enqueue(pan.0, tween);
            return;
        }
        if slot < 0 {
            if let Some(handle) = self.one_shots.get_mut(&slot) {
                handle.enqueue_panning(pan, tween).unwrap();
            }
            return;
        }
        let slot = slot as usize;

        if let Some(handle) = self.se_slots[slot].as_mut() {
            handle.enqueue_panning(pan, tween).unwrap();
        } else {
            warn!(
                "Tried to set pan of se slot {}, but there was no se playing",
                slot
            );
        }
    }

    pub fn stop(&mut self, slot: i32, fade_out: Tween) {
        let was_pending = self.pending.remove(&slot).is_some();
        if slot < 0 {
            stop_effect(
                &mut self.one_shots.remove(&slot),
                &mut self.stopping,
                fade_out,
            );
            return;
        }
        let slot = slot as usize;

        self.repeating[slot] = false;
        self.effect_ids[slot] = None;
        if !stop_effect(&mut self.se_slots[slot], &mut self.stopping, fade_out) && !was_pending {
            warn!("Tried to stop a SE that was not playing");
        }
    }

    /// Retire story effects before handing the shared tracks to the title.
    /// Cancellation includes unpublished loads and paused/stopping voices;
    /// normal script SESTOP retains its native four-tick minimum fade.
    pub fn retire_scene(&mut self) {
        self.pending.clear();
        for handle in self
            .se_slots
            .iter_mut()
            .filter_map(Option::take)
            .chain(std::mem::take(&mut self.one_shots).into_values())
            .chain(self.stopping.drain(..))
        {
            handle.stop_immediate();
        }
        self.repeating.fill(false);
        self.effect_ids.fill(None);
    }
    #[cfg(test)]
    pub(crate) fn reference_scene_effect_count(&self) -> usize {
        self.se_slots.iter().flatten().count()
            + self.pending.len()
            + self.one_shots.len()
            + self.stopping.len()
    }

    pub fn stop_all(&mut self, fade_out: Tween) {
        for slot in 0..SE_SLOT_COUNT {
            if self.se_slots[slot].is_some() || self.pending.contains_key(&(slot as i32)) {
                self.stop(slot as i32, fade_out);
            }
        }
        self.pending.clear();
        for (_, handle) in std::mem::take(&mut self.one_shots) {
            stop_effect(&mut Some(handle), &mut self.stopping, fade_out);
        }
    }

    pub fn update(&mut self) {
        if self.paused {
            return;
        }
        let ready: Vec<_> = self
            .pending
            .iter_mut()
            .filter_map(|(&slot, pending)| pending.task.poll_naive().map(|result| (slot, result)))
            .collect();
        for (slot, result) in ready {
            let pending = self.pending.remove(&slot).unwrap();
            let se = match result {
                Ok(se) => se,
                Err(error) => {
                    warn!(%error, slot, "Failed to load sound effect");
                    if slot >= 0 {
                        self.effect_ids[slot as usize] = None;
                        self.repeating[slot as usize] = false;
                    }
                    continue;
                }
            };
            let loop_start = pending.repeat.then_some(se.info().loop_start);
            let data = match AudioData::from_audio_file(
                se,
                AudioSettings {
                    paused: false,
                    fade_in: Tween::IMMEDIATE,
                    loop_start,
                    loop_count: 0,
                    volume: Volume(pending.tweens.volume.value()),
                    pan: Pan(pending.tweens.panning.value()),
                    playback_rate: pending.tweens.playback_rate.value(),
                },
            ) {
                Ok(data) => data.with_tweens(pending.tweens),
                Err(error) => {
                    warn!(%error, slot, "Failed to prepare sound effect");
                    if slot >= 0 {
                        self.effect_ids[slot as usize] = None;
                        self.repeating[slot as usize] = false;
                    }
                    continue;
                }
            };
            let track = if slot >= 0 {
                &mut self.se_tracks[slot as usize]
            } else {
                &mut self.once_track
            };
            let handle = self.audio_manager.play_on_track(track, data);
            if slot >= 0 {
                self.se_slots[slot as usize] = Some(handle);
            } else {
                self.one_shots.insert(slot, handle);
            }
        }
        self.stopping
            .retain(|handle| !handle.get_wait_status().is_empty());
        self.one_shots
            .retain(|_, handle| !handle.get_wait_status().is_empty());
    }

    pub fn get_wait_status(&self, slot: i32) -> AudioWaitStatus {
        let status = se_wait_status(&self.se_slots, &self.repeating, &self.one_shots, slot);
        self.pending
            .iter()
            .filter(|(key, pending)| {
                if slot >= 0 {
                    **key == slot
                } else {
                    !pending.repeat
                }
            })
            .fold(status, |status, (_, pending)| {
                status | pending_status(&pending.tweens)
            })
    }
}

impl Drop for SePlayer {
    fn drop(&mut self) {
        for handle in self
            .se_slots
            .iter_mut()
            .filter_map(Option::as_mut)
            .chain(self.one_shots.values_mut())
            .chain(&mut self.stopping)
        {
            let _ = handle.stop(Tween::IMMEDIATE);
        }
    }
}

fn normalize_stopping_effects(tails: &mut Vec<AudioHandle>) {
    tails.retain(|handle| !handle.get_wait_status().is_empty());
    for handle in tails {
        let _ = handle.stop(Tween::linear(Ticks::from_f32(4.0)));
    }
}

fn native_se_stop_tween(tween: Tween) -> Tween {
    //0xf2438 selects max(requested,4) BEFORE0xf243c tests forzero.
    Tween::linear(tween.duration.max(Ticks::from_f32(4.0)))
}

/// Remove the active entry immediately;the native wait poll ignores its tail.
fn stop_effect(
    handle: &mut Option<AudioHandle>,
    tails: &mut Vec<AudioHandle>,
    tween: Tween,
) -> bool {
    let Some(mut handle) = handle.take() else {
        return false;
    };
    let tween = native_se_stop_tween(tween);
    handle.stop(tween).unwrap();
    if tween.duration > Ticks::ZERO {
        tails.push(handle);
    }
    true
}

/// Native0xf1e9c..0xf1ebc clamps the requested source-rate multiplier.
fn native_se_play_speed(speed: f32) -> f32 {
    speed.max(0.01).min(10.0)
}

/// Native0xf1edc..0xf1f08 reuses only matching active repeating entries.
/// The0xf21b8 branch clears and retargets all three interpolators together.
fn retarget_matching_repeat(
    handle: &mut Option<AudioHandle>,
    current_id: Option<SeId>,
    current_repeat: bool,
    requested_id: SeId,
    repeat: bool,
    volume: Volume,
    pan: Pan,
    speed: f32,
    tween: Tween,
) -> bool {
    if current_id != Some(requested_id) || !current_repeat || !repeat {
        return false;
    }
    let Some(handle) = handle.as_mut() else {
        return false;
    };
    if handle.get_wait_status().is_empty() {
        return false;
    }
    handle
        .set_parameters(volume, pan, native_se_play_speed(speed), tween)
        .unwrap();
    true
}

fn se_wait_status(
    slots: &[Option<AudioHandle>; SE_SLOT_COUNT],
    repeating: &[bool; SE_SLOT_COUNT],
    one_shots: &BTreeMap<i32, AudioHandle>,
    slot: i32,
) -> AudioWaitStatus {
    // Every negative argument means all transient effects. Repeating
    // positive slots are excluded by the original SEWAIT poll (0x71e0).
    if slot < 0 {
        return slots
            .iter()
            .zip(repeating)
            .filter(|(_, repeating)| !**repeating)
            .filter_map(|(handle, _)| handle.as_ref())
            .chain(one_shots.values())
            .fold(AudioWaitStatus::empty(), |status, handle| {
                status | handle.get_wait_status()
            });
    }
    slots
        .get(slot as usize)
        .and_then(Option::as_ref)
        .map_or(AudioWaitStatus::empty(), AudioHandle::get_wait_status)
}

#[cfg(test)]
mod tests;
