//! ADV's sibling message/screen owners, frame gate and command ownership.
//! The optional interactive overlay participates in the native group traversal.
use anyhow::{Result, ensure};
use shin_core::{
    format::scenario::higurashi::HigurashiScenario, time::Ticks,
    vm::higurashi::command::HigurashiRuntimeCommand,
};
use shin_render::PassKind;

use super::{
    message_owner::HigurashiMessageOwner,
    message_playback::MessageVoice,
    scene::{CommandStatus, HigurashiScene},
};
use crate::asset::system::AssetServer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Child {
    Overlay,
    Message,
    Screen,
}
pub fn render_plan_with_overlay(message: f32, screen: f32, overlay: Option<f32>) -> Vec<Draw> {
    render_plan_with_overlay_owner(message, screen, overlay.map(|priority| (priority, 0)))
}
pub(crate) fn render_plan_with_overlay_owner(
    message: f32,
    screen: f32,
    overlay: Option<(f32, i32)>,
) -> Vec<Draw> {
    let Some((overlay, owner_id)) = overlay else {
        return render_plan(message, screen).to_vec();
    };
    let mut children = [
        (Child::Overlay, overlay, owner_id, 1),
        (Child::Message, message, 0, 3),
        (Child::Screen, screen, 0, 1),
    ];
    children.sort_by(|a, b| {
        if a.1 == b.1 {
            a.2.cmp(&b.2)
        } else {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        }
    });
    let mut reference = 1;
    let mut draws = Vec::with_capacity(6);
    for (child, _, _, count) in children {
        draws.push(Draw {
            child,
            stencil: reference,
            pass: PassKind::Transparent,
        });
        reference += count;
    }
    let mut result = draws
        .iter()
        .rev()
        .map(|d| Draw {
            pass: PassKind::Opaque,
            ..*d
        })
        .collect::<Vec<_>>();
    result.extend(draws);
    result
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Draw {
    pub child: Child,
    pub stencil: u8,
    pub pass: PassKind,
}

/// Native stable sort6e340 and traversal686d8. Screen reserves one stencil
/// value (6826c), message three (85e14), including its glyph reference+2.
pub fn render_plan(message_priority: f32, screen_priority: f32) -> [Draw; 4] {
    let (children, stencils) = if screen_priority > message_priority {
        ([Child::Screen, Child::Message], [1, 2])
    } else {
        // Both fresh base owner IDs are0. Stable keyed storage puts Message
        // before Screen; keyed Group IDs1/2 are not base owner IDs.
        ([Child::Message, Child::Screen], [1, 4])
    };
    [
        Draw {
            child: children[1],
            stencil: stencils[1],
            pass: PassKind::Opaque,
        },
        Draw {
            child: children[0],
            stencil: stencils[0],
            pass: PassKind::Opaque,
        },
        Draw {
            child: children[0],
            stencil: stencils[0],
            pass: PassKind::Transparent,
        },
        Draw {
            child: children[1],
            stencil: stencils[1],
            pass: PassKind::Transparent,
        },
    ]
}

pub struct HigurashiFrame {
    pub scene: HigurashiScene,
    pub message: HigurashiMessageOwner,
    pub(crate) cache: super::adv_cache::AdvCache,
    visual_epoch: u64,
    autoplay_request: bool,
    skip_request: bool,
}
impl HigurashiFrame {
    pub fn new(scene: HigurashiScene, message: HigurashiMessageOwner) -> Self {
        Self {
            scene,
            message,
            cache: Default::default(),
            visual_epoch: 0,
            autoplay_request: false,
            skip_request: false,
        }
    }
    pub fn command_pending(&self) -> bool {
        self.scene.command_pending() || self.message.command_pending()
    }
    /// Prevent another command owner from running while MSGWAIT or WIPE owns
    /// the VM continuation. Asset loads complete before publishing layers.
    pub async fn dispatch(
        &mut self,
        command: &HigurashiRuntimeCommand,
        assets: &AssetServer,
        scenario: &HigurashiScenario,
        seeds: impl FnMut() -> [u32; 4],
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fast_skip: bool,
        read_flags: u32,
        text_speed: i32,
        outline_mode: u8,
        voice: &impl MessageVoice,
    ) -> Result<Option<CommandStatus>> {
        ensure!(
            !self.command_pending(),
            "Higurashi ADV command is still pending"
        );
        if let Some(status) = self.message.dispatch(
            command,
            device,
            queue,
            fast_skip,
            read_flags,
            text_speed,
            outline_mode,
            voice,
        )? {
            return Ok(Some(status));
        }
        if let Some(status) = self.scene.dispatch(command, fast_skip)? {
            return Ok(Some(status));
        }
        Ok(self
            .scene
            .layers_mut()?
            .dispatch(command, assets, scenario, seeds)
            .await?
            .then_some(CommandStatus::Complete))
    }
    pub fn poll_command(&mut self, fast_skip: bool, voice: &impl MessageVoice) -> CommandStatus {
        if self.message.command_pending() {
            self.message.poll_command(voice)
        } else {
            self.scene.poll_command(fast_skip)
        }
    }
    /// Requests survive disabled/paused frames:84694 consumes14a0/14a1 only
    /// after ADV12da8 permits the outer group callback to run.
    pub fn request_message_update(&mut self, autoplay: bool, skip: bool) {
        self.autoplay_request |= autoplay;
        self.skip_request |= skip;
    }
    pub fn advance(
        &mut self,
        delta: Ticks,
        animations_enabled: bool,
        paused: bool,
        voice: &mut impl MessageVoice,
        lipsync: Option<&dyn Fn(i32, f32) -> Option<u8>>,
    ) {
        if !animations_enabled || paused || !self.scene.animations_enabled() {
            return;
        }
        // Outer group6dbf0 visits storage order, child1 then child2. Properties
        // advance inside each owner's callback, never a second time afterwards.
        self.scene.advance_message_properties(delta);
        self.message.advance(
            delta,
            std::mem::take(&mut self.autoplay_request),
            std::mem::take(&mut self.skip_request),
            voice,
        );
        let voice_feedback = |character, pan| voice.character_lipsync(character, pan);
        self.scene
            .advance_screen(delta, lipsync.or(Some(&voice_feedback)));
        self.visual_epoch = self.visual_epoch.wrapping_add(1);
    }
    pub(crate) fn visual_epoch(&self) -> u64 {
        self.visual_epoch
    }
    pub fn render_plan(&self) -> [Draw; 4] {
        let layers = self.scene.layers();
        render_plan(
            layers.message_properties().values()[5],
            layers.screen_properties().values()[5],
        )
    }
}

#[cfg(test)]
pub(super) mod tests;
