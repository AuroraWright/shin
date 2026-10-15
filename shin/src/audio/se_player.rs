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
mod tests {
    use super::*;
    use kira::{AudioManager as KiraManager, AudioManagerSettings};
    use shin_audio::test_support::{MockBackendSettings, SampleBackend as MockBackend};
    use shin_core::format::{
        audio::{AudioBuffer, AudioFrameSource},
        scenario::instruction_elements::FromNumber,
    };

    struct Tone {
        position: u32,
    }
    impl AudioFrameSource for Tone {
        fn max_frame_size(&self) -> usize {
            8
        }
        fn sample_rate(&self) -> u32 {
            1000
        }
        fn pre_skip(&self) -> u32 {
            0
        }
        fn pre_roll(&self) -> u32 {
            0
        }
        fn read_frame(&mut self, buffer: &mut AudioBuffer) -> bool {
            if self.position >= 32 {
                return false;
            }
            for _ in 0..8 {
                buffer.push((0.1, 0.1));
            }
            self.position += 8;
            true
        }
        fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
            self.position = position;
            Ok(0)
        }
        fn current_sample_position(&self) -> u32 {
            self.position
        }
    }

    fn audio(repeat: bool) -> AudioData<Tone> {
        AudioData {
            source: Tone { position: 0 },
            settings: AudioSettings {
                paused: false,
                playback_rate: 1.0,

                fade_in: Tween::IMMEDIATE,
                loop_start: repeat.then_some(0),
                loop_count: 0,
                volume: Volume::default(),
                pan: Pan::default(),
            },
        }
    }

    #[test]
    fn pending_parameters_start_at_playback_and_preserve_queued_pan() {
        let mut tweens = pending_tweens(
            Volume(0.8),
            Pan(0.0),
            1.0,
            Tween::linear(Ticks::from_f32(12.0)),
        );
        assert_eq!(
            pending_status(&tweens),
            AudioWaitStatus::LOADING | AudioWaitStatus::PLAYING | AudioWaitStatus::VOLUME_TWEENING
        );
        tweens
            .volume
            .enqueue_now(0.6, Tween::linear(Ticks::from_f32(6.0)));
        tweens
            .panning
            .enqueue(1.0, Tween::linear(Ticks::from_f32(6.0)));
        tweens
            .panning
            .enqueue(-1.0, Tween::linear(Ticks::from_f32(6.0)));
        let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let handle = manager.play(audio(true).with_tweens(tweens)).unwrap();
        assert_eq!(
            handle.get_wait_status(),
            AudioWaitStatus::PLAYING
                | AudioWaitStatus::VOLUME_TWEENING
                | AudioWaitStatus::PANNING_TWEENING
        );
        manager.backend_mut().on_start_processing();
        for _ in 0..105 {
            manager.backend_mut().on_start_processing();
            let _ = manager.backend_mut().process();
        }
        assert!(
            !handle
                .get_wait_status()
                .contains(AudioWaitStatus::VOLUME_TWEENING)
        );
        assert!(
            handle
                .get_wait_status()
                .contains(AudioWaitStatus::PANNING_TWEENING)
        );
        manager.backend_mut().on_start_processing();
        for _ in 0..105 {
            manager.backend_mut().on_start_processing();
            let _ = manager.backend_mut().process();
        }
        assert_eq!(handle.get_wait_status(), AudioWaitStatus::PLAYING);
        let frame = manager.backend_mut().process();
        assert!(frame.left > 0.08 && frame.right.abs() < 0.00001);
    }

    #[test]
    fn restore_shortens_owned_inactive_tail_to_four_ticks_without_cutting_it() {
        let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let mut active = Some(manager.play(audio(true)).unwrap());
        let mut tails = Vec::new();
        stop_effect(
            &mut active,
            &mut tails,
            Tween::linear(Ticks::from_f32(30.0)),
        );
        for _ in 0..10 {
            manager.backend_mut().on_start_processing();
            let _ = manager.backend_mut().process();
        }
        normalize_stopping_effects(&mut tails);
        let mut frame = kira::Frame::ZERO;
        for _ in 0..30 {
            manager.backend_mut().on_start_processing();
            frame = manager.backend_mut().process();
        }
        assert!(frame.left > 0.04 && frame.left < 0.06);
        assert!(
            tails[0]
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
        );
        for _ in 0..40 {
            manager.backend_mut().on_start_processing();
            frame = manager.backend_mut().process();
        }
        assert_eq!(frame, kira::Frame::ZERO);
        normalize_stopping_effects(&mut tails);
        assert!(tails.is_empty());
    }

    #[test]
    fn native_se_speed_clamps_without_reversing_or_freezing_audio() {
        for (input, expected) in [
            (-2.0, 0.01),
            (0.0, 0.01),
            (0.001, 0.01),
            (0.01, 0.01),
            (0.5, 0.5),
            (1.0, 1.0),
            (1.5, 1.5),
            (10.0, 10.0),
            (100.0, 10.0),
        ] {
            assert_eq!(native_se_play_speed(input), expected);
        }
        assert_eq!(native_se_play_speed(f32::NAN), 0.01);
    }

    #[test]
    fn native_stop_always_has_at_least_four_ticks() {
        for (requested, expected) in [
            (-1.0, 4.0),
            (0.0, 4.0),
            (0.5, 4.0),
            (1.0, 4.0),
            (4.0, 4.0),
            (10.0, 10.0),
        ] {
            assert_eq!(
                native_se_stop_tween(Tween::linear(Ticks::from_f32(requested))).duration,
                Ticks::from_f32(expected)
            );
        }
    }

    #[test]
    fn matching_repeat_retargets_live_pcm_without_restarting_the_source() {
        let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let mut handle = Some(manager.play(audio(true)).unwrap());
        let advance = |manager: &mut KiraManager<MockBackend>, count| {
            let backend = manager.backend_mut();
            let mut frame = kira::Frame::ZERO;
            for _ in 0..count {
                backend.on_start_processing();
                frame = backend.process();
            }
            frame
        };
        advance(&mut manager, 12);
        let before = handle.as_ref().unwrap().position_millis();
        let duration = Tween::linear(shin_core::time::Ticks::from_millis(10.0));
        for (id, old_repeat, new_id, new_repeat) in [
            (Some(SeId::from_number(1)), true, SeId::from_number(2), true),
            (
                Some(SeId::from_number(1)),
                false,
                SeId::from_number(1),
                true,
            ),
            (
                Some(SeId::from_number(1)),
                true,
                SeId::from_number(1),
                false,
            ),
            (None, true, SeId::from_number(1), true),
        ] {
            assert!(!retarget_matching_repeat(
                &mut handle,
                id,
                old_repeat,
                new_id,
                new_repeat,
                Volume(0.5),
                Pan(-1.0),
                2.0,
                duration
            ));
            assert_eq!(handle.as_ref().unwrap().position_millis(), before);
        }
        // Three updates before the next audio callback must use the latest
        // target without replacing the source or exhausting the command queue.
        for volume in [0.8, 0.6, 0.5] {
            assert!(retarget_matching_repeat(
                &mut handle,
                Some(SeId::from_number(1)),
                true,
                SeId::from_number(1),
                true,
                Volume(volume),
                Pan(-1.0),
                2.0,
                duration,
            ));
        }
        advance(&mut manager, 5);
        let live = handle.as_ref().unwrap();
        assert!(
            live.position_millis() > before + 4,
            "Matching repeat must retain and advance its existing decoder"
        );
        assert!(live.get_wait_status().contains(
            AudioWaitStatus::VOLUME_TWEENING
                | AudioWaitStatus::PANNING_TWEENING
                | AudioWaitStatus::PLAY_SPEED_TWEENING
        ));
        let frame = advance(&mut manager, 12);
        assert!((frame.left - 0.05 * std::f32::consts::SQRT_2).abs() < 0.00001);
        assert!(frame.right.abs() < 0.00001);
        assert_eq!(
            handle.as_ref().unwrap().get_wait_status(),
            AudioWaitStatus::PLAYING
        );
        handle.as_mut().unwrap().stop(Tween::IMMEDIATE).unwrap();
        advance(&mut manager, 4);
        assert!(!retarget_matching_repeat(
            &mut handle,
            Some(SeId::from_number(1)),
            true,
            SeId::from_number(1),
            true,
            Volume(1.0),
            Pan(0.0),
            1.0,
            duration
        ));
    }

    #[test]
    fn zero_duration_replacement_retains_the_native_four_tick_tail() {
        let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let mut old = Some(manager.play(audio(true)).unwrap());
        for _ in 0..12 {
            let backend = manager.backend_mut();
            backend.on_start_processing();
            backend.process();
        }
        let mut tails = Vec::new();
        assert!(stop_effect(&mut old, &mut tails, Tween::IMMEDIATE));
        assert!(old.is_none());
        assert_eq!(tails.len(), 1);
        let replacement = manager.play(audio(true)).unwrap();
        let backend = manager.backend_mut();
        backend.on_start_processing();
        let first = backend.process();
        assert!(
            first.left > 0.09,
            "Old source retains the native four-tick fade while the new source starts"
        );
        let mut frame = kira::Frame::ZERO;
        for _ in 0..75 {
            backend.on_start_processing();
            frame = backend.process();
        }
        assert!(tails[0].get_wait_status().is_empty());
        assert!((frame.left - 0.1).abs() < 0.00001);
        assert!(
            replacement
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
        );
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_se_play_speed() {
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
            (0xf1c50, 0xa9415013),
            (0xf1c60, 0x1e221009),
            (0xf1c68, 0x39400109),
            (0xf1c6c, 0x340000a9),
            (0xf1c80, 0x91006100),
            (0xf1c84, 0x97ff25e7),
            (0xf1c8c, 0x4ea91d21),
            (0xf1c98, 0x97ff275a),
            (0x1aa5c, 0x2d7f0500),
            (0x1aa60, 0xbd400102),
            (0x1aa64, 0x2a1503e1),
            (0x1aa68, 0x2a1f03e3),
            (0x1aa6c, 0x2a1f03e4),
            (0x1aa70, 0x94035cf8),
            (0x1aa78, 0x2a1503e1),
            (0x1aa80, 0x94035e60),
            (0x1aa90, 0xf10082bf),
            (0x1aa94, 0x54fffda1),
            (0xf2054, 0x390002a8),
            (0xf205c, 0x2901debf),
            (0xf2070, 0x97ff2664),
            (0xf207c, 0x97ff2499),
            (0xf20c4, 0x9400066f),
            (0xf20cc, 0xc85ffed7),
            (0xf20d4, 0x35ffffc9),
            (0xf2900, 0x7100011f),
            (0xf2908, 0x91006260),
            (0xf290c, 0x1a881514),
            (0xf2b04, 0x39400008),
            (0xf2bb4, 0x91048315),
            (0xf2bbc, 0x940007f5),
            (0xf2bc4, 0xb9000f1a),
            (0xf2bcc, 0x940007b5),
            (0xf2be8, 0x97ff239a),
            (0xf2bf8, 0x97ff2396),
            (0xf2c08, 0x97ff2392),
            (0xf2814, 0x91006140),
            (0xf281c, 0x2a1f03e2),
            (0xf2884, 0x9101c140),
            (0xf288c, 0x2a1f03e2),
            (0xbb6e0, 0x12180688),
            (0xbb6f0, 0x7104011f),
            (0xbb88c, 0x97fffe95),
            (0xbb890, 0x2d428660),
            (0xbb898, 0x540003e5),
            (0xbb910, 0x14000029),
            (0x6f24, 0xbd403263),
            (0x6f34, 0x5e21d863),
            (0x6f38, 0xbd4a5902),
            (0x6f4c, 0x1e220862),
            (0x6f54, 0x9403abbf),
            (0xf1e9c, 0xbd494901),
            (0xf1ea4, 0x1e249000),
            (0xf1eb4, 0x1e212040),
            (0xf1eb8, 0x1e205840),
            (0xf1ebc, 0x1e218c08),
            (0xf2080, 0x4ea81d00),
            (0xf2088, 0x97ff2496),
            (0xf2c00, 0x91032315),
            (0xf2c58, 0x97ff2212),
            (0xf2c60, 0x94000838),
            (0xf4d68, 0xbd02aa88),
            (0xf1f20, 0x2a1f03e2),
            (0xf1f24, 0x94000137),
            (0xf2430, 0x710012df),
            (0xf2434, 0x321e03e8),
            (0xf2438, 0x1a88c2c8),
            (0xf243c, 0x7100011f),
            (0xf2440, 0x540002ad),
            (0xf2468, 0x97ff23ee),
            (0xf247c, 0x97ff2561),
            (0xf2480, 0x390002df),
            (0xf24f0, 0x97fffdac),
            (0xf1d2c, 0x39400328),
            (0xf1d30, 0x340005e8),
            (0xf28bc, 0x3940026a),
            (0xf28c0, 0x3400008a),
            (0xf28c8, 0x6b01015f),
            (0xf28cc, 0x54000180),
            (0xf1ee0, 0x394002aa),
            (0xf1ee4, 0x3400014a),
            (0xf1eec, 0x6b19015f),
            (0xf1ef8, 0x6b18015f),
            (0xf1f00, 0xb94012aa),
            (0xf1f04, 0x2a17014a),
            (0xf1f08, 0x3400158a),
            (0xf21bc, 0x97ff2499),
            (0xf21c0, 0x1e2202cb),
            (0xf21d4, 0x97ff260b),
            (0xf21dc, 0x97ff2491),
            (0xf21f0, 0x97ff2604),
            (0xf21f4, 0x910322a0),
            (0xf21f8, 0x97ff248a),
            (0xf220c, 0x97ff25fd),
            (0xf2930, 0x91032260),
            (0xf2938, 0x97ff242e),
            (0xf293c, 0x321c0288),
        ] {
            assert_eq!(word(a), expected, "native SE speed{a:x}");
        }
        assert_eq!(word(0x159a58), 0.001f32.to_bits());
        assert_eq!(word(0x159948), 0.01f32.to_bits());
    }

    #[test]
    fn wait_all_includes_one_shots_and_excludes_repeating_tracks() {
        let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let mut slots = [(); SE_SLOT_COUNT].map(|_| None);
        slots[0] = Some(manager.play(audio(true)).unwrap());
        let mut repeating = [false; SE_SLOT_COUNT];
        repeating[0] = true;
        let mut one_shots = BTreeMap::new();
        assert!(se_wait_status(&slots, &repeating, &one_shots, -1).is_empty());
        assert!(
            se_wait_status(&slots, &repeating, &one_shots, 0).contains(AudioWaitStatus::PLAYING)
        );
        one_shots.insert(-1, manager.play(audio(false)).unwrap());
        one_shots.insert(-2, manager.play(audio(false)).unwrap());
        assert!(
            se_wait_status(&slots, &repeating, &one_shots, -2).contains(AudioWaitStatus::PLAYING)
        );
        let backend = manager.backend_mut();
        for _ in 0..64 {
            backend.on_start_processing();
            backend.process();
        }
        assert!(se_wait_status(&slots, &repeating, &one_shots, -1).is_empty());
        assert!(
            se_wait_status(&slots, &repeating, &one_shots, 0).contains(AudioWaitStatus::PLAYING)
        );
        // Native SESTOP removes the entry from both slot and all-effects
        // waits immediately;its audible fade remains owned independently.
        let mut tails = Vec::new();
        assert!(stop_effect(
            &mut slots[0],
            &mut tails,
            Tween::linear(Ticks::from_f32(1.0))
        ));
        repeating[0] = false;
        assert!(se_wait_status(&slots, &repeating, &one_shots, 0).is_empty());
        assert!(se_wait_status(&slots, &repeating, &one_shots, -1).is_empty());
        backend.on_start_processing();
        let first = backend.process();
        assert!(first.left > 0.09);
        assert_eq!(tails.len(), 1);
        assert!(
            tails[0]
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
        );
        for _ in 0..60 {
            backend.on_start_processing();
            backend.process();
        }
        assert!(
            tails[0]
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
        );
        for _ in 0..10 {
            backend.on_start_processing();
            backend.process();
        }
        assert!(tails[0].get_wait_status().is_empty());
        tails.retain(|handle| !handle.get_wait_status().is_empty());
        assert!(tails.is_empty());
    }
}
