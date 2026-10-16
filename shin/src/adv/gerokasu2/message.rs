//! Own the later-engine message clock and prepared glyphs together.
//! Window geometry, clipping, portrait and ADV/audio adapters remain callers.
use anyhow::Result;
use glam::Mat4;
use shin_core::{primitives::update::FrameId, time::Ticks};
use shin_render::{ColorBlendType, PassKind, RenderRequestBuilder, render_pass::RenderPass};

use super::{
    message_clock::{MessageClock, MessageEffects, MessageParams},
    message_text::Gerokasu2MessageText,
    message_ui::{self, MessageTextures, MessageUiSettings},
};

pub struct Gerokasu2Message {
    clock: MessageClock,
    text: Gerokasu2MessageText,
}

impl Gerokasu2Message {
    pub fn new(text: Gerokasu2MessageText, params: MessageParams) -> Result<Self> {
        let clock = MessageClock::new(text.commands(), text.lines(), text.layout_size(), params)?;
        Ok(Self { clock, text })
    }

    pub fn clock(&self) -> &MessageClock {
        &self.clock
    }
    pub(super) fn clear_for_reconstruction(&mut self) {
        self.clock.clear_for_reconstruction();
        self.text.clear();
    }
    pub(super) fn clock_mut(&mut self) -> &mut MessageClock {
        &mut self.clock
    }
    pub fn text(&self) -> &Gerokasu2MessageText {
        &self.text
    }
    /// Publish an already prepared message while retaining the layer's frame
    /// tracker, modal state, outgoing windows and queued input requests.
    /// Native MSGSET does not stop backend voice during text replacement.
    pub fn replace(&mut self, mut text: Gerokasu2MessageText, params: MessageParams) -> Result<()> {
        self.clock
            .install(text.commands(), text.lines(), text.layout_size(), params)?;
        self.clock.apply_to_text(&mut text)?;
        self.text = text;
        Ok(())
    }
    pub fn request_autoplay(&mut self) {
        self.clock.request_autoplay();
    }
    pub fn send_sync(&mut self) {
        self.clock.send_sync();
    }
    pub fn set_modal_hidden(&mut self, hidden: bool) {
        self.clock.modal_hidden = hidden;
    }
    pub fn set_skip_speed(&mut self, speed: u8) {
        self.clock.skip_speed = speed;
    }

    pub fn update(
        &mut self,
        frame: FrameId,
        delta: Ticks,
        animations_allowed: bool,
        effects: &mut impl MessageEffects,
    ) -> Result<()> {
        self.clock.update(frame, delta, animations_allowed, effects);
        self.clock.apply_to_text(&mut self.text)
    }
    pub fn try_advance(
        &mut self,
        keep_voice_after_message: bool,
        effects: &mut impl MessageEffects,
    ) -> Result<bool> {
        let result = self.clock.try_advance(keep_voice_after_message, effects);
        self.clock.apply_to_text(&mut self.text)?;
        Ok(result)
    }
    pub fn fast_forward(&mut self, effects: &mut impl MessageEffects) -> Result<()> {
        self.clock.fast_forward(effects);
        self.clock.apply_to_text(&mut self.text)
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        transform: Mat4,
    ) -> Result<()> {
        self.text.render(pass, builder, transform)
    }

    /// Whole later-engine layer submission. Parent uses native0..1 clip depth;
    /// local is the MessageLayer geometry matrix (default translation -960,-540).
    /// The caller owns the depth/stencil attachment and portrait submission.
    pub fn render_layer(
        &self,
        pass: &mut RenderPass,
        textures: &MessageTextures,
        settings: MessageUiSettings,
        parent: Mat4,
        local: Mat4,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        self.render_layer_with_portrait(
            pass, textures, settings, parent, local, stencil, kind, None,
        )
    }

    pub(crate) fn render_layer_with_portrait(
        &self,
        pass: &mut RenderPass,
        textures: &MessageTextures,
        settings: MessageUiSettings,
        parent: Mat4,
        local: Mat4,
        stencil: u8,
        kind: PassKind,
        portrait: Option<&super::message_portrait::Gerokasu2MessagePortrait>,
    ) -> Result<()> {
        let plan = message_ui::plan(&self.clock, true, settings, parent, local, stencil, kind)?;
        for draw in &plan.draws {
            draw.render(pass, textures);
        }
        if let Some(portrait) = portrait {
            portrait.render(pass, &self.clock, parent, stencil, kind);
        }
        if let Some(transform) = plan.text_transform {
            self.text.render(
                pass,
                RenderRequestBuilder::new()
                    .color_blend_type(ColorBlendType::Layer1)
                    .depth_stencil_shorthand(plan.text_stencil, true, true),
                transform,
            )?;
        }
        Ok(())
    }
}
