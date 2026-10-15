//! Real later-engine root: bank0 Screen,bank1 retained NewDrawable Message.
use super::{layer_factory::BasicDrawable, message_owner::SharedMessageOwner};
use crate::{
    layer::{
        DrawableLayer, Layer, LayerGroup, LayerProperties, NewDrawableLayer,
        NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass, NewDrawableLayerWrapper,
        PropertyPolicy, ScreenLayer,
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
    },
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext},
};
use anyhow::{Result, bail};
use glam::{Mat4, vec3};
use shin_core::vm::command::types::{LayerbankId, PLANES_COUNT};
use shin_render::{
    PassKind,
    render_pass::RenderPass,
    shaders::types::{RenderClone, RenderCloneCtx},
};
use std::sync::Arc;

const SCREEN: LayerbankId = LayerbankId::new_unchecked(0);
const OVERLAY: LayerbankId = LayerbankId::new_unchecked(2);
const MESSAGE: LayerbankId = LayerbankId::new_unchecked(1);
#[derive(Clone)]
struct MessageRenderer {
    owner: SharedMessageOwner,
    error: Arc<parking_lot::Mutex<Option<String>>>,
}
impl RenderClone for MessageRenderer {
    fn render_clone(&self, _: &mut RenderCloneCtx) -> Self {
        self.clone()
    }
}
impl AdvUpdatable for MessageRenderer {
    fn update(&mut self, _: &AdvUpdateContext) {}
}
impl NewDrawableLayerFastForward for MessageRenderer {
    fn fast_forward(&mut self) {}
}
impl NewDrawableLayerNeedsSeparatePass for MessageRenderer {
    fn needs_separate_pass(&self, props: &LayerProperties) -> bool {
        props.is_blending_nontrivial()
            || props.is_fragment_shader_nontrivial()
            || props.get_clip_mode() != DrawableClipMode::None
    }
}
impl NewDrawableLayer for MessageRenderer {
    fn stencil_bump(&self) -> u8 {
        3
    } // original8f97c
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        _: &DrawableParams,
        _: &DrawableClipParams,
        stencil: u8,
        kind: PassKind,
    ) {
        if let Err(error) = self.owner.lock().render(
            pass,
            transform.compute_final_transform(),
            Mat4::from_translation(vec3(-960.0, -540.0, 0.0)),
            stencil,
            kind,
        ) {
            *self.error.lock() = Some(format!("{error:#}"));
        }
    }
}
type MessageDrawable = NewDrawableLayerWrapper<MessageRenderer>;
enum RootChild {
    Screen(ScreenLayer<BasicDrawable>),
    Message(MessageDrawable),
    Selection(super::selection::draw::SelectionDrawable),
}
impl RenderClone for RootChild {
    fn render_clone(&self, ctx: &mut RenderCloneCtx) -> Self {
        match self {
            Self::Screen(s) => Self::Screen(s.render_clone(ctx)),
            Self::Message(m) => Self::Message(m.render_clone(ctx)),
            Self::Selection(s) => Self::Selection(s.render_clone(ctx)),
        }
    }
}
impl AdvUpdatable for RootChild {
    fn update(&mut self, ctx: &AdvUpdateContext) {
        match self {
            Self::Screen(s) => s.update(ctx),
            Self::Message(m) => m.update(ctx),
            Self::Selection(s) => s.update(ctx),
        }
    }
}
impl Layer for RootChild {
    fn fast_forward(&mut self) {
        match self {
            Self::Screen(s) => s.fast_forward(),
            Self::Message(m) => m.fast_forward(),
            Self::Selection(s) => s.fast_forward(),
        }
    }
    fn get_stencil_bump(&self) -> u8 {
        match self {
            Self::Screen(s) => s.get_stencil_bump(),
            Self::Message(m) => m.get_stencil_bump(),
            Self::Selection(s) => s.get_stencil_bump(),
        }
    }
    fn pre_render(&mut self, ctx: &mut PreRenderContext, t: &TransformParams) {
        match self {
            Self::Screen(s) => s.pre_render(ctx, t),
            Self::Message(m) => m.pre_render(ctx, t),
            Self::Selection(s) => s.pre_render(ctx, t),
        }
    }
    fn render(&self, p: &mut RenderPass, t: &TransformParams, s: u8, k: PassKind) {
        match self {
            Self::Screen(screen) => screen.render(p, t, s, k),
            Self::Message(m) => m.render(p, t, s, k),
            Self::Selection(choice) => choice.render(p, t, s, k),
        }
    }
}
impl DrawableLayer for RootChild {
    fn properties(&self) -> &LayerProperties {
        match self {
            Self::Screen(s) => s.properties(),
            Self::Message(m) => m.properties(),
            Self::Selection(s) => s.properties(),
        }
    }
    fn properties_mut(&mut self) -> &mut LayerProperties {
        match self {
            Self::Screen(s) => s.properties_mut(),
            Self::Message(m) => m.properties_mut(),
            Self::Selection(s) => s.properties_mut(),
        }
    }
}
pub struct Gerokasu2Root {
    group: LayerGroup<RootChild>,
    error: Arc<parking_lot::Mutex<Option<String>>>,
}
impl Gerokasu2Root {
    #[cfg(test)]
    pub(crate) fn reference_message_ownership(&self) -> Option<(usize, usize, [f32; 3])> {
        match self.group.get_direct_layer(MESSAGE) {
            Some(RootChild::Message(m)) => Some((
                m.properties().identity(),
                m.reference_retained_render_identity().unwrap(),
                m.reference_effect_phases(),
            )),
            _ => None,
        }
    }
    pub fn new() -> Self {
        let mut group = LayerGroup::new_direct_with_policy(
            Some("Gerokasu2Root".into()),
            PropertyPolicy::Gerokasu2,
        );
        group.add_layer(
            SCREEN,
            RootChild::Screen(ScreenLayer::with_property_policy(
                PLANES_COUNT,
                PropertyPolicy::Gerokasu2,
            )),
        );
        Self {
            group,
            error: Default::default(),
        }
    }
    pub fn attach_message(&mut self, owner: SharedMessageOwner) -> Result<()> {
        if let Some(RootChild::Message(message)) = self.group.get_direct_layer(MESSAGE) {
            if message.inner_ref().owner.same_owner(&owner) {
                return Ok(());
            }
            bail!("Gerokasu2 root already has a different message owner");
        }
        let mut message = MessageDrawable::from_retained_inner_constructor(|| MessageRenderer {
            owner,
            error: self.error.clone(),
        });
        message.set_properties(LayerProperties::with_policy(PropertyPolicy::Gerokasu2));
        self.group.add_layer(MESSAGE, RootChild::Message(message));
        Ok(())
    }
    pub fn attach_selection(&mut self, owner: super::selection::SharedSelection) {
        self.group.add_layer(
            OVERLAY,
            RootChild::Selection(super::selection::draw::drawable(owner, self.error.clone())),
        );
    }
    pub fn remove_selection(&mut self) {
        self.group
            .remove_layer(OVERLAY, shin_core::time::Ticks::ZERO);
    }
    pub fn has_message(&self) -> bool {
        self.group.get_direct_layer(MESSAGE).is_some()
    }
    pub fn message_owner(&self) -> Option<SharedMessageOwner> {
        match self.group.get_direct_layer(MESSAGE) {
            Some(RootChild::Message(m)) => Some(m.inner_ref().owner.clone()),
            _ => None,
        }
    }
    pub fn message_properties_mut(&mut self) -> Option<&mut LayerProperties> {
        match self.group.get_direct_layer_mut(MESSAGE) {
            Some(RootChild::Message(message)) => Some(message.properties_mut()),
            _ => None,
        }
    }
    pub fn screen(&self) -> &ScreenLayer<BasicDrawable> {
        match self.group.get_direct_layer(SCREEN) {
            Some(RootChild::Screen(s)) => s,
            _ => unreachable!(),
        }
    }
    pub fn screen_mut(&mut self) -> &mut ScreenLayer<BasicDrawable> {
        match self.group.get_direct_layer_mut(SCREEN) {
            Some(RootChild::Screen(s)) => s,
            _ => unreachable!(),
        }
    }
    /// Native6f4d0 advances root base,Screen0,then Message1. The runner performs
    /// the message clock after this tree/base update; retained frame guards
    /// prevent a second tree update from advancing the same owners again.
    pub fn prepare_message_frame(&mut self, ctx: &AdvUpdateContext) {
        self.group.update(ctx);
    }
    pub fn check_render(&self) -> Result<()> {
        if let Some(error) = self.error.lock().take() {
            bail!("Gerokasu2 root message render: {error}");
        }
        Ok(())
    }
}
impl RenderClone for Gerokasu2Root {
    fn render_clone(&self, ctx: &mut RenderCloneCtx) -> Self {
        let mut group = self.group.render_clone(ctx);
        // Native root copies Screen0 and the retained Message1; overlays do
        // not survive page/history snapshots.
        group.remove_layer(OVERLAY, shin_core::time::Ticks::ZERO);
        Self {
            group,
            error: self.error.clone(),
        }
    }
}
impl AdvUpdatable for Gerokasu2Root {
    fn update(&mut self, ctx: &AdvUpdateContext) {
        self.group.update(ctx);
    }
}
impl Layer for Gerokasu2Root {
    fn fast_forward(&mut self) {
        self.group.fast_forward();
    }
    fn get_stencil_bump(&self) -> u8 {
        self.group.get_stencil_bump()
    }
    fn pre_render(&mut self, ctx: &mut PreRenderContext, t: &TransformParams) {
        self.group.pre_render(ctx, t);
    }
    fn render(&self, p: &mut RenderPass, t: &TransformParams, s: u8, k: PassKind) {
        self.group.render(p, t, s, k);
    }
}
impl DrawableLayer for Gerokasu2Root {
    fn properties(&self) -> &LayerProperties {
        self.group.properties()
    }
    fn properties_mut(&mut self) -> &mut LayerProperties {
        self.group.properties_mut()
    }
}

#[cfg(test)]
mod tests;
