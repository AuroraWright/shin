//! Native EffectLayer63290: captures preceding layers, not an animation emitter.
use crate::{
    layer::{
        DrawableLayer, Layer, LayerProperties, NewDrawableLayer,
        new_drawable_layer::{NewDrawableLayerNeedsSeparatePass, NewDrawableLayerState},
        owner::RetainedLayer,
        render_layers_with_bg,
        render_params::{DrawableClipParams, DrawableParams, TransformParams},
    },
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext},
};
use shin_core::primitives::color::UnormColor;
use shin_render::{
    PassKind,
    render_pass::RenderPass,
    shaders::types::{
        RenderClone, RenderCloneCtx,
        texture::{DepthStencilTarget, TextureTarget},
    },
};

#[derive(Debug)]
pub struct EffectLayer {
    update_tracker: crate::layer::update_tracker::LayerUpdateTracker,
    state: NewDrawableLayerState,
    props: LayerProperties,
    pending_children: Vec<RetainedLayer>,
}
impl RenderClone for EffectLayer {
    fn render_clone(&self, context: &mut RenderCloneCtx) -> Self {
        Self {
            update_tracker: self.update_tracker.clone(),
            state: self.state.render_clone(context),
            props: self.props.clone(),
            // Native63790 copies drawable state but initializes an empty list.
            pending_children: Vec::new(),
        }
    }
}
impl EffectLayer {
    fn assign_owned_lower(&mut self, lower: &[RetainedLayer]) {
        let common = self.pending_children.len().min(lower.len());
        for (current, next) in self.pending_children[..common].iter_mut().zip(lower) {
            current.clone_from(next);
        }
        self.pending_children.truncate(lower.len());
        self.pending_children.extend_from_slice(&lower[common..]);
    }
    pub fn new() -> Self {
        Self {
            update_tracker: Default::default(),
            state: NewDrawableLayerState::new(),
            props: LayerProperties::new(),
            pending_children: Vec::new(),
        }
    }
}
struct Capture<'a> {
    lower: &'a [&'a dyn Layer],
}
struct OwnedCapture<'a> {
    pending: &'a mut Vec<RetainedLayer>,
}
impl NewDrawableLayerNeedsSeparatePass for OwnedCapture<'_> {
    fn needs_separate_pass(&self, _props: &LayerProperties) -> bool {
        true
    }
}
impl NewDrawableLayer for OwnedCapture<'_> {
    fn render_drawable_indirect(
        &mut self,
        context: &mut PreRenderContext,
        props: &LayerProperties,
        target: TextureTarget,
        depth_stencil: DepthStencilTarget,
        transform: &TransformParams,
    ) -> PassKind {
        let lower: Vec<&dyn Layer> = self.pending.iter().map(|l| l as &dyn Layer).collect();
        let pass = Capture { lower: &lower }.render_drawable_indirect(
            context,
            props,
            target,
            depth_stencil,
            transform,
        );
        drop(lower);
        // Native639b8 releases children after their actual indirect draw. The
        // invisible base early return never invokes this delegate.
        self.pending.clear();
        pass
    }
    fn render_drawable_direct(
        &self,
        _pass: &mut RenderPass,
        _transform: &TransformParams,
        _drawable: &DrawableParams,
        _clip: &DrawableClipParams,
        _stencil_ref: u8,
        _pass_kind: PassKind,
    ) {
    }
}
impl NewDrawableLayerNeedsSeparatePass for Capture<'_> {
    fn needs_separate_pass(&self, _props: &LayerProperties) -> bool {
        true
    }
}
impl NewDrawableLayer for Capture<'_> {
    fn render_drawable_indirect(
        &mut self,
        context: &mut PreRenderContext,
        _props: &LayerProperties,
        target: TextureTarget,
        depth_stencil: DepthStencilTarget,
        transform: &TransformParams,
    ) -> PassKind {
        let mut pass = context.begin_pass(target, Some(depth_stencil), "EffectLayer/capture");
        pass.clear(Some(UnormColor::BLACK), Some(0), None);
        // Native766a4 forwards the capture's parent transform unchanged. Effect
        // layer transform affects final clipping; it doesn't move its siblings.
        render_layers_with_bg(&mut pass, transform, self.lower, |_, _, _| {}, true, 0);
        PassKind::Opaque
    }
    fn render_drawable_direct(
        &self,
        _pass: &mut RenderPass,
        _transform: &TransformParams,
        _drawable: &DrawableParams,
        _clip: &DrawableClipParams,
        _stencil_ref: u8,
        _pass_kind: PassKind,
    ) { /* Native79380: no direct geometry. */
    }
}
impl AdvUpdatable for EffectLayer {
    fn update(&mut self, context: &AdvUpdateContext) {
        if !self.update_tracker.update(context.frame_id) {
            return;
        }
        self.props.update(context);
        self.state.update(context, &self.props);
    }
}
impl Layer for EffectLayer {
    fn needs_lower_capture(&self) -> bool {
        true
    }
    fn fast_forward(&mut self) {
        self.props.fast_forward();
    }
    fn pre_render_with_owned_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[RetainedLayer],
    ) -> bool {
        self.assign_owned_lower(lower);
        self.state.pre_render(
            context,
            &self.props,
            &mut OwnedCapture {
                pending: &mut self.pending_children,
            },
            transform,
        );
        true
    }
    // Native63810 regular pre-render is empty. The borrowed compatibility path
    // remains for callers that have not yet adopted stable batch owners.
    fn pre_render_with_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[&dyn Layer],
    ) -> bool {
        self.pending_children.clear();
        self.state
            .pre_render(context, &self.props, &mut Capture { lower }, transform);
        true
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        self.state.render(
            &self.props,
            &Capture { lower: &[] },
            pass,
            transform,
            stencil_ref,
            pass_kind,
        );
    }
}
impl DrawableLayer for EffectLayer {
    fn properties(&self) -> &LayerProperties {
        &self.props
    }
    fn properties_mut(&mut self) -> &mut LayerProperties {
        &mut self.props
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod owner_tests;
