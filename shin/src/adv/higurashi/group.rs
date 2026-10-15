//! Older screen group: priority/owner-ID ordering and opposite pass traversals.
//! Slot storage order is only the stable input order (native 6d200/6e340).
use anyhow::{Result, ensure};
use shin_render::{PassKind, render_pass::RenderPass};

use crate::layer::{
    higurashi_properties::HigurashiProperties, render_params::higurashi::HigurashiTransform,
    user::higurashi::HigurashiUserLayer,
};

#[derive(Clone, Copy)]
pub struct Child<'a> {
    pub owner_id: i32,
    pub slot: u8,
    pub layer: &'a HigurashiUserLayer,
}

/// Temporary native layers use the same group ordering and traversal as the
/// ordinary retained drawables,without changing their resource ownership.
pub trait Drawable {
    fn properties(&self) -> &HigurashiProperties;
    fn render(
        &self,
        pass: &mut RenderPass,
        parent: &HigurashiTransform,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()>;
}
impl Drawable for HigurashiUserLayer {
    fn properties(&self) -> &HigurashiProperties {
        &self.properties
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        parent: &HigurashiTransform,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        HigurashiUserLayer::render(self, pass, parent, stencil, kind)
    }
}
#[derive(Clone, Copy)]
pub struct DrawableChild<'a> {
    pub owner_id: i32,
    pub slot: u8,
    pub layer: &'a dyn Drawable,
}

pub fn ordered(mut children: Vec<Child<'_>>) -> Vec<Child<'_>> {
    sort(&mut children, |child| {
        (child.layer.properties.values()[5], child.owner_id)
    });
    children
}

/// Native6cc80 prepares sorted lower prefixes before the ordinary two passes.
/// A completed effect replaces its captured prefix for the next effect.
pub fn prepare_children(
    context: &mut crate::render::PreRenderContext,
    children: Vec<Child<'_>>,
    properties: &HigurashiProperties,
    parent: &HigurashiTransform,
    depth: &crate::layer::user::higurashi::effect_capture::SharedDepth,
) -> Result<()> {
    let children = ordered(children);
    let transform = properties.composed_transform(parent);
    let mut lower = Vec::new();
    for child in children {
        if child
            .layer
            .prepare_lower(context, depth, &lower, &transform)?
        {
            lower.clear();
        }
        lower.push(DrawableChild {
            owner_id: child.owner_id,
            slot: child.slot,
            layer: child.layer,
        });
    }
    Ok(())
}

pub fn ordered_drawables(mut children: Vec<DrawableChild<'_>>) -> Vec<DrawableChild<'_>> {
    sort(&mut children, |child| {
        (child.layer.properties().values()[5], child.owner_id)
    });
    children
}
fn sort<T>(children: &mut [T], metadata: impl Fn(&T) -> (f32, i32)) {
    children.sort_by(|a, b| {
        let (pa, id_a) = metadata(a);
        let (pb, id_b) = metadata(b);
        if pa == pb {
            id_a.cmp(&id_b)
        } else {
            pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
        }
    });
}
fn traversal(count: usize) -> impl Iterator<Item = (usize, u8, PassKind)> {
    (0..count)
        .rev()
        .map(|i| (i, (i + 1) as u8, PassKind::Opaque))
        .chain((0..count).map(|i| (i, (i + 1) as u8, PassKind::Transparent)))
}

pub struct Draw<'a> {
    pub child: Child<'a>,
    /// Effective low byte from native697f0; ordinary child reserves are1.
    /// Native686d8 accumulates a u32 before converting at that endpoint.
    pub stencil: u8,
    pub pass: PassKind,
}

pub fn render_plan(children: Vec<Child<'_>>) -> Result<Vec<Draw<'_>>> {
    let children = ordered(children);
    let mut plan = Vec::with_capacity(children.len() * 2);
    for (i, stencil, pass) in traversal(children.len()) {
        plan.push(Draw {
            child: children[i],
            stencil,
            pass,
        });
    }
    Ok(plan)
}

pub fn render(
    pass: &mut RenderPass,
    children: Vec<Child<'_>>,
    properties: &HigurashiProperties,
    parent: &HigurashiTransform,
) -> Result<()> {
    render_drawables(
        pass,
        children
            .into_iter()
            .map(|child| DrawableChild {
                owner_id: child.owner_id,
                slot: child.slot,
                layer: child.layer,
            })
            .collect(),
        properties,
        parent,
    )
}

pub fn render_drawables(
    pass: &mut RenderPass,
    children: Vec<DrawableChild<'_>>,
    properties: &HigurashiProperties,
    parent: &HigurashiTransform,
) -> Result<()> {
    let p = properties.values();
    // Native 6aaec applies the group's color/shader after child rendering.
    // The initial authored scene uses the identity operation; other group
    // effects need their texture postprocessing stage before being accepted.
    ensure!(
        p[6..10] == [1000.0; 4] && p[20] == 0.0 && p[21] == 0.0,
        "Higurashi group color/shader postprocessing is not implemented yet"
    );
    // Actual6d200 and cachedGroup6dfc4/Screenba554 do not sample44..47.
    if !properties.is_visible() {
        return Ok(());
    }
    let transform = properties.composed_transform(parent);
    render_transformed_children(pass, children, &transform)
}

/// The framebuffer's internal6d200 traversal does not test this group's
/// external visibility/opacity. Color processing follows the completed image.
pub fn render_capture_drawables(
    pass: &mut RenderPass,
    children: Vec<DrawableChild<'_>>,
    properties: &HigurashiProperties,
    parent: &HigurashiTransform,
) -> Result<()> {
    // Actual6d200 does not sample group44..47 or prepare a leaf clip mask.
    render_transformed_children(pass, children, &properties.composed_transform(parent))
}
pub(crate) fn render_capture(
    pass: &mut RenderPass,
    children: Vec<Child<'_>>,
    properties: &HigurashiProperties,
    parent: &HigurashiTransform,
) -> Result<()> {
    render_capture_drawables(
        pass,
        children
            .into_iter()
            .map(|child| DrawableChild {
                owner_id: child.owner_id,
                slot: child.slot,
                layer: child.layer,
            })
            .collect(),
        properties,
        parent,
    )
}

fn render_transformed_children(
    pass: &mut RenderPass,
    children: Vec<DrawableChild<'_>>,
    transform: &HigurashiTransform,
) -> Result<()> {
    render_ordered_drawables(pass, &ordered_drawables(children), transform)
}

/// Original686d8 draws an already sorted lower prefix. Effect captures replace
/// each completed prefix with its effect owner before the next capture.
pub fn render_ordered_drawables(
    pass: &mut RenderPass,
    children: &[DrawableChild<'_>],
    transform: &HigurashiTransform,
) -> Result<()> {
    // Native 686d8 assigns monotonically increasing stencil references from1,
    // visits opaque children in reverse, then transparent children forwards.
    for (i, stencil, kind) in traversal(children.len()) {
        children[i].layer.render(pass, transform, stencil, kind)?;
    }
    Ok(())
}

#[cfg(test)]
mod stencil_endpoint_tests;
#[cfg(test)]
mod temporal_native_tests;
#[cfg(test)]
mod tests;
