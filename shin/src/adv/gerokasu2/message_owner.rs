//! Evaluated ADV commands share one later-engine message/resource owner.
use anyhow::{Result, ensure};
use glam::Mat4;
use shin_core::{
    format::scenario::gerokasu2::BustupInfoItem,
    primitives::update::FrameId,
    time::Ticks,
    vm::gerokasu2::{
        Gerokasu2RuntimeCommand,
        message_face::{MessageFaceState, PortraitChange},
    },
};
use shin_render::{PassKind, render_pass::RenderPass};
use std::sync::Arc;

use super::{
    message::Gerokasu2Message,
    message_clock::{MessageEffects, VoiceTimeline},
    message_commands::{
        MessageAction, MessageCommandContext, MessageCommandState, MessageCommandStatus,
    },
    message_portrait::Gerokasu2MessagePortrait,
    message_text::Gerokasu2MessageText,
    message_ui::{MessageResources, MessageUiSettings},
};
use crate::asset::system::AssetServer;

pub struct MessageCommandOutcome {
    pub status: MessageCommandStatus,
    /// The runner submits this once to the owned, generation-checked portrait
    /// loader before MSGSET's save callback. Standalone dispatch callers use
    /// apply_portrait at the same boundary.
    pub portrait: PortraitChange,
    pub installed_message: Option<u32>,
}

pub struct Gerokasu2MessageOwner {
    resources: MessageResources,
    commands: MessageCommandState,
    face: MessageFaceState,
    message: Option<Gerokasu2Message>,
    portrait: Gerokasu2MessagePortrait,
    pub ui_settings: MessageUiSettings,
}

/// Native8c528 returns the same MessageLayer owner across root copies.
#[derive(Clone)]
pub struct SharedMessageOwner(Arc<parking_lot::Mutex<Gerokasu2MessageOwner>>);
impl SharedMessageOwner {
    pub fn new(owner: Gerokasu2MessageOwner) -> Self {
        Self(Arc::new(parking_lot::Mutex::new(owner)))
    }
    pub fn lock(&self) -> parking_lot::MutexGuard<'_, Gerokasu2MessageOwner> {
        self.0.lock()
    }
    pub fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    #[cfg(test)]
    pub(crate) fn weak_owner(&self) -> impl Fn() -> usize + use<> {
        let weak = Arc::downgrade(&self.0);
        move || weak.strong_count()
    }
}
impl Gerokasu2MessageOwner {
    pub async fn load(
        assets: &AssetServer,
        style: i32,
        async_message_active: bool,
        retained_face: i32,
    ) -> Result<Self> {
        Ok(Self::from_resources(
            MessageResources::load(assets).await?,
            style,
            async_message_active,
            retained_face,
        ))
    }
    pub fn from_resources(
        resources: MessageResources,
        style: i32,
        async_message_active: bool,
        retained_face: i32,
    ) -> Self {
        Self {
            resources,
            commands: MessageCommandState::new(style, async_message_active),
            face: MessageFaceState::new(retained_face),
            message: None,
            portrait: Default::default(),
            ui_settings: Default::default(),
        }
    }
    pub fn resources(&self) -> super::message_ui::MessageResources {
        super::message_ui::MessageResources {
            medium: self.resources.medium.clone(),
            textures: self.resources.textures.clone(),
        }
    }
    /// Publish VM command fields without reinstalling visible text. The native
    /// retained MSGSET first poll owns installation; mode0 owns display clear.
    pub(crate) fn restore_descriptor(
        &mut self,
        descriptor: &super::descriptors::MessageDescriptor,
    ) {
        self.commands.restore_descriptor(descriptor);
        self.face = MessageFaceState::new(descriptor.face);
    }
    pub(crate) fn clear_for_reconstruction(&mut self) {
        if let Some(message) = self.message.as_mut() {
            message.clear_for_reconstruction();
        }
        self.portrait = Default::default();
    }
    pub fn message(&self) -> Option<&Gerokasu2Message> {
        self.message.as_ref()
    }
    pub(crate) fn close_for_exit(&mut self) {
        if let Some(message) = self.message.as_mut() {
            message.clock_mut().close(true);
        }
    }
    pub(crate) fn exit_busy(&self) -> bool {
        self.message.as_ref().is_some_and(|m| m.clock().busy())
    }
    pub fn portrait(&self) -> &Gerokasu2MessagePortrait {
        &self.portrait
    }
    pub fn apply_portrait(&mut self, assets: Arc<AssetServer>, change: PortraitChange) {
        self.portrait.apply(assets, change);
    }
    pub fn update_portrait(
        &mut self,
        frame: FrameId,
        delta: Ticks,
        voice: &super::voice::Gerokasu2VoicePlayer,
    ) {
        self.portrait.update(frame, delta, voice);
    }
    pub fn command_pending(&self) -> bool {
        self.commands.command_pending()
    }
    pub fn async_message_active(&self) -> bool {
        self.commands.async_message_active()
    }
    pub fn retained_face(&self) -> i32 {
        self.face.retained_index()
    }
    pub fn retained_text(&self) -> Option<&str> {
        self.commands.retained_text()
    }
    pub fn set_modal_hidden(&mut self, hidden: bool) {
        if let Some(message) = self.message.as_mut() {
            message.set_modal_hidden(hidden);
        }
    }

