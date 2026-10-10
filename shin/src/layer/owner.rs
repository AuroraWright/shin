//! Stable ownership of actual mutable layers. Retaining never invokes RenderClone.
use std::sync::Arc;

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use shin_render::{
    PassKind,
    render_pass::RenderPass,
    shaders::types::{RenderClone, RenderCloneCtx},
};

use super::{Layer, render_params::TransformParams};
use crate::{
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext},
};

#[derive(Debug)]
struct Cell<T> {
    layer: RwLock<T>,
}

/// The graph and a deferred capture retain the same cell. Callers scope read and
/// write guards; a retained child cannot invalidate a later graph mutation.
#[derive(Debug)]
pub(crate) struct LayerOwner<T> {
    cell: Arc<Cell<T>>,
}

impl<T> Clone for LayerOwner<T> {
    fn clone(&self) -> Self {
        Self {
            cell: self.cell.clone(),
        }
    }
}
impl<T> LayerOwner<T> {
    pub(crate) fn new(layer: T) -> Self {
        Self {
            cell: Arc::new(Cell {
                layer: RwLock::new(layer),
            }),
        }
    }
    pub(crate) fn read(&self) -> RwLockReadGuard<'_, T> {
        self.cell.layer.read()
    }
    pub(crate) fn write(&self) -> RwLockWriteGuard<'_, T> {
        self.cell.layer.write()
    }
    #[cfg(test)]
    pub(crate) fn reference_strong_count(&self) -> usize {
        Arc::strong_count(&self.cell)
    }
    #[cfg(test)]
    pub(crate) fn reference_identity(&self) -> usize {
        Arc::as_ptr(&self.cell) as usize
    }
    #[cfg(test)]
    pub(crate) fn reference_weak(&self) -> impl Fn() -> usize + use<T> {
        let weak = Arc::downgrade(&self.cell);
        move || weak.strong_count()
    }
}
impl<T: RenderClone> RenderClone for LayerOwner<T> {
    fn render_clone(&self, context: &mut RenderCloneCtx) -> Self {
        Self::new(self.read().render_clone(context))
    }
}

// Mutable operations use the same cell as rendering. This erased interface
// retains a whole layer, including its mutable controller and prepared state.
trait Dispatch: Send + Sync {
    fn update(&self, context: &AdvUpdateContext);
    fn fast_forward(&self);
    fn get_stencil_bump(&self) -> u8;
    fn needs_lower_capture(&self) -> bool;
    fn pre_render(&self, context: &mut PreRenderContext, transform: &TransformParams);
    fn pre_render_with_lower(
        &self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[&dyn Layer],
    ) -> bool;
    fn pre_render_with_owned_lower(
        &self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[RetainedLayer],
    ) -> bool;
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    );
}
impl<T: Layer + Send + Sync> Dispatch for Cell<T> {
    fn update(&self, context: &AdvUpdateContext) {
        self.layer.write().update(context);
    }
    fn fast_forward(&self) {
        self.layer.write().fast_forward();
    }
    fn get_stencil_bump(&self) -> u8 {
        self.layer.read().get_stencil_bump()
    }
    fn needs_lower_capture(&self) -> bool {
        self.layer.read().needs_lower_capture()
    }
    fn pre_render(&self, context: &mut PreRenderContext, transform: &TransformParams) {
        self.layer.write().pre_render(context, transform);
    }
    fn pre_render_with_lower(
        &self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[&dyn Layer],
    ) -> bool {
        self.layer
            .write()
            .pre_render_with_lower(context, transform, lower)
    }
    fn pre_render_with_owned_lower(
        &self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[RetainedLayer],
    ) -> bool {
        self.layer
            .write()
            .pre_render_with_owned_lower(context, transform, lower)
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        self.layer
            .read()
            .render(pass, transform, stencil_ref, pass_kind);
    }
}

#[derive(Clone)]
pub(crate) struct RetainedLayer {
    cell: Arc<dyn Dispatch>,
}
impl std::fmt::Debug for RetainedLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetainedLayer")
            .field("owner", &Arc::as_ptr(&self.cell))
            .finish()
    }
}
impl<T: Layer + Send + Sync + 'static> LayerOwner<T> {
    pub(crate) fn retain(&self) -> RetainedLayer {
        RetainedLayer {
            cell: self.cell.clone(),
        }
    }
}
impl RetainedLayer {
    #[cfg(test)]
    pub(crate) fn reference_identity(&self) -> usize {
        Arc::as_ptr(&self.cell) as *const () as usize
    }
}
impl AdvUpdatable for RetainedLayer {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.cell.update(context);
    }
}
impl Layer for RetainedLayer {
    fn fast_forward(&mut self) {
        self.cell.fast_forward();
    }
    fn get_stencil_bump(&self) -> u8 {
        self.cell.get_stencil_bump()
    }
    fn needs_lower_capture(&self) -> bool {
        self.cell.needs_lower_capture()
    }
    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        self.cell.pre_render(context, transform);
    }
    fn pre_render_with_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[&dyn Layer],
    ) -> bool {
        self.cell.pre_render_with_lower(context, transform, lower)
    }
    fn pre_render_with_owned_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[RetainedLayer],
    ) -> bool {
        self.cell
            .pre_render_with_owned_lower(context, transform, lower)
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        self.cell.render(pass, transform, stencil_ref, pass_kind);
    }
}
