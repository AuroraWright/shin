//! Stable ownership of actual mutable layers. Retaining never invokes RenderClone.
use std::sync::Arc;

use parking_lot::{
    ArcRwLockReadGuard, ArcRwLockWriteGuard, RawRwLock, RwLock, RwLockReadGuard, RwLockWriteGuard,
};
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
    layer: Arc<RwLock<T>>,
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
                layer: Arc::new(RwLock::new(layer)),
            }),
        }
    }
    pub(crate) fn read(&self) -> RwLockReadGuard<'_, T> {
        self.cell.layer.read()
    }
    pub(crate) fn write(&self) -> RwLockWriteGuard<'_, T> {
        self.cell.layer.write()
    }
    /// Keep both the cell and its read lock alive while traversing nested owners.
    pub(crate) fn read_owned(&self) -> LayerOwnerRead<T> {
        LayerOwnerRead {
            _owner: self.clone(),
            guard: self.cell.layer.read_arc(),
        }
    }
    pub(crate) fn write_owned(&self) -> LayerOwnerWrite<T> {
        LayerOwnerWrite {
            guard: self.cell.layer.write_arc(),
            _owner: self.clone(),
        }
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

pub(crate) struct LayerOwnerRead<T> {
    // The lock alone keeps T alive, but retaining the cell also preserves owner
    // identity and reference-count observations while this view is in use.
    // Drop the guard before the last cell reference.
    guard: ArcRwLockReadGuard<RawRwLock, T>,
    _owner: LayerOwner<T>,
}
impl<T> std::ops::Deref for LayerOwnerRead<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard
    }
}

pub(crate) struct LayerOwnerWrite<T> {
    guard: ArcRwLockWriteGuard<RawRwLock, T>,
    _owner: LayerOwner<T>,
}
impl<T> std::ops::Deref for LayerOwnerWrite<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard
    }
}
impl<T> std::ops::DerefMut for LayerOwnerWrite<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