    pub fn dispatch(
        &mut self,
        command: &Gerokasu2RuntimeCommand,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        context: MessageCommandContext,
        voice: Option<VoiceTimeline>,
        bustups: &[BustupInfoItem],
    ) -> Result<Option<MessageCommandOutcome>> {
        ensure!(
            !self.command_pending(),
            "Gerokasu2 message command is still pending"
        );
        if let Gerokasu2RuntimeCommand::MessageFace { face } = command {
            return Ok(Some(MessageCommandOutcome {
                status: MessageCommandStatus::Complete,
                portrait: self
                    .face
                    .on_msgface(*face, self.async_message_active(), bustups),
                installed_message: None,
            }));
        }
        let Some(action) = self.commands.begin(
            command,
            self.message.as_mut().map(Gerokasu2Message::clock_mut),
            context,
        )?
        else {
            return Ok(None);
        };
        let mut portrait = PortraitChange::Unchanged;
        let mut installed_message = None;
        if let MessageAction::Install(install) = action {
            let Gerokasu2RuntimeCommand::MessageSet { text, .. } = command else {
                unreachable!()
            };
            crate::text_stream::message(
                text,
                crate::text_stream::Markup::Standard,
                install.params.message_id,
            );
            let text = Gerokasu2MessageText::layout_and_prepare(
                device,
                queue,
                self.resources.fonts(),
                text,
                install.params.mode,
                install.alignment,
                context.message_speed,
            )?;
            if let Some(message) = &mut self.message {
                message.replace(text, install.params)?;
            } else {
                self.message = Some(Gerokasu2Message::new(text, install.params)?);
            }
            portrait = self.face.on_message_installed(bustups);
            installed_message = Some(install.params.message_id);
        }
        Ok(Some(MessageCommandOutcome {
            status: self.poll_command(voice),
            portrait,
            installed_message,
        }))
    }
    pub fn poll_command(&mut self, voice: Option<VoiceTimeline>) -> MessageCommandStatus {
        self.commands
            .poll(self.message.as_ref().map(Gerokasu2Message::clock), voice)
    }
    pub fn update(
        &mut self,
        frame: FrameId,
        delta: Ticks,
        animations_allowed: bool,
        effects: &mut impl MessageEffects,
    ) -> Result<()> {
        if let Some(message) = &mut self.message {
            message.update(frame, delta, animations_allowed, effects)?;
        }
        Ok(())
    }
    pub fn try_advance(
        &mut self,
        keep_voice_after_message: bool,
        effects: &mut impl MessageEffects,
    ) -> Result<bool> {
        self.message.as_mut().map_or(Ok(false), |message| {
            message.try_advance(keep_voice_after_message, effects)
        })
    }
    pub fn fast_forward(&mut self, effects: &mut impl MessageEffects) -> Result<()> {
        if let Some(message) = &mut self.message {
            message.fast_forward(effects)?;
        }
        Ok(())
    }
    pub fn request_autoplay(&mut self) {
        if let Some(message) = &mut self.message {
            message.request_autoplay();
        }
    }
    pub(crate) fn message_mut_settings(&mut self, skip_speed: u8) {
        if let Some(message) = self.message.as_mut() {
            message.set_skip_speed(skip_speed);
        }
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        parent: Mat4,
        local: Mat4,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        if let Some(message) = &self.message {
            message.render_layer_with_portrait(
                pass,
                &self.resources.textures,
                self.ui_settings,
                parent,
                local,
                stencil,
                kind,
                Some(&self.portrait),
            )?;
        }
        Ok(())
    }
}
