use super::SharedSelection;
use crate::{
    layer::{
        DrawableLayer, LayerProperties, NewDrawableLayer, NewDrawableLayerFastForward,
        NewDrawableLayerNeedsSeparatePass, NewDrawableLayerWrapper, PropertyPolicy,
        render_params::{DrawableClipParams, DrawableParams, TransformParams},
    },
    update::{AdvUpdatable, AdvUpdateContext},
};
use shin_render::{
    PassKind,
    render_pass::RenderPass,
    shaders::types::{RenderClone, RenderCloneCtx},
};
use std::sync::Arc;
#[derive(Clone)]
pub(crate) struct SelectionRenderer {
    owner: SharedSelection,
    error: Arc<parking_lot::Mutex<Option<String>>>,
}
pub(crate) type SelectionDrawable = NewDrawableLayerWrapper<SelectionRenderer>;
pub(crate) fn drawable(
    owner: SharedSelection,
    error: Arc<parking_lot::Mutex<Option<String>>>,
) -> SelectionDrawable {
    let mut d =
        SelectionDrawable::from_retained_inner_constructor(|| SelectionRenderer { owner, error });
    d.set_properties(LayerProperties::with_policy(PropertyPolicy::Gerokasu2));
    d
}
impl RenderClone for SelectionRenderer {
    fn render_clone(&self, _: &mut RenderCloneCtx) -> Self {
        self.clone()
    }
}
impl AdvUpdatable for SelectionRenderer {
    fn update(&mut self, ctx: &AdvUpdateContext) {
        self.owner.lock().update(ctx.delta_ticks);
    }
}
impl NewDrawableLayerFastForward for SelectionRenderer {
    fn fast_forward(&mut self) {}
}
impl NewDrawableLayerNeedsSeparatePass for SelectionRenderer {
    fn needs_separate_pass(&self, props: &LayerProperties) -> bool {
        props.is_blending_nontrivial() || props.is_fragment_shader_nontrivial()
    }
}
impl NewDrawableLayer for SelectionRenderer {
    fn stencil_bump(&self) -> u8 {
        1
    }
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        t: &TransformParams,
        _: &DrawableParams,
        _: &DrawableClipParams,
        s: u8,
        k: PassKind,
    ) {
        if let Err(e) = self
            .owner
            .lock()
            .render(pass, t.compute_final_transform(), s, k)
        {
            *self.error.lock() = Some(format!("{e:#}"));
        }
    }
}
