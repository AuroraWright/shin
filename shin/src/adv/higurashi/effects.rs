//! Dynamic signed SE entries over the shared decoder, mixer and media handles.
use super::effect_commands::EffectPlay;
use crate::{
    asset::system::AssetServer,
    audio::{SettingsGain, VoiceFocus},
};
use anyhow::Result;
use kira::track::TrackBuilder;
use shin_audio::AudioTrack as TrackHandle;
use shin_audio::{AudioData, AudioFile, AudioHandle, AudioManager, AudioSettings, AudioTweens};
use shin_core::{
    time::{Ticks, Tween, Tweener},
    vm::command::types::{Pan, Volume},
};
use shin_tasks::AsyncTask;
use std::sync::Arc;

enum Source {
    Loading {
        task: AsyncTask<Result<Arc<AudioFile>>>,
        tweens: AudioTweens,
    },
    Playing(AudioHandle),
    Stopping(AudioHandle),
}
struct Entry {
    slot: i32,
    id: i32,
    count: u32,
    source: Source,
    // Repeating the native idempotent finish needs no additional audio command
    // until authored parameters or a new stop fade change the source again.
    finish_requested: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkipOperation {
    DropLoad,
    Stop,
    FinishParameters,
    Keep,
}
pub(crate) fn skip_operation(count: u32, state: u32) -> SkipOperation {
    if (count as i32) >= 1 {
        match state {
            0 => SkipOperation::DropLoad,
            1 => SkipOperation::Stop,
            _ => SkipOperation::Keep,
        }
    } else if state <= 2 {
        SkipOperation::FinishParameters
    } else {
        SkipOperation::Keep
    }
}
pub struct HigurashiEffects {
    manager: Arc<AudioManager>,
    entries: Vec<Entry>,
    track: TrackHandle,
    gain: SettingsGain,
    focus_track: TrackHandle,
    focus: VoiceFocus,
    paused: bool,
}
fn fade(time: i32) -> Tween {
    Tween::linear(Ticks::from_f32(time as f32))
}
fn volume(value: f32) -> Volume {
    Volume(if value > 0.0 { value.min(1.0) } else { 0.0 })
}
fn pan(value: f32) -> Pan {
    Pan(if value > -1.0 { value.min(1.0) } else { -1.0 })
}
fn speed(value: f32) -> f32 {
    if value > 0.01 { value.min(10.0) } else { 0.01 }
}
fn loading_status(tweens: &AudioTweens) -> i32 {
    let mut status = 3;
    for (tween, bit) in [
        (&tweens.volume, 4),
        (&tweens.panning, 8),
        (&tweens.playback_rate, 16),
    ] {
        if !tween.is_idle() {
            status |= bit;
        }
    }
    status
}
impl HigurashiEffects {
    pub fn new(manager: Arc<AudioManager>) -> Self {
        let mut mixer = manager.kira_manager().lock();
        let mut track: TrackHandle = mixer
            .add_sub_track(TrackBuilder::new())
            .expect("SE category track")
            .into();
        let focus_track = track
            .add_sub_track(TrackBuilder::new())
            .expect("Higurashi SE voice focus track")
            .into();
        drop(mixer);
        Self {
            manager,
            entries: Vec::new(),
            track,
            gain: SettingsGain::default(),
            focus_track,
            focus: VoiceFocus::default(),
            paused: false,
        }
    }
    pub fn set_volume(&mut self, percent: u8) {
        self.gain.apply(&mut self.track, percent);
    }
    pub fn update_voice_focus(&mut self, delta: Ticks, active: bool) {
        self.focus
            .update_track(&mut self.focus_track, delta, true, active);
    }
    #[cfg(test)]
    pub(crate) fn focus_gain(&self) -> f32 {
        self.focus.gain
    }
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        for entry in &self.entries {
            if let Source::Playing(handle) | Source::Stopping(handle) = &entry.source {
                handle.set_paused(paused);
            }
        }
    }
    pub fn request(
        &mut self,
        assets: Arc<AssetServer>,
        path: Option<String>,
        request: EffectPlay,
    ) -> Result<()> {
        let (v, p, s, tween) = (
            volume(request.volume),
            pan(request.pan),
            speed(request.speed),
            fade(request.fade),
        );
        self.retire_finished();
        // Nativec3304 searches the whole vector, including stopping entries.
        // Only two infinite requests with matching slot/ID may reuse a source.
        if let Some(index) = self.entries.iter().position(|e| {
            e.slot == request.slot && e.id == request.id && e.count == 0 && request.count == 0
        }) {
            let entry = &mut self.entries[index];
            match &mut entry.source {
                Source::Loading { tweens, .. } => {
                    tweens.volume.enqueue_now(v.0, tween);
                    tweens.panning.enqueue_now(p.0, tween);
                    tweens.playback_rate.enqueue_now(s, tween);
                }
                Source::Playing(handle) => {
                    handle.set_parameters(v, p, s, tween)?;
                }
                Source::Stopping(handle) => {
                    handle.resume_parameters(v, p, s, tween)?;
                }
            }
            entry.finish_requested = false;
            if matches!(entry.source, Source::Stopping(_)) {
                let entry = self.entries.remove(index);
                let Source::Stopping(handle) = entry.source else {
                    unreachable!()
                };
                self.entries.insert(
                    index,
                    Entry {
                        source: Source::Playing(handle),
                        ..entry
                    },
                );
            }
            return Ok(());
        }
        self.stop(Some(request.slot), 0)?;
        // Invalid metadata retires the previous entry before lookup fails.
        let Some(path) = path else {
            return Ok(());
        };
        let task = shin_tasks::async_io::spawn(async move { assets.load(&path).await });
        self.entries.push(Entry {
            slot: request.slot,
            id: request.id,
            count: request.count,
            finish_requested: false,
            source: Source::Loading {
                task,
                tweens: initial_tweens(v, p, s, tween),
            },
        });
        Ok(())
    }
    pub fn stop(&mut self, slot: Option<i32>, time: i32) -> Result<()> {
        // Remove loaders immediately. Stop tails remain reusable but no longer
        // participate in exact or aggregate waits.
        let mut index = 0;
        while index < self.entries.len() {
            let entry = &mut self.entries[index];
            if slot.is_some_and(|s| s != entry.slot) {
                index += 1;
                continue;
            }
            match &mut entry.source {
                Source::Loading { .. } => {
                    self.entries.remove(index);
                    continue;
                }
                Source::Playing(handle) => {
                    handle.finish_volume_and_stop(fade(time.max(4)))?;
                }
                Source::Stopping(_) => {
                    index += 1;
                    continue;
                }
            }
            // Swap entry ownership without constructing a replacement worker.
            let entry = self.entries.remove(index);
            let Source::Playing(handle) = entry.source else {
                unreachable!()
            };
            self.entries.insert(
                index,
                Entry {
                    source: Source::Stopping(handle),
                    finish_requested: false,
                    ..entry
                },
            );
            index += 1;
        }
        Ok(())
    }
    pub fn queue_volume(&mut self, slot: i32, value: f32, time: i32) -> Result<()> {
        if let Some(entry) = self.active_mut(slot) {
            match &mut entry.source {
                Source::Loading { tweens, .. } => {
                    tweens.volume.enqueue(volume(value).0, fade(time));
                }
                Source::Playing(handle) => {
                    handle.enqueue_volume(volume(value), fade(time))?;
                }
                Source::Stopping(_) => unreachable!(),
            }
            entry.finish_requested = false;
        }
        Ok(())
    }
    /// Nativec312c leaves loaders/state0 and playing/state1 unchanged. Only
    /// stopping/state2 tails finish their target and restart a four-tick fade.
    pub fn prepare_reconstruction(&mut self) -> Result<()> {
        for entry in &mut self.entries {
            if let Source::Stopping(handle) = &mut entry.source {
                handle.finish_volume_and_stop(fade(4))?;
                entry.finish_requested = false;
            }
        }
        Ok(())
    }
    /// Original c3cc8 removes finite pending loads, fades finite sources for
    /// four ticks and finishes only volume/pan for infinite sources and tails.
    pub fn fast_forward(&mut self) -> Result<()> {
        let mut index = 0;
        while index < self.entries.len() {
            let entry = &mut self.entries[index];
            let state = match entry.source {
                Source::Loading { .. } => 0,
                Source::Playing(_) => 1,
                Source::Stopping(_) => 2,
            };
            let operation = skip_operation(entry.count, state);
            if matches!(operation, SkipOperation::DropLoad | SkipOperation::Stop) {
                match &mut entry.source {
                    Source::Loading { .. } => {
                        self.entries.remove(index);
                        continue;
                    }
                    Source::Playing(handle) => {
                        handle.finish_volume_and_stop(fade(4))?;
                        let entry = self.entries.remove(index);
                        let Source::Playing(handle) = entry.source else {
                            unreachable!()
                        };
                        self.entries.insert(
                            index,
                            Entry {
                                source: Source::Stopping(handle),
                                finish_requested: false,
                                ..entry
                            },
                        );
                    }
                    Source::Stopping(_) => {}
                }
            } else if operation == SkipOperation::FinishParameters {
                match &mut entry.source {
                    Source::Loading { tweens, .. } => {
                        tweens.volume.fast_forward();
                        tweens.panning.fast_forward();
                    }
                    Source::Playing(handle) | Source::Stopping(handle) => {
                        if !entry.finish_requested {
                            handle.finish_volume_and_pan()?;
                            entry.finish_requested = true;
                        }
                    }
                }
            }
            index += 1;
        }
        Ok(())
    }
    pub fn queue_pan(&mut self, slot: i32, value: f32, time: i32) -> Result<()> {
        if let Some(entry) = self.active_mut(slot) {
            match &mut entry.source {
                Source::Loading { tweens, .. } => {
                    tweens.panning.enqueue(pan(value).0, fade(time));
                }
                Source::Playing(handle) => {
                    handle.enqueue_panning(pan(value), fade(time))?;
                }
                Source::Stopping(_) => unreachable!(),
            }
            entry.finish_requested = false;
        }
        Ok(())
    }
    fn active_mut(&mut self, slot: i32) -> Option<&mut Entry> {
        self.entries
            .iter_mut()
            .find(|e| e.slot == slot && !matches!(e.source, Source::Stopping(_)))
    }
    pub fn status(&self, slot: i32) -> i32 {
        self.entries
            .iter()
            .find(|e| e.slot == slot && !matches!(e.source, Source::Stopping(_)))
            .map_or(0, |e| match &e.source {
                Source::Loading { tweens, .. } => loading_status(tweens),
                Source::Playing(handle) => handle.get_wait_status().bits(),
                Source::Stopping(_) => unreachable!(),
            })
    }
    pub fn active_slots(&self) -> Vec<i32> {
        self.entries
            .iter()
            .filter(|e| !matches!(e.source, Source::Stopping(_)))
            .map(|e| e.slot)
            .collect()
    }
    fn retire_finished(&mut self) {
        self.entries.retain(|e| match &e.source {
            Source::Loading { .. } => true,
            Source::Playing(h) | Source::Stopping(h) => !h.get_wait_status().is_empty(),
        });
    }
    pub fn update(&mut self) {
        let mut index = 0;
        while index < self.entries.len() {
            let entry = &mut self.entries[index];
            let result = match &mut entry.source {
                Source::Loading { task, .. } => task.poll_naive(),
                _ => None,
            };
            if let Some(result) = result {
                let entry = self.entries.remove(index);
                let Source::Loading { tweens, .. } = entry.source else {
                    unreachable!()
                };
                let result = result.and_then(|file| {
                    let loop_start = Some(file.info().loop_start);
                    AudioData::from_audio_file(
                        file,
                        AudioSettings {
                            paused: self.paused,
                            fade_in: Tween::IMMEDIATE,
                            loop_start,
                            loop_count: entry.count,
                            volume: Volume(tweens.volume.value()),
                            pan: Pan(tweens.panning.value()),
                            playback_rate: tweens.playback_rate.value(),
                        },
                    )
                    .map(|data| data.with_tweens(tweens))
                });
                match result {
                    Ok(data) => {
                        self.entries.insert(
                            index,
                            Entry {
                                source: Source::Playing(
                                    self.manager.play_on_track(&mut self.focus_track, data),
                                ),
                                ..entry
                            },
                        );
                    }
                    Err(error) => {
                        tracing::warn!(%error, slot=entry.slot, "Failed to prepare Higurashi SE");
                        continue;
                    }
                }
            }
            index += 1;
        }
        self.retire_finished();
    }
    #[cfg(test)]
    pub(crate) fn position(&self, slot: i32) -> Option<u32> {
        self.entries
            .iter()
            .find(|e| e.slot == slot)
            .and_then(|e| match &e.source {
                Source::Playing(h) | Source::Stopping(h) => Some(h.position_samples()),
                _ => None,
            })
    }
}
fn initial_tweens(v: Volume, p: Pan, speed: f32, tween: Tween) -> AudioTweens {
    let mut volume = Tweener::new(0.0);
    volume.enqueue(v.0, tween);
    AudioTweens {
        volume,
        panning: Tweener::new(p.0),
        playback_rate: Tweener::new(speed),
    }
}
impl Drop for HigurashiEffects {
    fn drop(&mut self) {
        for entry in &self.entries {
            if let Source::Playing(h) | Source::Stopping(h) = &entry.source {
                h.stop_immediate();
            }
        }
    }
}
