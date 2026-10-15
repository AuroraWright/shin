use super::{SettingsGain, voice_focus::VoiceFocus};
use std::sync::Arc;

use crate::asset::system::AssetServer;
use kira::track::TrackBuilder;
use shin_audio::AudioTrack as TrackHandle;
use shin_audio::{AudioData, AudioFile, AudioHandle, AudioManager, AudioSettings, AudioTweens};
use shin_core::{
    format::scenario::info::BgmId,
    time::{Ticks, Tween, Tweener},
    vm::command::types::{AudioWaitStatus, Pan, Volume},
};
use shin_tasks::AsyncTask;
use tracing::warn;

pub struct BgmPlayer {
    audio_manager: Arc<AudioManager>,
    settings_gain: SettingsGain,
    bgm_track: TrackHandle,
    focus_track: TrackHandle,
    voice_focus: VoiceFocus,
    pending: Option<PendingBgm>,
    current_bgm: Option<AudioHandle>,
    current_id: Option<BgmId>,
    stopping: Vec<AudioHandle>,
    paused: bool,
}

struct PendingBgm {
    task: AsyncTask<anyhow::Result<Arc<AudioFile>>>,
    loop_count: u32,
    volume: Tweener,
}

fn pending_volume(volume: Volume, fade: Tween) -> Tweener {
    let mut tweener = Tweener::new(0.0);
    tweener.enqueue(volume.0, fade);
    tweener
}

fn pending_status(volume: &Tweener) -> AudioWaitStatus {
    let mut status = AudioWaitStatus::LOADING | AudioWaitStatus::PLAYING;
    if !volume.is_idle() {
        status |= AudioWaitStatus::VOLUME_TWEENING;
    }
    status
}

