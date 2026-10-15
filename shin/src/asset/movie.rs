use std::sync::Arc;

use anyhow::{Context, Result};
use shin_audio::AudioManager;
use shin_core::vm::command::types::Volume;
use shin_video::{VideoPlayerHandle, mp4::Mp4};

use crate::asset::system::{Asset, AssetDataAccessor, AssetDataCursor, AssetLoadContext};

pub struct Movie {
    label: String,
    // TODO: allow to start decoding the video before the first frame is requested
    mp4: Mp4<AssetDataCursor>,
}

impl Asset for Movie {
    type Args = ();

    async fn load(
        _context: &Arc<AssetLoadContext>,
        _args: (),
        name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        let cursor = data.cursor()?;
        let mp4 = Mp4::new(cursor).context("Reading Mp4")?;
        Ok(Self {
            label: name.to_string(),
            mp4,
        })
    }
}

impl Movie {
    /// Updated Higurashi initializes its own transport paused; the live owner
    /// starts it only after publishing preparation readiness.
    pub(crate) fn prepare_higurashi_with_volume(
        &self,
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        volume: Volume,
        active_clock: Option<Arc<dyn Fn() -> u64 + Send + Sync>>,
    ) -> Result<shin_video::higurashi::HigurashiVideoPlayerHandle> {
        match active_clock {
            Some(clock) => shin_video::higurashi::HigurashiVideoPlayerHandle::new_paused_with_clock(
                device, audio_manager, self.mp4.clone(), volume, clock,
            ),
            None => shin_video::higurashi::HigurashiVideoPlayerHandle::new_paused_with_volume(
                device, audio_manager, self.mp4.clone(), volume,
            ),
        }
    }
    pub fn prepare_with_volume(
        &self,
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        volume: Volume,
    ) -> Result<VideoPlayerHandle> {
        VideoPlayerHandle::new_paused_with_volume(device, audio_manager, self.mp4.clone(), volume)
    }
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn play(
        &self,
        device: &wgpu::Device,
        audio_manager: &AudioManager,
    ) -> Result<VideoPlayerHandle> {
        VideoPlayerHandle::new(device, audio_manager, self.mp4.clone())
    }

    pub fn prepare_looping_with_volume(
        &self,
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        volume: Volume,
    ) -> Result<VideoPlayerHandle> {
        VideoPlayerHandle::new_looping_with_volume(
            device,
            audio_manager,
            self.mp4.clone(),
            volume,
            true,
        )
    }

    pub fn play_looping_with_volume(
        &self,
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        volume: Volume,
    ) -> Result<VideoPlayerHandle> {
        VideoPlayerHandle::new_looping_with_volume(
            device,
            audio_manager,
            self.mp4.clone(),
            volume,
            false,
        )
    }

    pub fn play_with_volume(
        &self,
        device: &wgpu::Device,
        audio_manager: &AudioManager,
        volume: Volume,
    ) -> Result<VideoPlayerHandle> {
        VideoPlayerHandle::new_with_volume(device, audio_manager, self.mp4.clone(), volume)
    }
}

#[cfg(all(test, target_os = "macos"))]
mod worker_failure_reference;
