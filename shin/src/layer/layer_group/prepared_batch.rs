//! Native 806b0 prepared Slot: a distinct pair vector, shared wipe and captures.
use super::*;
use crate::{
    layer::{
        owner::{LayerOwner, RetainedLayer},
        render_layers,
    },
    render::render_texture_holder::RenderTextureHolder,
    wiper::Wiper,
};

pub(super) struct PreparedPair {
    pub source: Option<RetainedLayer>,
    pub target: RetainedLayer,
}

pub(super) struct PreparedSlotBatch {
    update_tracker: crate::layer::update_tracker::LayerUpdateTracker,
    pub(super) pairs: Vec<PreparedPair>,
    pub(super) wiper: Option<LayerOwner<AnyWiper>>,
    pub(super) source_texture: RenderTextureHolder,
    pub(super) target_texture: RenderTextureHolder,
}
impl PreparedSlotBatch {
    pub(super) fn new(first: PreparedPair, wiper: LayerOwner<AnyWiper>) -> Self {
        Self {
            update_tracker: Default::default(),
            pairs: vec![first],
            wiper: Some(wiper),
            source_texture: RenderTextureHolder::new("PreparedSlotBatch/source"),
            target_texture: RenderTextureHolder::new("PreparedSlotBatch/target"),
        }
    }
    pub(super) fn append(&mut self, pair: PreparedPair) {
        self.pairs.push(pair);
    }

    fn prepare(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[RetainedLayer],
    ) {
        for source_side in [true, false] {
            let mut pending = lower.to_vec();
            for pair in &mut self.pairs {
                let child = if source_side {
                    pair.source.as_mut()
                } else {
                    Some(&mut pair.target)
                };
                let Some(child) = child else {
                    continue;
                };
                if child.pre_render_with_owned_lower(context, transform, &pending) {
                    pending.clear();
                }
                pending.push(child.clone());
            }
            let holder = if source_side {
                &mut self.source_texture
            } else {
                &mut self.target_texture
            };
            let texture = holder.get_or_init(context);
            let mut pass = context.begin_pass(
                texture.as_texture_target(),
                Some(context.depth_stencil),
                "PreparedSlotBatch/capture",
            );
            pass.clear(None, Some(0), None);
            let borrowed = pending.iter().map(|l| l as &dyn Layer).collect::<Vec<_>>();
            render_layers(&mut pass, transform, &borrowed, FloatColor4::BLACK, 0);
        }
    }
}
impl RenderClone for PreparedSlotBatch {
    fn render_clone(&self, context: &mut RenderCloneCtx) -> Self {
        // Native7f9d0 leaves the prepared vector and both captures empty. Copy
        // runs the real wipe clone body, while a normal retain shares its cell.
        Self {
            update_tracker: Default::default(),
            pairs: Vec::new(),
            wiper: self.wiper.render_clone(context),
            source_texture: RenderTextureHolder::new("PreparedSlotBatch/source"),
            target_texture: RenderTextureHolder::new("PreparedSlotBatch/target"),
        }
    }
}
impl AdvUpdatable for PreparedSlotBatch {
    fn update(&mut self, context: &AdvUpdateContext) {
        if !self.update_tracker.update(context.frame_id) {
            return;
        }
        // Native7ea20 updates direct source/target and wipe fields, not the
        // prepared pair vector. The prepared constructor leaves direct fields
        // null; semantic children advance through the graph exactly once.
        if let Some(wiper) = &mut self.wiper {
            wiper.update(context);
            if !wiper.is_running() {
                self.wiper = None;
                self.source_texture.clear();
                self.target_texture.clear();
            }
        }
    }
}
impl Layer for PreparedSlotBatch {
    fn needs_lower_capture(&self) -> bool {
        true
    }
    fn fast_forward(&mut self) {
        if let Some(wiper) = &mut self.wiper {
            wiper.fast_forward();
        }
    }
    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        self.prepare(context, transform, &[]);
    }
    fn pre_render_with_owned_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[RetainedLayer],
    ) -> bool {
        self.prepare(context, transform, lower);
        false
    }
    fn render(&self, pass: &mut RenderPass, _: &TransformParams, stencil_ref: u8, kind: PassKind) {
        if kind == PassKind::Opaque
            && let Some(wiper) = &self.wiper
        {
            wiper.render(
                pass,
                RenderRequestBuilder::new().depth_stencil_shorthand(stencil_ref, true, false),
                self.target_texture
                    .get()
                    .expect("prepared target capture")
                    .as_texture_source(),
                self.source_texture
                    .get()
                    .expect("prepared source capture")
                    .as_texture_source(),
            );
        }
    }
}
