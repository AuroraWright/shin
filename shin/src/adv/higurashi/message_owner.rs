//! MSGINIT/SET/WAIT/SIGNAL/SYNC/CLOSE/FACE dispatcher and retained message owner.
//! Voice transport and composed page properties are supplied separately.
use super::{
    message_glyphs::HigurashiMessageGlyphs,
    message_playback::{HigurashiMessagePlayback, MessageSettings, MessageVoice},
    message_ui::{HigurashiMessageTextures, MessageUiSettings, message_ui_draws},
    scene::CommandStatus,
};
use crate::{
    asset::{font::GpuFontLazy, system::AssetServer},
    layer::render_params::higurashi::HigurashiTransform,
};
use anyhow::{Result, ensure};
use shin_core::{time::Ticks, vm::higurashi::command::HigurashiRuntimeCommand};
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

enum Pending {
    Wait(i32),
    VoiceSync(i32, i32),
    Close,
}

pub struct HigurashiMessageOwner {
    pub playback: HigurashiMessagePlayback,
    pub settings: MessageSettings,
    pub ui_settings: MessageUiSettings,
    font: Arc<GpuFontLazy>,
    textures: Arc<HigurashiMessageTextures>,
    speaker_width: f32,
    glyphs: Option<HigurashiMessageGlyphs>,
    init_style: i32,
    init_options: [i32; 2],
    pending: Option<Pending>,
}

impl HigurashiMessageOwner {
    pub(super) fn prepare_reconstruction(
        &mut self,
        descriptor: &super::descriptors::MessageDescriptor,
        voice: &mut impl MessageVoice,
    ) {
        self.playback.prepare_reconstruction(voice);
        // These caches represent VM8c84..8c8c, not the existing glyph style.
        self.init_style = descriptor.style;
        self.init_options = descriptor.options;
        self.pending = None;
    }
    /// Native820c4 clears displayed text and restores the window reset fields.
    /// Font/texture resources and MSGINIT caches remain owned.
    pub(super) fn clear_reconstruction_message(&mut self) {
        self.playback.close(false);
        self.playback.hidden = false;
        self.playback.modal_slide = 1.0;
        self.playback.height = 357.0;
        self.playback.height_target = 357.0;
        self.playback.current_time = 0.0;
        self.playback.sections = 0;
        self.glyphs = None;
    }
    pub async fn load(assets: &AssetServer) -> Result<Self> {
        Ok(Self::from_common_resources(
            HigurashiMessageGlyphs::load_font(assets).await?,
            HigurashiMessageTextures::load(assets).await?,
        ))
    }
    pub(crate) fn from_common_resources(
        font: Arc<GpuFontLazy>,
        textures: Arc<HigurashiMessageTextures>,
    ) -> Self {
        Self {
            playback: Default::default(),
            settings: Default::default(),
            ui_settings: Default::default(),
            font,
            textures,
            speaker_width: 0.0,
            glyphs: None,
            init_style: 0,
            init_options: [0; 2],
            pending: None,
        }
    }
    pub fn glyphs(&self) -> Option<&HigurashiMessageGlyphs> {
        self.glyphs.as_ref()
    }
    pub fn command_pending(&self) -> bool {
        self.pending.is_some()
    }
    ///839ec(message,1):close with its existing slide,independent of skip.
    pub(crate) fn close_for_exit(&mut self) -> Result<()> {
        ensure!(
            self.pending.is_none(),
            "EXIT cannot replace a pending message command"
        );
        self.playback.close(true);
        self.glyphs = None;
        Ok(())
    }