/// Capturable targets use shared cells. The closed root Screen/Message container
/// uses direct values because it never installs an Effect or replacement wipe.
pub(crate) enum LayerStorage<T> {
    Direct(T),
    Shared(LayerOwner<T>),
}
pub(crate) enum LayerRead<'a, T> {
    Direct(&'a T),
    Shared(LayerOwnerRead<T>),
}
impl<T> std::ops::Deref for LayerRead<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        match self {
            Self::Direct(t) => t,
            Self::Shared(t) => t,
        }
    }
}
pub(crate) enum LayerWrite<'a, T> {
    Direct(&'a mut T),
    Shared(LayerOwnerWrite<T>),
}
impl<T> std::ops::Deref for LayerWrite<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        match self {
            Self::Direct(t) => t,
            Self::Shared(t) => t,
        }
    }
}
impl<T> std::ops::DerefMut for LayerWrite<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        match self {
            Self::Direct(t) => t,
            Self::Shared(t) => t,
        }
    }
}
impl<T> LayerStorage<T> {
    pub(crate) fn into_owner(self) -> LayerOwner<T> {
        match self {
            Self::Direct(t) => LayerOwner::new(t),
            Self::Shared(owner) => owner,
        }
    }
    pub(crate) fn shared_owner(&self) -> Option<LayerOwner<T>> {
        match self {
            Self::Direct(_) => None,
            Self::Shared(owner) => Some(owner.clone()),
        }
    }
    pub(crate) fn new_shared(t: T) -> Self {
        Self::Shared(LayerOwner::new(t))
    }
    pub(crate) fn read(&self) -> LayerRead<'_, T> {
        match self {
            Self::Direct(t) => LayerRead::Direct(t),
            Self::Shared(t) => LayerRead::Shared(t.read_owned()),
        }
    }
    pub(crate) fn write(&mut self) -> LayerWrite<'_, T> {
        match self {
            Self::Direct(t) => LayerWrite::Direct(t),
            Self::Shared(t) => LayerWrite::Shared(t.write_owned()),
        }
    }
    pub(crate) fn direct(&self) -> Option<&T> {
        match self {
            Self::Direct(t) => Some(t),
            Self::Shared(_) => None,
        }
    }
    pub(crate) fn direct_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Direct(t) => Some(t),
            Self::Shared(_) => None,
        }
    }
}
impl<T: RenderClone> RenderClone for LayerStorage<T> {
    fn render_clone(&self, context: &mut RenderCloneCtx) -> Self {
        match self {
            Self::Direct(t) => Self::Direct(t.render_clone(context)),
            Self::Shared(t) => Self::Shared(t.render_clone(context)),
        }
    }
}
impl<T: AdvUpdatable> AdvUpdatable for LayerStorage<T> {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.write().update(context);
    }
}
impl<T: Layer> Layer for LayerStorage<T> {
    fn fast_forward(&mut self) {
        self.write().fast_forward();
    }
    fn get_stencil_bump(&self) -> u8 {
        self.read().get_stencil_bump()
    }
    fn needs_lower_capture(&self) -> bool {
        self.read().needs_lower_capture()
    }
    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        self.write().pre_render(context, transform);
    }
    fn pre_render_with_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[&dyn Layer],
    ) -> bool {
        self.write()
            .pre_render_with_lower(context, transform, lower)
    }
    fn pre_render_with_owned_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[RetainedLayer],
    ) -> bool {
        self.write()
            .pre_render_with_owned_lower(context, transform, lower)
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        self.read().render(pass, transform, stencil_ref, pass_kind);
    }
}
pub(crate) struct LayerPropertyRead<'a, T>(LayerRead<'a, T>);
pub(crate) struct LayerPropertyWrite<'a, T>(LayerWrite<'a, T>);
impl<T: super::DrawableLayer> std::ops::Deref for LayerPropertyRead<'_, T> {
    type Target = super::LayerProperties;
    fn deref(&self) -> &Self::Target {
        self.0.properties()
    }
}
impl<T: super::DrawableLayer> std::ops::Deref for LayerPropertyWrite<'_, T> {
    type Target = super::LayerProperties;
    fn deref(&self) -> &Self::Target {
        self.0.properties()
    }
}
impl<T: super::DrawableLayer> std::ops::DerefMut for LayerPropertyWrite<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.properties_mut()
    }
}
impl<T: super::DrawableLayer> LayerStorage<T> {
    pub(crate) fn render_order_key(&self) -> (f32, i32) {
        self.read().render_order_key()
    }
    pub(crate) fn properties(&self) -> LayerPropertyRead<'_, T> {
        LayerPropertyRead(self.read())
    }
    pub(crate) fn properties_mut(&mut self) -> LayerPropertyWrite<'_, T> {
        LayerPropertyWrite(self.write())
    }
    pub(crate) fn set_properties(&mut self, properties: super::LayerProperties) {
        self.write().set_properties(properties);
    }
}

impl<T: AdvUpdatable> AdvUpdatable for LayerOwner<T> {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.write().update(context);
    }
}
impl<T: Layer> Layer for LayerOwner<T> {
    fn fast_forward(&mut self) {
        self.write().fast_forward();
    }
    fn get_stencil_bump(&self) -> u8 {
        self.read().get_stencil_bump()
    }
    fn needs_lower_capture(&self) -> bool {
        self.read().needs_lower_capture()
    }
    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        self.write().pre_render(context, transform);
    }
    fn pre_render_with_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[&dyn Layer],
    ) -> bool {
        self.write()
            .pre_render_with_lower(context, transform, lower)
    }
    fn pre_render_with_owned_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[RetainedLayer],
    ) -> bool {
        self.write()
            .pre_render_with_owned_lower(context, transform, lower)
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        self.read().render(pass, transform, stencil_ref, pass_kind);
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
    #[cfg(test)]
    fn reference_any(&self) -> &dyn std::any::Any;
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
impl<T: Layer + Send + Sync + 'static> Dispatch for Cell<T> {
    #[cfg(test)]
    fn reference_any(&self) -> &dyn std::any::Any {
        self
    }
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
    pub(crate) fn reference_read<T: 'static>(&self) -> Option<RwLockReadGuard<'_, T>> {
        self.cell
            .reference_any()
            .downcast_ref::<Cell<T>>()
            .map(|cell| cell.layer.read())
    }
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