impl BgmPlayer {
    #[cfg(test)]
    pub(crate) fn reference_focus_gain(&self) -> f32 {
        self.voice_focus.gain
    }
    #[cfg(test)]
    pub(crate) fn reference_loading_repeat(&self) -> Option<bool> {
        self.pending.as_ref().map(|pending| pending.loop_count == 0)
    }
    #[cfg(test)]
    pub(crate) fn reference_loading_loop_count(&self) -> Option<u32> {
        self.pending.as_ref().map(|pending| pending.loop_count)
    }
    pub fn new(audio_manager: Arc<AudioManager>) -> Self {
        let mut manager = audio_manager.kira_manager().lock();

        let mut bgm_track: TrackHandle = manager
            .add_sub_track(TrackBuilder::new())
            .expect("Failed to create bgm track")
            .into();

        let focus_track = bgm_track
            .add_sub_track(TrackBuilder::new())
            .expect("Failed to create voice focus track")
            .into();

        drop(manager);

        Self {
            settings_gain: SettingsGain::default(),
            audio_manager,
            bgm_track,
            focus_track,
            voice_focus: VoiceFocus::default(),
            current_bgm: None,
            pending: None,
            current_id: None,
            stopping: Vec::new(),
            paused: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn reference_hold_loading(&mut self) -> futures::channel::oneshot::Sender<()> {
        let pending = self.pending.take().expect("Pending BGM request");
        let (sender, receiver) = futures::channel::oneshot::channel();
        self.pending = Some(PendingBgm {
            task: shin_tasks::async_io::spawn(async move {
                receiver
                    .await
                    .expect("Publication gate released or task canceled");
                pending.task.await
            }),
            loop_count: pending.loop_count,
            volume: pending.volume,
        });
        sender
    }

    #[cfg(test)]
    pub(crate) fn reference_verify_async_loading(
        server: Arc<AssetServer>,
        scenario: &shin_core::format::scenario::Scenario,
    ) {
        use shin_core::format::scenario::instruction_elements::FromNumber;
        let (index, info) = scenario
            .info_tables()
            .bgm_info
            .iter()
            .enumerate()
            .find(|(_, info)| info.linked_bgm_id.repr().is_some())
            .unwrap();
        let id = BgmId::from_number(index as i32);
        let path = info.path().to_owned();
        let mut player = Self::new(Arc::new(AudioManager::new_for_tests()));
        let fade = Tween::linear(Ticks::from_f32(60.0));
        assert!(player.request(server.clone(), path.clone(), id, true, Volume(0.8), fade));
        assert_eq!(player.get_wait_status().bits(), 7);
        assert_eq!(player.position(), Ticks::ZERO);
        assert!(!player.request(
            server.clone(),
            "/missing-bgm-reuse.nxa".into(),
            id,
            false,
            Volume(0.6),
            fade
        ));
        player.set_volume(Volume(0.4), fade);
        let ready: Arc<AudioFile> = server.load_sync(&path).unwrap();
        assert!(!player.play(id, ready, "", false, Volume(0.4), fade));
        let pending = player.pending.as_ref().unwrap();
        assert!(
            pending.loop_count == 0,
            "same-ID request changed the original loop policy"
        );
        assert_eq!(pending.volume.value(), 0.0);
        assert_eq!(pending.volume.target_value(), 0.4);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while player.pending.is_some() {
            player.update();
            assert!(
                std::time::Instant::now() < deadline,
                "BGM loading did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            player.current_bgm.is_some(),
            "matching request replaced the original load"
        );
        assert_eq!(player.current_id, Some(id));
        assert_eq!(player.get_wait_status().bits(), 6);
        assert!(!player.request(
            server.clone(),
            "/missing-loaded-reuse.nxa".into(),
            id,
            false,
            Volume(0.5),
            fade
        ));
        assert!(player.pending.is_none());
        // Replace audible music with a failing load: waits follow the new
        // loading entry, never the still-owned old fade-out tail.
        let replacement = BgmId::from_number(id.into_i32() + 1);
        assert!(player.request(
            server.clone(),
            "/missing-bgm-replacement.nxa".into(),
            replacement,
            false,
            Volume::default(),
            Tween::IMMEDIATE
        ));
        assert_eq!(player.get_wait_status().bits(), 3);
        assert_eq!(player.position(), Ticks::ZERO);
        assert_eq!(player.stopping.len(), 1);
        while player.pending.is_some() {
            player.update();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(player.get_wait_status().is_empty());
        assert!(player.current_id.is_none());
        assert!(player.request(
            server.clone(),
            path.clone(),
            id,
            true,
            Volume::default(),
            fade
        ));
        player.stop(Tween::IMMEDIATE);
        player.update();
        assert!(player.pending.is_none());
        assert!(player.current_bgm.is_none());
        assert!(player.get_wait_status().is_empty());
        // Ready restoration must supersede a pending command without waiting.
        assert!(player.request(
            server.clone(),
            "/missing-restored-bgm.nxa".into(),
            replacement,
            true,
            Volume::default(),
            fade
        ));
        let bgm: Arc<AudioFile> = server.load_sync(&path).unwrap();
        assert!(player.play(id, bgm, "", true, Volume::default(), Tween::IMMEDIATE));
        assert!(player.pending.is_none());
        assert_eq!(player.get_wait_status(), AudioWaitStatus::PLAYING);
        player.stop(Tween::IMMEDIATE);
        assert!(player.get_wait_status().is_empty());
        eprintln!(
            "Async BGM: pending/reuse/volume/loop policy/failed replacement/stop/ready restoration pass"
        );
    }

    pub fn set_settings_volume(&mut self, percent: u8) {
        self.settings_gain.apply(&mut self.bgm_track, percent);
    }

    pub fn update_voice_focus(&mut self, delta: Ticks, enabled: bool, voice_active: bool) {
        self.voice_focus
            .update_track(&mut self.focus_track, delta, enabled, voice_active);
    }

    /// Native music requests return immediately and reuse an active ID even
    /// during loading, retaining its original loop policy.
    pub fn request(
        &mut self,
        server: Arc<AssetServer>,
        path: String,
        bgm_id: BgmId,
        repeat: bool,
        volume: Volume,
        fade: Tween,
    ) -> bool {
        self.request_with_loop_count(
            server,
            path,
            bgm_id,
            if repeat { 0 } else { 1 },
            volume,
            fade,
        )
    }

    /// Retain the native total-pass count, including the final untrimmed tail.
    /// Matching active/loading IDs retain their original count.
    pub fn request_with_loop_count(
        &mut self,
        server: Arc<AssetServer>,
        path: String,
        bgm_id: BgmId,
        loop_count: u32,
        volume: Volume,
        fade: Tween,
    ) -> bool {
        if self.current_id == Some(bgm_id) {
            if let Some(pending) = &mut self.pending {
                pending.volume.enqueue_now(volume.0, fade);
                return false;
            }
        }
        if retarget_matching_bgm(&mut self.current_bgm, self.current_id, bgm_id, volume, fade) {
            return false;
        }
        stop_bgm(&mut self.current_bgm, &mut self.stopping, fade);
        self.current_id = Some(bgm_id);
        self.pending = Some(PendingBgm {
            task: shin_tasks::async_io::spawn(async move { server.load(&path).await }),
            loop_count,
            volume: pending_volume(volume, fade),
        });
        true
    }

    pub fn play(
        &mut self,
        bgm_id: BgmId,
        bgm: Arc<AudioFile>,
        _display_name: &str,
        repeat: bool,
        volume: Volume,
        fade_in: Tween,
    ) -> bool {
        if self.current_id == Some(bgm_id) {
            if let Some(pending) = &mut self.pending {
                pending.volume.enqueue_now(volume.0, fade_in);
                return false;
            }
        }
        self.pending = None;
        if retarget_matching_bgm(
            &mut self.current_bgm,
            self.current_id,
            bgm_id,
            volume,
            fade_in,
        ) {
            return false;
        }
        stop_bgm(&mut self.current_bgm, &mut self.stopping, fade_in);
        let loop_start = repeat.then_some(bgm.info().loop_start);
        let kira_data = match AudioData::from_audio_file(
            bgm,
            AudioSettings {
                paused: self.paused,
                playback_rate: 1.0,

                fade_in,
                loop_start,
                loop_count: 0,
                volume,
                pan: Pan::default(),
            },
        ) {
            Ok(data) => data,
            Err(error) => {
                warn!(%error, "Failed to prepare BGM");
                self.current_id = None;
                return false;
            }
        };

        let handle = self
            .audio_manager
            .play_on_track(&mut self.focus_track, kira_data);

        self.current_bgm = Some(handle);
        self.current_id = Some(bgm_id);
        true
    }

    pub fn restore_empty(&mut self) {
        if self.current_bgm.is_some() || self.pending.is_some() {
            self.stop(Tween::IMMEDIATE);
        } else {
            self.current_id = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn reference_source(&self) -> (Option<BgmId>, u32, u64) {
        (
            self.current_id,
            self.current_bgm
                .as_ref()
                .map_or(0, AudioHandle::position_millis),
            self.focus_track.id(),
        )
    }
    #[cfg(test)]
    pub(crate) fn reference_pause_source(&self, paused: bool) {
        if let Some(handle) = &self.current_bgm {
            handle.set_paused(paused);
        }
    }

    pub fn set_volume(&mut self, volume: Volume, tween: Tween) {
        if let Some(pending) = &mut self.pending {
            pending.volume.enqueue_now(volume.0, tween);
        } else if let Some(handle) = self.current_bgm.as_mut() {
            handle.set_volume(volume, tween).unwrap();
        } else {
            warn!("Tried to set volume of BGM, but no BGM is currently playing");
        }
    }

    /// Older native32d28 appends to the active/loading volume trajectory.
    /// Keep the existing Umineko setter's replacement behavior separate.
    pub(crate) fn enqueue_volume(&mut self, volume: Volume, tween: Tween) -> anyhow::Result<()> {
        if let Some(pending) = &mut self.pending {
            pending.volume.enqueue(volume.0, tween);
        } else if let Some(handle) = self.current_bgm.as_mut() {
            handle.enqueue_volume(volume, tween)?;
        }
        Ok(())
    }

    pub(crate) fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        for handle in self.current_bgm.iter().chain(&self.stopping) {
            handle.set_paused(paused);
        }
    }

    pub(crate) fn sample_clock(&self) -> Option<(u64, u32)> {
        self.current_bgm
            .as_ref()
            .map(|h| (u64::from(h.position_samples()), h.sample_rate()))
    }

    pub fn stop(&mut self, fade_out: Tween) {
        self.current_id = None;
        let was_pending = self.pending.take().is_some();
        if !stop_bgm(&mut self.current_bgm, &mut self.stopping, fade_out) && !was_pending {
            warn!("Tried to stop BGM, but no BGM is currently playing");
        }
    }

    pub fn update(&mut self) {
        if let Some(result) = self
            .pending
            .as_mut()
            .and_then(|pending| pending.task.poll_naive())
        {
            let pending = self.pending.take().unwrap();
            let result = result.and_then(|bgm| {
                let loop_start = Some(bgm.info().loop_start);
                AudioData::from_audio_file(
                    bgm,
                    AudioSettings {
                        paused: self.paused,
                        playback_rate: 1.0,

                        fade_in: Tween::IMMEDIATE,
                        loop_start,
                        loop_count: pending.loop_count,
                        volume: Volume(pending.volume.value()),
                        pan: Pan::default(),
                    },
                )
            });
            match result {
                Ok(data) => {
                    let data = data.with_tweens(AudioTweens {
                        volume: pending.volume,
                        panning: Tweener::new(0.0),
                        playback_rate: Tweener::new(1.0),
                    });
                    self.current_bgm = Some(
                        self.audio_manager
                            .play_on_track(&mut self.focus_track, data),
                    );
                }
                Err(error) => {
                    warn!(%error, "Failed to prepare BGM");
                    self.current_id = None;
                }
            }
        }
        self.stopping
            .retain(|handle| !handle.get_wait_status().is_empty());
    }

    pub fn get_wait_status(&self) -> AudioWaitStatus {
        if let Some(pending) = &self.pending {
            return pending_status(&pending.volume);
        }
        self.current_bgm
            .as_ref()
            .map_or(AudioWaitStatus::empty(), AudioHandle::get_wait_status)
    }

    pub fn position(&self) -> Ticks {
        self.current_bgm
            .as_ref()
            .map_or(Ticks::ZERO, AudioHandle::position)
    }

    pub fn position_micros(&self) -> u64 {
        self.current_bgm
            .as_ref()
            .map_or(0, AudioHandle::position_micros)
    }
}

impl Drop for BgmPlayer {
    fn drop(&mut self) {
        for handle in self.current_bgm.iter_mut().chain(&mut self.stopping) {
            let _ = handle.stop(Tween::IMMEDIATE);
        }
    }
}

fn retarget_matching_bgm(
    handle: &mut Option<AudioHandle>,
    current: Option<BgmId>,
    requested: BgmId,
    volume: Volume,
    tween: Tween,
) -> bool {
    if current != Some(requested) {
        return false;
    }
    let Some(handle) = handle.as_mut() else {
        return false;
    };
    if handle.get_wait_status().is_empty() {
        return false;
    }
    handle.set_volume(volume, tween).unwrap();
    true
}

fn stop_bgm(handle: &mut Option<AudioHandle>, tails: &mut Vec<AudioHandle>, tween: Tween) -> bool {
    let Some(mut handle) = handle.take() else {
        return false;
    };
    handle
        .stop(Tween::linear(tween.duration.max(Ticks::from_f32(4.0))))
        .unwrap();
    tails.push(handle);
    true
}

// Track-title drawing is handled by adv::bgm_title.

#[cfg(test)]
mod tests {
    use super::*;
    use kira::{AudioManager as KiraManager, AudioManagerSettings};
    use shin_audio::test_support::{MockBackendSettings, SampleBackend as MockBackend};
    use shin_core::format::{
        audio::{AudioBuffer, AudioFrameSource},
        scenario::instruction_elements::FromNumber,
    };
    struct Tone(u32);
    impl AudioFrameSource for Tone {
        fn max_frame_size(&self) -> usize {
            32
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
            if self.0 >= 32 {
                return false;
            }
            for _ in 0..32 {
                buffer.push((0.1, 0.1));
            }
            self.0 += 32;
            true
        }
        fn samples_seek(&mut self, position: u32) -> anyhow::Result<u32> {
            self.0 = position;
            Ok(0)
        }
        fn current_sample_position(&self) -> u32 {
            self.0
        }
    }
    fn advance(manager: &mut KiraManager<MockBackend>, count: u32) -> kira::Frame {
        let backend = manager.backend_mut();
        let mut frame = kira::Frame::ZERO;
        for _ in 0..count {
            backend.on_start_processing();
            frame = backend.process();
        }
        frame
    }
    #[test]
    fn pending_bgm_fade_and_volume_retarget_transfer_to_first_pcm() {
        let mut volume = pending_volume(Volume(0.8), Tween::linear(Ticks::from_f32(12.0)));
        assert_eq!(pending_status(&volume).bits(), 7);
        volume.enqueue_now(0.4, Tween::linear(Ticks::from_f32(6.0)));
        assert_eq!(volume.value(), 0.0);
        let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let handle = manager
            .play(
                AudioData {
                    source: Tone(0),
                    settings: AudioSettings {
                        paused: false,
                        playback_rate: 1.0,
                        fade_in: Tween::IMMEDIATE,
                        loop_start: Some(0),
                        loop_count: 0,
                        volume: Volume::default(),
                        pan: Pan::default(),
                    },
                }
                .with_tweens(AudioTweens {
                    volume,
                    panning: Tweener::new(0.0),
                    playback_rate: Tweener::new(1.0),
                }),
            )
            .unwrap();
        assert_eq!(handle.get_wait_status().bits(), 6);
        let halfway = advance(&mut manager, 50);
        assert!((halfway.left - 0.02).abs() < 0.001);
        let complete = advance(&mut manager, 60);
        assert!((complete.left - 0.04).abs() < 0.00001);
        assert_eq!(handle.get_wait_status(), AudioWaitStatus::PLAYING);
    }

    #[test]
    fn native_bgm_reuse_preserves_position_and_stop_releases_active_entry() {
        let mut manager = KiraManager::<MockBackend>::new(AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 1000 },
            internal_buffer_size: 1,
            ..Default::default()
        })
        .unwrap();
        let mut handle = Some(
            manager
                .play(AudioData {
                    source: Tone(0),
                    settings: AudioSettings {
                        paused: false,
                        playback_rate: 1.0,
                        fade_in: Tween::IMMEDIATE,
                        loop_start: Some(0),
                        loop_count: 0,
                        volume: Volume(1.0),
                        pan: Pan(0.0),
                    },
                })
                .unwrap(),
        );
        let id = BgmId::from_number(1);
        advance(&mut manager, 12);
        let before = handle.as_ref().unwrap().position_millis();
        let tween = Tween::linear(Ticks::from_millis(10.0));
        assert!(!retarget_matching_bgm(
            &mut handle,
            Some(id),
            BgmId::from_number(2),
            Volume(0.5),
            tween
        ));
        assert!(retarget_matching_bgm(
            &mut handle,
            Some(id),
            id,
            Volume(0.5),
            tween
        ));
        advance(&mut manager, 5);
        assert!(handle.as_ref().unwrap().position_millis() > before);
        assert!(
            handle
                .as_ref()
                .unwrap()
                .get_wait_status()
                .contains(AudioWaitStatus::VOLUME_TWEENING)
        );
        assert!((advance(&mut manager, 12).left - 0.05).abs() < 0.00001);
        let mut tails = Vec::new();
        assert!(stop_bgm(&mut handle, &mut tails, Tween::IMMEDIATE));
        assert!(handle.is_none());
        assert_eq!(tails.len(), 1);
        assert!(!stop_bgm(&mut handle, &mut tails, Tween::IMMEDIATE));
        assert!(advance(&mut manager, 60).left > 0.0);
        advance(&mut manager, 10);
        assert!(tails[0].get_wait_status().is_empty());
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_bgm_identity_retarget_stop_and_wait() {
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
            (0x33d08, 0x97ffd22a),
            (0x33d14, 0x320003e1),
            (0x33d18, 0x2a1403e2),
            (0x33d20, 0x9402ff58),
            (0x33a14, 0x2900feb3),
            (0x33a18, 0x390002a8),
            (0x33a28, 0x9403048e),
            (0x33a34, 0x94021e2b),
            (0x33a48, 0x94021fee),
            (0x33e4c, 0xb9400948),
            (0x33e50, 0x7100011f),
            (0x33e54, 0x321f03e8),
            (0x33e5c, 0x1a881513),
            (0x33f74, 0xb9400ac8),
            (0x33f78, 0x340000e8),
            (0x33f9c, 0x940302fd),
            (0x33fa0, 0x340000a0),
            (0x33fa8, 0x940302be),
            (0x33fac, 0xb9000ad8),
            (0x33fcc, 0x94021ea1),
            (0x33ddc, 0x321803e1),
            (0x33de4, 0x94021f07),
            (0x33b38, 0x9402779e),
            (0x33b48, 0x9402779a),
            (0x6c3c, 0x9400b2f1),
            (0x6c40, 0x360000a0),
            (0x6c50, 0x94025d6c),
            (0x33858, 0x3940012a),
            (0x33864, 0x6b13015f),
            (0x33868, 0x54000760),
            (0x33880, 0x710012ff),
            (0x33888, 0x1a89c2e9),
            (0x338a8, 0x94021ede),
            (0x338bc, 0x94022051),
            (0x338c4, 0x3900035f),
            (0x33958, 0x94021eb2),
            (0x3395c, 0x1e2202e1),
            (0x3396c, 0x94022025),
            (0x33a34, 0x94021e2b),
            (0x33a48, 0x94021fee),
            (0x33c50, 0x7100103f),
            (0x33c58, 0x1a89c029),
            (0x33c78, 0x94021dea),
            (0x33c8c, 0x94021f5d),
            (0x33c94, 0x390002bf),
            (0x33e20, 0x3940014b),
            (0x33e24, 0x350000eb),
            (0x33e60, 0x94021ee4),
            (0x33e68, 0x321e0268),
            (0x33e70, 0x1a931100),
            (0x33e90, 0x3940014b),
            (0x33e94, 0x350000ab),
        ] {
            assert_eq!(word(a), expected, "native BGM{a:x}");
        }
    }
}
