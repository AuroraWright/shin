use std::sync::Arc;

use from_variants::FromVariants;
use glam::Mat4;
use parking_lot::{Mutex, MutexGuard};
use shin_core::{format::scenario::Scenario, vm::command::types::LayerbankId};
use shin_derive::RenderClone;
use shin_render::{PassKind, render_pass::RenderPass};

use crate::{
    adv::assets::AdvFonts,
    audio::VoicePlayer,
    layer::{
        DrawableLayer, Layer, LayerGroup,
        message_layer::{MessageLayer, MessageboxTextures},
        properties::LayerProperties,
        render_layer_without_bg,
        render_params::TransformParams,
        screen_layer::ScreenLayer,
    },
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext, Updatable, UpdateContext},
};

const OVERLAY_LAYERBANK: LayerbankId = LayerbankId::new_unchecked(0);
const MESSAGE_LAYERBANK: LayerbankId = LayerbankId::new_unchecked(1);
const SCREEN_LAYERBANK: LayerbankId = LayerbankId::new_unchecked(2);

#[derive(RenderClone, FromVariants)]
enum RootLayer {
    // Overlay(OverlayLayer),
    Message(SharedMessageLayer),
    Screen(#[render_clone(needs_render)] ScreenLayer),
}

// Native b00e0 returns the same message object with an increased refcount.
// Keep the properties facade outside the lock to satisfy DrawableLayer's
// borrowed property access; it points at the message's same property state.
struct SharedMessageLayer {
    inner: Arc<Mutex<MessageLayer>>,
    props: LayerProperties,
}

impl SharedMessageLayer {
    fn new(message: MessageLayer) -> Self {
        let props = message.properties().share();
        Self {
            inner: Arc::new(Mutex::new(message)),
            props,
        }
    }
}

impl Clone for SharedMessageLayer {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            props: self.props.share(),
        }
    }
}

impl AdvUpdatable for RootLayer {
    fn update(&mut self, context: &AdvUpdateContext) {
        match self {
            // RootLayer::Overlay(overlay) => overlay.update(context),
            RootLayer::Message(message) => message.inner.lock().update(context),
            RootLayer::Screen(screen) => screen.update(context),
        }
    }
}

impl Layer for RootLayer {
    fn fast_forward(&mut self) {
        match self {
            RootLayer::Message(message) => message.inner.lock().fast_forward(),
            RootLayer::Screen(screen) => screen.fast_forward(),
        }
    }

    fn get_stencil_bump(&self) -> u8 {
        match self {
            // RootLayer::Overlay(overlay) => overlay.get_stencil_bump(),
            RootLayer::Message(message) => message.inner.lock().get_stencil_bump(),
            RootLayer::Screen(screen) => screen.get_stencil_bump(),
        }
    }

    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        match self {
            // RootLayer::Overlay(overlay) => overlay.pre_render(context, transform),
            RootLayer::Message(message) => message.inner.lock().pre_render(context, transform),
            RootLayer::Screen(screen) => screen.pre_render(context, transform),
        }
    }

    #[tracing::instrument(skip_all)]
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        match self {
            // RootLayer::Overlay(overlay) => overlay.render(pass, transform, stencil_ref, pass_kind),
            RootLayer::Message(message) => {
                message
                    .inner
                    .lock()
                    .render(pass, transform, stencil_ref, pass_kind)
            }
            RootLayer::Screen(screen) => screen.render(pass, transform, stencil_ref, pass_kind),
        }
    }
}

impl DrawableLayer for RootLayer {
    fn render_order_key(&self) -> (f32, i32) {
        match self {
            // Native MessageLayer RTTI1daf28 derives directly from Layer,
            // not DrawableLayer. Comparator8129c/812a0 uses these sentinels
            // after the drawable cast fails; message properties don't sort it.
            RootLayer::Message(_) => (f32::MIN, i32::MAX),
            RootLayer::Screen(screen) => screen.render_order_key(),
        }
    }
    fn properties(&self) -> &LayerProperties {
        match self {
            // RootLayer::Overlay(overlay) => overlay.properties(),
            RootLayer::Message(message) => &message.props,
            RootLayer::Screen(screen) => screen.properties(),
        }
    }

