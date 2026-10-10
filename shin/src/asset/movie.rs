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
        let cursor = data.cursor();
        let mp4 = Mp4::new(cursor).context("Reading Mp4")?;
        Ok(Self {
            label: name.to_string(),
            mp4,
        })
    }
}

impl Movie {
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