    /// Read/ignore-input flags come from the ADV read/skip policy. The command's
    /// auto_wait controls command ownership, independently of slide animation.
    pub fn dispatch(
        &mut self,
        command: &HigurashiRuntimeCommand,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fast_skip: bool,
        read_flags: u32,
        text_speed: i32,
        outline_mode: u8,
        voice: &impl MessageVoice,
    ) -> Result<Option<CommandStatus>> {
        ensure!(
            self.pending.is_none(),
            "Higurashi message command is still pending"
        );
        match command {
            HigurashiRuntimeCommand::MSGINIT {
                style,
                updated_options,
            } => {
                self.init_style = *style;
                self.init_options = updated_options.unwrap_or([0; 2]);
            }
            HigurashiRuntimeCommand::MSGSET {
                msg_id,
                auto_wait,
                argument,
                text,
            } => {
                ensure!(read_flags & !3 == 0, "Unsupported Higurashi message flags");
                let glyphs = HigurashiMessageGlyphs::prepare(
                    device,
                    queue,
                    self.font.clone(),
                    self.init_style,
                    self.init_options,
                    text_speed,
                    outline_mode,
                    text,
                );
                self.playback.install(
                    self.init_style,
                    self.init_options,
                    read_flags,
                    *argument,
                    super::read_state::native_message_id(msg_id.0),
                    !fast_skip,
                );
                self.playback.set_glyphs(&glyphs);
                self.speaker_width = glyphs.lines.first().map_or(0.0, |line| line.width);
                self.glyphs = Some(glyphs);
                if *auto_wait != 0 {
                    self.pending = Some(Pending::Wait(-1));
                }
            }
            HigurashiRuntimeCommand::MSGWAIT { signal } => {
                self.pending = Some(Pending::Wait(*signal))
            }
            HigurashiRuntimeCommand::MSGSIGNAL {} => self.playback.send_sync(),
            HigurashiRuntimeCommand::MSGSYNC {
                args: [index, time],
            } => self.pending = Some(Pending::VoiceSync(*index, *time)),
            HigurashiRuntimeCommand::MSGCLOSE { wait } => {
                self.playback.close(!fast_skip);
                self.glyphs = None;
                if *wait != 0 {
                    self.pending = Some(Pending::Close);
                }
            }
            HigurashiRuntimeCommand::MSGFACE { original_argument } => {
                // Original MSGFACE forwards its argument to 7fd34, a RET.
                // Updated opcode8b is MSGHIDE and calls 83a00 instead.
                if original_argument.is_none() {
                    self.playback.block_input();
                }
            }
            _ => return Ok(None),
        }
        Ok(Some(self.poll_command(voice)))
    }

    pub fn poll_command(&mut self, voice: &impl MessageVoice) -> CommandStatus {
        let waiting = match self.pending {
            Some(Pending::Wait(section)) => self.playback.waiting(section),
            Some(Pending::VoiceSync(index, time)) => {
                self.playback.voice_waiting(index, time, voice)
            }
            Some(Pending::Close) => self.playback.busy(),
            None => false,
        };
        if waiting {
            CommandStatus::Pending
        } else {
            self.pending = None;
            CommandStatus::Complete
        }
    }

    pub fn advance(
        &mut self,
        ticks: Ticks,
        autoplay: bool,
        skip_request: bool,
        voice: &mut impl MessageVoice,
    ) {
        self.playback
            .advance(ticks.as_f32(), autoplay, skip_request, self.settings, voice);
    }
    pub fn click(&mut self, voice: &mut impl MessageVoice) -> bool {
        self.playback.click(self.settings, voice)
    }
    pub fn fast_forward(&mut self, voice: &mut impl MessageVoice) {
        self.playback.fast_forward(voice);
    }

    /// Caller supplies the fully composed message owner transform. Windows and
    /// cursors precede the live border/glyph passes, as in updated84f6c.
    pub fn render(
        &self,
        pass: &mut RenderPass,
        transform: &HigurashiTransform,
        stencil: u8,
    ) -> Result<()> {
        if self.playback.input_blocked {
            return Ok(());
        }
        for draw in message_ui_draws(
            &self.playback,
            self.speaker_width,
            self.glyphs
                .as_ref()
                .map_or(glam::Vec2::ZERO, |glyphs| glyphs.size),
            // Native83730 allocates a minimum20-byte buffer even for empty text.
            self.glyphs.is_some(),
            self.ui_settings,
            transform,
            stencil,
        ) {
            draw.render(pass, &self.textures);
        }
        if let Some(glyphs) = &self.glyphs {
            glyphs.render(
                pass,
                transform,
                stencil,
                self.playback.natural_slide,
                self.playback.modal_slide,
                self.playback.height,
                &self.playback.progress(),
            )?;
        }
        Ok(())
    }
}