    fn properties_mut(&mut self) -> &mut LayerProperties {
        match self {
            // RootLayer::Overlay(overlay) => overlay.properties_mut(),
            RootLayer::Message(message) => &mut message.props,
            RootLayer::Screen(screen) => screen.properties_mut(),
        }
    }
}

#[derive(RenderClone)]
pub struct RootLayerGroup {
    #[render_clone(needs_render)]
    inner: LayerGroup<RootLayer>,
}

impl RootLayerGroup {
    pub fn new(
        adv_fonts: AdvFonts,
        messagebox_textures: Arc<MessageboxTextures>,
        voice_player: VoicePlayer,
    ) -> Self {
        let mut inner = LayerGroup::new(Some("RootLayerGroup".to_string()));
        inner.add_layer(
            MESSAGE_LAYERBANK,
            SharedMessageLayer::new(MessageLayer::new(
                adv_fonts,
                messagebox_textures,
                voice_player,
            ))
            .into(),
        );
        inner.add_layer(SCREEN_LAYERBANK, ScreenLayer::new(4).into());

        Self { inner }
    }

    /// Native13ca8..13cf8 temporarily removes root child IDs1 and0 before
    /// rendering its menu snapshot, then restores the same layer objects.
    pub fn capture_modal_snapshot(
        &mut self,
        context: &mut PreRenderContext,
    ) -> shin_render::render_texture::RenderTexture {
        let message = self
            .inner
            .remove_layer(MESSAGE_LAYERBANK, shin_core::time::Ticks::ZERO);
        let overlay = self
            .inner
            .remove_layer(OVERLAY_LAYERBANK, shin_core::time::Ticks::ZERO);
        let transform = TransformParams::default();
        let mut snapshot = context.new_render_texture("Modal scene snapshot".into());
        self.pre_render(context, &transform);
        {
            let mut pass = context.begin_pass(
                snapshot.as_texture_target(),
                Some(context.depth_stencil),
                "Modal scene snapshot",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            render_layer_without_bg(&mut pass, &transform, self, 0);
        }
        if let Some(layer) = message {
            self.inner.add_layer(MESSAGE_LAYERBANK, layer);
        }
        if let Some(layer) = overlay {
            self.inner.add_layer(OVERLAY_LAYERBANK, layer);
        }
        // Restore the port's prepared draw list/offscreen caches as well as layers.
        // No update occurs, so message clocks, voices and transitions remain intact.
        self.pre_render(context, &transform);
        snapshot
    }

    pub fn screen_layer(&self) -> &ScreenLayer {
        let Some(RootLayer::Screen(screen)) = self.inner.get_layer(SCREEN_LAYERBANK) else {
            unreachable!();
        };

        screen
    }

    pub fn screen_layer_mut(&mut self) -> &mut ScreenLayer {
        let Some(RootLayer::Screen(screen)) = self.inner.get_layer_mut(SCREEN_LAYERBANK) else {
            unreachable!();
        };

        screen
    }

    pub(crate) fn message_owner(&self) -> Arc<Mutex<MessageLayer>> {
        let Some(RootLayer::Message(message)) = self.inner.get_layer(MESSAGE_LAYERBANK) else {
            unreachable!();
        };
        message.inner.clone()
    }

    pub fn message_layer(&self) -> MutexGuard<'_, MessageLayer> {
        let Some(RootLayer::Message(message)) = self.inner.get_layer(MESSAGE_LAYERBANK) else {
            unreachable!();
        };

        message.inner.lock()
    }

    pub fn message_layer_mut(&mut self) -> MutexGuard<'_, MessageLayer> {
        let Some(RootLayer::Message(message)) = self.inner.get_layer_mut(MESSAGE_LAYERBANK) else {
            unreachable!();
        };

        message.inner.lock()
    }
}

impl AdvUpdatable for RootLayerGroup {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.inner.update(context);
    }
}

impl Layer for RootLayerGroup {
    fn fast_forward(&mut self) {
        self.inner.fast_forward();
    }

    fn get_stencil_bump(&self) -> u8 {
        self.inner.get_stencil_bump()
    }

    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        self.inner.pre_render(context, transform);
    }

    #[tracing::instrument(skip_all)]
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        self.inner.render(pass, transform, stencil_ref, pass_kind);
    }
}

impl DrawableLayer for RootLayerGroup {
    fn properties(&self) -> &LayerProperties {
        self.inner.properties()
    }

    fn properties_mut(&mut self) -> &mut LayerProperties {
        self.inner.properties_mut()
    }
}

#[cfg(test)]
impl RootLayerGroup {
    pub(crate) fn reference_retained_message_ownership(
        mut self,
        context: &mut PreRenderContext,
        update: &mut AdvUpdateContext,
        scenario: &Arc<Scenario>,
    ) {
        use shin_core::vm::command::types::{AudioWaitStatus, LayerId, Volume};
        crate::layer::message_layer::reference_live_frame_guard(
            &mut self.message_layer_mut(),
            update,
        );
        let weak = {
            let Some(RootLayer::Message(message)) = self.inner.get_layer(MESSAGE_LAYERBANK) else {
                panic!()
            };
            Arc::downgrade(&message.inner)
        };
        let voice_pointer = self.message_layer().voice_player() as *const VoicePlayer;
        let mut back = shin_render::shaders::types::RenderClone::render_clone(
            &self,
            &mut context.render_clone_ctx(),
        );
        assert_eq!(weak.strong_count(), 2);
        assert_eq!(
            back.message_layer().voice_player() as *const VoicePlayer,
            voice_pointer
        );
        assert_ne!(self.properties().identity(), back.properties().identity());
        assert_ne!(
            self.screen_layer().properties().identity(),
            back.screen_layer().properties().identity()
        );
        let Some(RootLayer::Message(message)) = back.inner.get_layer_mut(MESSAGE_LAYERBANK) else {
            panic!()
        };
        message.props.set_layer_id(LayerId::new(71));
        assert_eq!(
            self.message_layer().properties().get_layer_id(),
            LayerId::new(71)
        );
        self.message_layer_mut()
            .properties_mut()
            .set_layer_id(LayerId::new(72));
        assert_eq!(message.props.get_layer_id(), LayerId::new(72));
        assert!(self.message_layer_mut().voice_player_mut().play(
            scenario,
            "19/11900001",
            0,
            0,
            crate::audio::VoicePlayFlags::empty(),
            Volume::default(),
        ));
        assert!(
            back.message_layer()
                .voice_player()
                .get_wait_status()
                .contains(AudioWaitStatus::PLAYING)
        );
        back.message_layer_mut().voice_player_mut().stop();
        assert!(
            self.message_layer()
                .voice_player()
                .get_wait_status()
                .is_empty()
        );
        let ticks = self.message_layer().reference_update_ticks();
        update.frame_id.advance();
        update.delta_ticks = shin_core::time::Ticks::from_f32(1.0);
        update.are_animations_allowed = false;
        self.update(update);
        back.update(update);
        assert_eq!(
            back.message_layer().reference_update_ticks(),
            ticks + update.delta_ticks
        );
        drop(self);
        assert_eq!(weak.strong_count(), 1);
        assert_eq!(
            back.message_layer().voice_player() as *const VoicePlayer,
            voice_pointer
        );
        drop(back);
        assert!(
            weak.upgrade().is_none(),
            "Last root releases the retained message and voice owner"
        );
        eprintln!(
            "Native root clone: same message/voice, shared property facade, independent root/screen properties, both-owner once-per-frame update and final-owner release pass"
        );
    }
}
