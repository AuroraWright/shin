#[cfg(test)]
mod delayed_tests;
#[cfg(test)]
mod native_copy_tests;
mod transition;
#[cfg(test)]
mod update_tests;
use crate::wiper::AnyWiper;
use std::sync::Arc;
use transition::SlotLayer;

use glam::{Mat4, Vec3, Vec3Swizzles as _, vec2, vec3};
use shin_core::{
    primitives::color::{FloatColor4, UnormColor},
    time::Ticks,
    vm::command::types::{LayerProperty, LayerbankId, MaskFlags},
};
use shin_render::{
    ColorBlendType, DepthStencilState, DrawPrimitive, LayerShaderOutputKind, PassKind,
    RenderProgramWithArguments, RenderRequestBuilder, StencilFunction, StencilMask,
    StencilOperation, StencilPipelineState, StencilState,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{
        RenderClone, RenderCloneCtx,
        buffer::VertexSource,
        texture::{DepthStencilTarget, TextureTarget},
        vertices::{MaskVertex, PosTexVertex, PosVertex},
    },
};

use crate::{
    asset::mask::{MaskRenderFlags, MaskTexture},
    layer::{
        DrawableLayer, Layer, NewDrawableLayer, UserLayer,
        new_drawable_layer::{
            NewDrawableLayerNeedsSeparatePass, NewDrawableLayerState, PrerenderedDrawable,
        },
        properties::LayerProperties,
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
    },
    render::{PreRenderContext, VIRTUAL_CANVAS_SIZE_VEC, top_left_projection_matrix},
    update::{AdvUpdatable, AdvUpdateContext},
};

#[derive(RenderClone)]
struct LayerItem<T> {
    // None represents native child ID -1: retained for rendering and updating,
    // but no longer reachable through its original layer bank.
    pub layerbank_id: Option<LayerbankId>,
    removal_delay: Ticks,
    #[render_clone(needs_render)]
    pub layer: SlotLayer<T>,
}

#[derive(Clone, Copy)]
struct LayerRenderItem {
    // TODO: maybe we should make the layers actually shareable with `Arc`s, yknow...
    pub layer_index: usize,
    pub stencil_ref_relative: u8,
}

pub struct LayerGroup<T = UserLayer> {
    update_tracker: crate::layer::update_tracker::LayerUpdateTracker,
    layers: Vec<LayerItem<T>>,

    stencil_bump: u8,
    layers_to_render: Vec<LayerRenderItem>,

    new_drawable_state: NewDrawableLayerState,
    mask_texture: Option<Arc<MaskTexture>>,
    mask_flags: MaskFlags,
    props: LayerProperties,
    label: String,
}

impl<T: DrawableLayer + RenderClone> RenderClone for LayerGroup<T> {
    fn render_clone(&self, context: &mut RenderCloneCtx) -> Self {
        // Native79cb8 copies the drawable base before the children at79d34.
        // Slot copies can initialize properties lazily, consuming RNG draws;
        // copying the parent's prepared properties first preserves their seeds.
        let props = self.props.clone();
        let new_drawable_state = self.new_drawable_state.render_clone(context);
        let layers = self.layers.render_clone(context);
        Self {
            update_tracker: self.update_tracker.clone(),
            layers,
            stencil_bump: self.stencil_bump,
            // Native79ce8/79cec leave the prepared render list empty.
            layers_to_render: Vec::new(),
            new_drawable_state,
            mask_texture: self.mask_texture.clone(),
            mask_flags: self.mask_flags,
            props,
            label: self.label.clone(),
        }
    }
}

impl<T> LayerGroup<T> {
    fn pre_render_slot_batch(
        &mut self,
        indices: &[usize],
        lower: &[usize],
        context: &mut PreRenderContext,
        transform: &TransformParams,
    ) where
        T: Layer,
    {
        // Native7ca30/806b0 retain pairs in a transient render batch. The
        // semantic children stay in this group; borrow them for the same two
        // captures and store the result in the first slot's scratch holders.
        for source_side in [true, false] {
            let mut pending: Vec<(usize, Option<bool>)> =
                lower.iter().map(|&i| (i, None)).collect();
            for &index in indices {
                if self.layers[index].layer.batch_layer(source_side).is_none() {
                    continue;
                }
                let (left, current_and_right) = self.layers.split_at_mut(index);
                let (current, right) = current_and_right.split_first_mut().unwrap();
                let lower = pending
                    .iter()
                    .filter_map(|&(i, side)| {
                        let item = if i < index {
                            &left[i]
                        } else {
                            &right[i - index - 1]
                        };
                        side.map_or(Some(&item.layer as &dyn Layer), |s| {
                            item.layer.batch_layer(s)
                        })
                    })
                    .collect::<Vec<_>>();
                let consumed =
                    current
                        .layer
                        .pre_render_batch_layer(source_side, context, transform, &lower);
                if consumed {
                    pending.clear();
                }
                pending.push((index, Some(source_side)));
            }
            let index = indices[0];
            let (left, current_and_right) = self.layers.split_at_mut(index);
            let (current, right) = current_and_right.split_first_mut().unwrap();
            let SlotLayer {
                source,
                target,
                wiper,
                source_texture,
                target_texture,
                ..
            } = &mut current.layer;
            let layers = pending
                .iter()
                .filter_map(|&(i, side)| {
                    if i == index {
                        let source_side = side.unwrap();
                        if source_side && wiper.is_some() {
                            source.as_deref().map(|s| s as &dyn Layer)
                        } else {
                            Some(&*target as &dyn Layer)
                        }
                    } else {
                        let item = if i < index {
                            &left[i]
                        } else {
                            &right[i - index - 1]
                        };
                        side.map_or(Some(&item.layer as &dyn Layer), |s| {
                            item.layer.batch_layer(s)
                        })
                    }
                })
                .collect::<Vec<_>>();
            let holder = if source_side {
                source_texture
            } else {
                target_texture
            };
            let texture = holder.get_or_init(context);
            let mut pass = context.begin_pass(
                texture.as_texture_target(),
                Some(context.depth_stencil),
                "SlotLayer/batch",
            );
            pass.clear(None, Some(0), None);
            crate::layer::render_layers(&mut pass, transform, &layers, FloatColor4::BLACK, 0);
        }
    }

    #[cfg(test)]
    pub(crate) fn reference_owner_leaves(&self) -> Vec<&T> {
        let mut leaves = Vec::new();
        for item in &self.layers {
            item.layer.reference_owner_leaves(&mut leaves);
        }
        leaves
    }
    #[cfg(test)]
    pub(crate) fn reference_owner_entry_words(&self) -> Vec<(i32, u32, bool)> {
        self.layers
            .iter()
            .map(|entry| {
                let bank = entry.layerbank_id.map_or(-1, |b| i32::from(b.raw()));
                (
                    bank,
                    if bank < 0 {
                        entry.removal_delay.as_f32().to_bits()
                    } else {
                        0
                    },
                    entry.layer.wiper.is_some(),
                )
            })
            .collect()
    }
    #[cfg(test)]
    pub(crate) fn reference_transition_words(&self) -> Vec<[u32; 7]> {
        let mut words = Vec::new();
        for item in &self.layers {
            item.layer.reference_transition_words(&mut words);
        }
        words
    }
    #[cfg(test)]
    pub(crate) fn reference_advance_transition_clocks(&mut self, delta: f32) {
        for item in &mut self.layers {
            item.layer.reference_advance_clocks(delta);
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_prepare_transition_textures(
        &mut self,
        device: &wgpu::Device,
        resize: &shin_render::resize::SurfaceResizeSource,
    ) -> usize {
        self.layers
            .iter_mut()
            .map(|i| i.layer.reference_prepare_textures(device, resize))
            .sum()
    }
    #[cfg(test)]
    pub(crate) fn reference_dissolve_seed(&self) -> u32 {
        self.new_drawable_state.reference_dissolve_seed()
    }
    #[cfg(test)]
    pub(crate) fn reference_effect_phases(&self) -> [f32; 3] {
        self.new_drawable_state.reference_effect_phases()
    }
    #[cfg(test)]
    pub(crate) fn reference_retained_layer_count(&self) -> usize {
        self.layers
            .iter()
            .filter(|item| item.layerbank_id.is_none())
            .count()
    }
    pub fn new(label: Option<String>) -> Self {
        Self {
            update_tracker: Default::default(),
            layers: vec![],
            stencil_bump: 0,
            layers_to_render: vec![],
            new_drawable_state: NewDrawableLayerState::new(),
            mask_texture: None,
            mask_flags: MaskFlags::empty(),
            props: LayerProperties::new(),
            label: label.unwrap_or_else(|| "unnamed".to_string()),
        }
    }

    pub fn add_layer(&mut self, layerbank_id: LayerbankId, layer: T) {
        self.add_layer_with_wiper(layerbank_id, layer, None);
    }

    /// Native 7a6e0 retains the previous slot beneath a replacement wiper.
    pub fn add_layer_with_wiper(
        &mut self,
        layerbank_id: LayerbankId,
        layer: T,
        wiper: Option<AnyWiper>,
    ) {
        let (index, source) = match self
            .layers
            .binary_search_by_key(&Some(layerbank_id), |item| item.layerbank_id)
        {
            Ok(index) => (index, Some(self.layers.remove(index).layer)),
            Err(index) => (index, None),
        };
        self.layers.insert(
            index,
            LayerItem {
                layerbank_id: Some(layerbank_id),
                removal_delay: Ticks::ZERO,
                layer: SlotLayer::new(layer, source, wiper),
            },
        );
    }

    /// Immediate removal returns the target. Delayed removal retains the whole
    /// slot under native ID -1 and returns None; its old bank is free for reuse.
    pub fn remove_layer(&mut self, layerbank_id: LayerbankId, delay_time: Ticks) -> Option<T> {
        let index = self
            .layers
            .binary_search_by_key(&Some(layerbank_id), |item| item.layerbank_id)
            .ok()?;
        let mut removed = self.layers.remove(index);
        if delay_time == Ticks::ZERO {
            Some(removed.layer.target)
        } else {
            // Native7b510..7b520 stores ID -1 and the float countdown, then
            // 7b6e4..7b6f4 inserts the retained entry at the vector beginning.
            removed.layerbank_id = None;
            removed.removal_delay = delay_time;
            self.layers.insert(0, removed);
            None
        }
    }

    pub fn get_layer(&self, layerbank_id: LayerbankId) -> Option<&T> {
        self.layers
            .binary_search_by_key(&Some(layerbank_id), |item| item.layerbank_id)
            .map(|index| &self.layers[index].layer.target)
            .ok()
    }

    pub fn get_layer_mut(&mut self, layerbank_id: LayerbankId) -> Option<&mut T> {
        self.layers
            .binary_search_by_key(&Some(layerbank_id), |item| item.layerbank_id)
            .map(|index| &mut self.layers[index].layer.target)
            .ok()
    }

    pub fn get_used_layerbank_ids(&self) -> Vec<LayerbankId> {
        self.layers
            .iter()
            .filter_map(|item| item.layerbank_id)
            .collect()
    }

    pub fn clear_layers(&mut self) {
        self.layers.clear();
    }

    #[cfg(test)]
    pub(crate) fn reference_slot_properties(
        &self,
        bank: LayerbankId,
        property: LayerProperty,
    ) -> Vec<(usize, f32)>
    where
        T: DrawableLayer,
    {
        let mut output = Vec::new();
        if let Ok(index) = self
            .layers
            .binary_search_by_key(&Some(bank), |item| item.layerbank_id)
        {
            self.layers[index]
                .layer
                .property_trace(property, &mut output);
        }
        output
    }

    pub fn has_wiper_for_layerbank(&self, layerbank_id: LayerbankId) -> bool {
        self.layers
            .binary_search_by_key(&Some(layerbank_id), |item| item.layerbank_id)
            .is_ok_and(|index| self.layers[index].layer.is_transition_active())
    }

    pub fn set_mask_texture(&mut self, mask_texture: Arc<MaskTexture>, flags: MaskFlags) {
        self.mask_texture = Some(mask_texture);
        self.mask_flags = flags;
    }

    pub fn clear_mask_texture(&mut self) {
        self.mask_texture = None;
    }

    #[cfg(test)]
    pub(crate) fn reference_mask_state(&self) -> (Option<&Arc<MaskTexture>>, MaskFlags) {
        (self.mask_texture.as_ref(), self.mask_flags)
    }

    #[allow(clippy::len_zero)]
    pub fn needs_rendering(&self) -> bool {
        self.mask_texture.is_some() || self.layers.len() > 0
    }

    pub fn is_rendered_opaquely(&self) -> bool {
        self.new_drawable_state.is_rendered_opaquely(
            &self.props,
            &LayerGroupNewDrawableNeedsSeparatePassDelegate {
                has_mask_texture: self.mask_texture.is_some(),
            },
        )
    }
}

fn render_mask(
    pass: &mut RenderPass,
    builder: RenderRequestBuilder,
    mask: &MaskTexture,
    mask_flags: MaskFlags,
    transform: TransformParams,
    mask_render_flags: MaskRenderFlags,
) {
    pass.push_debug(&format!(
        "LayerGroup/mask[{:?}, {:?}]",
        mask_flags, mask_render_flags
    ));
    let mut scale = vec2(1.0, 1.0);

    if mask_flags.contains(MaskFlags::FLIP_X) {
        scale.x *= -1.0;
    }
    if mask_flags.contains(MaskFlags::FLIP_Y) {
        scale.y *= -1.0;
    }

    let mask_size = mask.texture.size_vec();

    if mask_flags.contains(MaskFlags::SCALE) {
        scale *= VIRTUAL_CANVAS_SIZE_VEC / mask_size;
    }

    let transform = transform.compute_final_transform()
        * Mat4::from_scale(scale.extend(1.0))
        * Mat4::from_translation(-mask_size.extend(0.0) * 0.5);

    mask.render(
        pass,
        builder.color_blend_type(ColorBlendType::NoColor),
        transform,
        mask_render_flags,
    );
    pass.pop_debug();
}

fn prepare_mask_for_offscreen(
    pass: &mut RenderPass,
    mask: &MaskTexture,
    mask_flags: MaskFlags,
    transform: TransformParams,
) {
    pass.push_debug("LayerGroup/prepare_mask1");

    pass.clear(Some(UnormColor::BLACK), Some(0x3f), None);

    let mask_render_flags = if mask_flags.contains(MaskFlags::INVERT) {
        MaskRenderFlags::BLACK | MaskRenderFlags::TRANSPARENT
    } else {
        MaskRenderFlags::WHITE | MaskRenderFlags::TRANSPARENT
    };

    let builder = RenderRequestBuilder::new().depth_stencil(DepthStencilState {
        stencil: StencilState {
            pipeline: StencilPipelineState {
                function: StencilFunction::Always,
                stencil_fail_operation: StencilOperation::Keep,
                depth_fail_operation: StencilOperation::Keep,
                pass_operation: StencilOperation::Replace,
                stencil_read_mask: StencilMask::All,
                stencil_write_mask: StencilMask::All,
            },
            stencil_reference: 0,
        },
        ..DepthStencilState::default()
    });

    render_mask(
        pass,
        builder,
        mask,
        mask_flags,
        transform,
        mask_render_flags,
    );
    pass.pop_debug();
}

fn prepare_mask_for_onscreen(
    pass: &mut RenderPass,
    mask: &MaskTexture,
    mask_flags: MaskFlags,
    transform: TransformParams,
    render_opaque: bool,
    render_transparent: bool,
) {
    pass.push_debug("LayerGroup/prepare_mask_for_onscreen");
    let mut render_flags = MaskRenderFlags::empty();

    if render_opaque {
        if mask_flags.contains(MaskFlags::INVERT) {
            render_flags |= MaskRenderFlags::BLACK;
        } else {
            render_flags |= MaskRenderFlags::WHITE;
        };
    }
    if render_transparent {
        render_flags |= MaskRenderFlags::TRANSPARENT;
    }

    let builder = RenderRequestBuilder::new().depth_stencil(DepthStencilState {
        stencil: StencilState {
            pipeline: StencilPipelineState {
                function: StencilFunction::Always,
                stencil_fail_operation: StencilOperation::Keep,
                depth_fail_operation: StencilOperation::Keep,
                pass_operation: StencilOperation::Replace,
                stencil_read_mask: StencilMask::All,
                stencil_write_mask: StencilMask::SignOnly,
            },
            stencil_reference: 0x80,
        },
        ..DepthStencilState::default()
    });

    render_mask(pass, builder, mask, mask_flags, transform, render_flags);

    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::NoColor)
            .depth_stencil(DepthStencilState {
                stencil: StencilState {
                    pipeline: StencilPipelineState {
                        function: StencilFunction::Always,
                        stencil_fail_operation: StencilOperation::Keep,
                        depth_fail_operation: StencilOperation::Keep,
                        pass_operation: StencilOperation::Invert,
                        stencil_read_mask: StencilMask::SignOnly,
                        stencil_write_mask: StencilMask::SignOnly,
                    },
                    stencil_reference: 0x80,
                },
                ..DepthStencilState::default()
            })
            .build(
                RenderProgramWithArguments::Clear {
                    vertices: VertexSource::VertexData {
                        vertices: &[
                            PosVertex {
                                position: vec3(-1.0, 1.0, 0.0),
                            },
                            PosVertex {
                                position: vec3(3.0, 1.0, 0.0),
                            },
                            PosVertex {
                                position: vec3(-1.0, -3.0, 0.0),
                            },
                        ],
                    },
                    color: FloatColor4::BLACK,
                },
                DrawPrimitive::Triangles,
            ),
    );
    pass.pop_debug();
}

fn finish_mask_for_onscreen(pass: &mut RenderPass) {
    pass.push_debug("LayerGroup/finish_mask_for_onscreen");
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::NoColor)
            .depth_stencil(DepthStencilState {
                stencil: StencilState {
                    pipeline: StencilPipelineState {
                        function: StencilFunction::Always,
                        stencil_fail_operation: StencilOperation::Keep,
                        depth_fail_operation: StencilOperation::Keep,
                        pass_operation: StencilOperation::Zero,
                        stencil_read_mask: StencilMask::SignOnly,
                        stencil_write_mask: StencilMask::SignOnly,
                    },
                    stencil_reference: 0x80,
                },
                ..DepthStencilState::default()
            })
            .build(
                RenderProgramWithArguments::Clear {
                    vertices: VertexSource::VertexData {
                        vertices: &[
                            PosVertex {
                                position: vec3(-1.0, 1.0, 0.0),
                            },
                            PosVertex {
                                position: vec3(3.0, 1.0, 0.0),
                            },
                            PosVertex {
                                position: vec3(-1.0, -3.0, 0.0),
                            },
                        ],
                    },
                    color: FloatColor4::BLACK,
                },
                DrawPrimitive::Triangles,
            ),
    );
    pass.pop_debug();
}

struct LayerGroupNewDrawableNeedsSeparatePassDelegate {
    has_mask_texture: bool,
}

impl NewDrawableLayerNeedsSeparatePass for LayerGroupNewDrawableNeedsSeparatePassDelegate {
    fn needs_separate_pass(&self, props: &LayerProperties) -> bool {
        if self.has_mask_texture {
            return true;
        }

        props.get_clip_mode() != DrawableClipMode::None
            || props.is_fragment_shader_nontrivial()
            || props.is_blending_nontrivial()
    }
}

struct LayerGroupNewDrawableDelegate<'a, T> {
    layers: &'a [LayerItem<T>],
    layers_to_render: Vec<LayerRenderItem>,
    mask_texture: &'a Option<Arc<MaskTexture>>,
    mask_flags: MaskFlags,
}

impl<T> NewDrawableLayerNeedsSeparatePass for LayerGroupNewDrawableDelegate<'_, T> {
    fn needs_separate_pass(&self, props: &LayerProperties) -> bool {
        LayerGroupNewDrawableNeedsSeparatePassDelegate {
            has_mask_texture: self.mask_texture.is_some(),
        }
        .needs_separate_pass(props)
    }
}

impl<T> NewDrawableLayer for LayerGroupNewDrawableDelegate<'_, T>
where
    T: Layer,
{
    #[tracing::instrument(skip_all)]
    fn render_drawable_indirect(
        &mut self,
        context: &mut PreRenderContext,
        props: &LayerProperties,
        target: TextureTarget,
        depth_stencil: DepthStencilTarget,
        transform: &TransformParams,
    ) -> PassKind {
        let mut pass = context.begin_pass(target, Some(depth_stencil), "LayerGroup/indirect");

        if !props.is_visible() {
            pass.clear(Some(UnormColor::BLACK), None, None);
        } else {
            let self_transform = props.get_composed_transform_params(transform);

            if let Some(mask) = self.mask_texture {
                prepare_mask_for_offscreen(&mut pass, mask, self.mask_flags, self_transform);
            } else {
                pass.clear(Some(UnormColor::BLACK), Some(0), None);
            }

            for render_item in self.layers_to_render.iter().rev() {
                self.layers[render_item.layer_index].layer.render(
                    &mut pass,
                    &self_transform,
                    render_item.stencil_ref_relative,
                    PassKind::Opaque,
                );
            }
            for render_item in &self.layers_to_render {
                self.layers[render_item.layer_index].layer.render(
                    &mut pass,
                    &self_transform,
                    render_item.stencil_ref_relative,
                    PassKind::Transparent,
                );
            }
        }

        self.layers_to_render.clear();

        // it's very sus that it does that, considering you can apply alpha and stuff...
        // but I guess that's just how planes are...
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
    ) {
        // direct rendering is always done by the LayerGroup without relying on NewDrawableLayer
        unreachable!()
    }
}

impl<T> AdvUpdatable for LayerGroup<T>
where
    T: AdvUpdatable,
{
    fn update(&mut self, context: &AdvUpdateContext) {
        if !self.update_tracker.update(context.frame_id) {
            return;
        }
        self.props.update(context);
        self.new_drawable_state.update(context, &self.props);

        for layer in &mut self.layers {
            layer.layer.update(context);
        }

        // Native7bd44..7bdd0 advances timers after every child update. A
        // finished timer keeps its transition wrapper while the wipe is alive.
        self.layers.retain_mut(|item| {
            if item.layerbank_id.is_some() {
                return true;
            }
            item.removal_delay = Ticks::from_f32(
                (item.removal_delay.as_f32() - context.delta_ticks.as_f32()).max(0.0),
            );
            item.removal_delay.as_f32().abs() >= f32::EPSILON || item.layer.is_transition_active()
        });
    }
}

impl<T> Layer for LayerGroup<T>
where
    T: DrawableLayer,
{
    fn fast_forward(&mut self) {
        self.props.fast_forward();

        for layer in self.layers.iter_mut() {
            layer.layer.fast_forward();
        }
        // Native7baf4..7bc68 removes every negative-ID entry after forwarding
        // all children, regardless of countdown or retained wipe identity.
        self.layers.retain(|item| item.layerbank_id.is_some());
    }

    fn get_stencil_bump(&self) -> u8 {
        self.stencil_bump
    }

    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        let mut layers = (0..self.layers.len()).collect::<Vec<_>>();

        layers.sort_by(|&left, &right| {
            let get_values = |index: usize| {
                let layer = &self.layers[index];
                // Native comparator81274/81348 reads drawable properties+20,
                // not the group's storage bank. Non-drawable root children
                // use native position/ID sentinels instead.
                layer.layer.render_order_key()
            };

            let (left_position, left_id) = get_values(left);
            let (right_position, right_id) = get_values(right);

            // if positions are close, compare by id
            if (left_position - right_position).abs() < f32::EPSILON {
                left_id.cmp(&right_id)
            } else {
                left_position
                    .partial_cmp(&right_position)
                    .unwrap()
                    .reverse()
            }
        });

        let self_transform = self.properties().get_composed_transform_params(transform);
        let mut pending: Vec<usize> = Vec::new();
        let mut position = 0;
        while position < layers.len() {
            let index = layers[position];
            let mut end = position + 1;
            while end < layers.len()
                && self.layers[index]
                    .layer
                    .can_batch_with(&self.layers[layers[end]].layer)
            {
                end += 1;
            }
            if end > position + 1 {
                self.pre_render_slot_batch(
                    &layers[position..end],
                    &pending,
                    context,
                    &self_transform,
                );
                pending.push(index);
                position = end;
                continue;
            }
            position += 1;
            // Native7c540..7c558 only supplies the batch to Effectable layers.
            // Ordinary authored scenes should not allocate every prefix here.
            if !self.layers[index].layer.needs_lower_capture() {
                self.layers[index]
                    .layer
                    .pre_render(context, &self_transform);
                pending.push(index);
                continue;
            }
            // Split by storage index while retaining the native rendering order.
            // Lower siblings are borrowed: captures never advance them a second time.
            let (left, current_and_right) = self.layers.split_at_mut(index);
            let (current, right) = current_and_right.split_first_mut().unwrap();
            let lower = pending
                .iter()
                .map(|&lower_index| {
                    let item = if lower_index < index {
                        &left[lower_index]
                    } else {
                        &right[lower_index - index - 1]
                    };
                    &item.layer as &dyn Layer
                })
                .collect::<Vec<_>>();
            let consumed = current
                .layer
                .pre_render_with_lower(context, &self_transform, &lower);
            if consumed {
                pending.clear();
            }
            pending.push(index);
        }

        self.layers_to_render.clear();
        let mut layers_to_render = std::mem::take(&mut self.layers_to_render);

        let mut stencil_value = 1;
        for &index in &pending {
            layers_to_render.push(LayerRenderItem {
                layer_index: index,
                stencil_ref_relative: stencil_value,
            });

            stencil_value += self.layers[index].layer.get_stencil_bump();
        }

        self.stencil_bump = stencil_value;

        let mut delegate = LayerGroupNewDrawableDelegate {
            layers: &self.layers,
            layers_to_render,
            mask_texture: &self.mask_texture,
            mask_flags: self.mask_flags,
        };

        // NOTE: we DON'T send self_transform to the pre-drawing
        // this is because the `render_drawable_indirect` will apply the transform itself
        self.new_drawable_state
            .pre_render(context, &self.props, &mut delegate, transform);

        self.layers_to_render = delegate.layers_to_render;
    }

    #[tracing::instrument(skip_all)]
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if let Some(tex) = self.new_drawable_state.get_prerendered_tex() {
            if let Some(mask) = &self.mask_texture {
                self.finish_render_with_mask(
                    pass,
                    tex,
                    mask,
                    self.mask_flags,
                    transform,
                    stencil_ref,
                    pass_kind,
                );
                return;
            } else {
                self.new_drawable_state.try_finish_indirect_render(
                    &self.props,
                    pass,
                    transform,
                    stencil_ref,
                    pass_kind,
                );
                return;
            }
        }

        let props = &self.props;
        if !props.is_visible() {
            return;
        }

        let self_transform = props.get_composed_transform_params(transform);

        pass.push_debug(&format!(
            "LayerGroup[{}]/{}",
            &self.label,
            match pass_kind {
                PassKind::Opaque => {
                    "opaque"
                }
                PassKind::Transparent => {
                    "transparent"
                }
            }
        ));

        match pass_kind {
            PassKind::Opaque => {
                for &LayerRenderItem {
                    layer_index,
                    stencil_ref_relative,
                } in self.layers_to_render.iter().rev()
                {
                    self.layers[layer_index].layer.render(
                        pass,
                        &self_transform,
                        stencil_ref + stencil_ref_relative,
                        pass_kind,
                    )
                }

                let vertices = &[
                    PosVertex {
                        position: vec3(-1.0, 1.0, 0.0),
                    },
                    PosVertex {
                        position: vec3(3.0, 1.0, 0.0),
                    },
                    PosVertex {
                        position: vec3(-1.0, -3.0, 0.0),
                    },
                ];

                pass.push_debug("clear");
                pass.run(
                    RenderRequestBuilder::new()
                        .depth_stencil(DepthStencilState {
                            depth: Default::default(),
                            stencil: StencilState {
                                pipeline: StencilPipelineState {
                                    function: StencilFunction::Greater,
                                    stencil_fail_operation: StencilOperation::Keep,
                                    depth_fail_operation: StencilOperation::Keep,
                                    pass_operation: StencilOperation::Replace,
                                    stencil_read_mask: StencilMask::All,
                                    stencil_write_mask: StencilMask::All,
                                },
                                stencil_reference: stencil_ref,
                            },
                        })
                        .build(
                            RenderProgramWithArguments::Clear {
                                vertices: VertexSource::VertexData { vertices },
                                color: FloatColor4::BLACK,
                            },
                            DrawPrimitive::Triangles,
                        ),
                );
                pass.pop_debug();
            }
            PassKind::Transparent => {
                for &LayerRenderItem {
                    layer_index,
                    stencil_ref_relative,
                } in &self.layers_to_render
                {
                    self.layers[layer_index].layer.render(
                        pass,
                        &self_transform,
                        stencil_ref + stencil_ref_relative,
                        pass_kind,
                    )
                }
            }
        }

        pass.pop_debug();
    }
}

impl<T> LayerGroup<T> {
    fn finish_render_with_mask(
        &self,
        pass: &mut RenderPass,
        prerendered: PrerenderedDrawable,
        mask: &MaskTexture,
        mask_flags: MaskFlags,
        transform_params: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        pass.push_debug("LayerGroup/finish_render_with_mask");

        let props = &self.props;
        if !props.is_visible() {
            return;
        }
        let color_multiplier = props.get_color_multiplier().premultiply();
        let blend_type = props.get_blend_type();
        let fragment_shader = props.get_fragment_shader();
        let fragment_shader_param = props.get_fragment_shader_param();

        let can_split_passes =
            prerendered.target_pass == PassKind::Opaque && !props.is_blending_nontrivial();

        let (render_opaque, render_transparent) = match (can_split_passes, pass_kind) {
            (true, PassKind::Opaque) => (true, false),
            (true, PassKind::Transparent) => (false, true),
            // if we use can't split passes, we have to render everything in transparent pass
            (false, PassKind::Opaque) => {
                return;
            }
            (false, PassKind::Transparent) => (true, true),
        };

        let transform_params = props.get_composed_transform_params(transform_params);

        prepare_mask_for_onscreen(
            pass,
            mask,
            mask_flags,
            transform_params,
            render_opaque,
            render_transparent,
        );

        let clip = props.get_clip_params();
        clip.begin(pass, &transform_params);

        // all the layer transforms were already applied during the pre-rendering
        // so we just take a simple blank slate transform
        let transform = top_left_projection_matrix();

        let fragment_shader = fragment_shader.simplify(fragment_shader_param);

        let builder =
            RenderRequestBuilder::new().depth_stencil_shorthand(stencil_ref, false, false);

        match pass_kind {
            PassKind::Opaque => {
                pass.run(builder.color_blend_type(ColorBlendType::Opaque).build(
                    RenderProgramWithArguments::Layer {
                        output_kind: LayerShaderOutputKind::Layer,
                        fragment_shader,
                        vertices: VertexSource::VertexData {
                            vertices: &build_quad_vertices(|t| PosTexVertex {
                                position: t * VIRTUAL_CANVAS_SIZE_VEC,
                                texture_position: t,
                            }),
                        },
                        texture: prerendered.render_texture,
                        transform,
                        color_multiplier,
                        fragment_shader_param,
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
            PassKind::Transparent => {
                let mut min = 0.0;
                let mut max = 1.0;
                if mask_flags.contains(MaskFlags::INVERT) {
                    std::mem::swap(&mut min, &mut max);
                }

                let mask_virt_coord = if mask_flags.contains(MaskFlags::SCALE) {
                    VIRTUAL_CANVAS_SIZE_VEC
                } else {
                    mask.texture.size_vec()
                };

                let mut flip_scale = Vec3::ONE;

                if mask_flags.contains(MaskFlags::FLIP_X) {
                    flip_scale.x *= -1.0;
                }
                if mask_flags.contains(MaskFlags::FLIP_Y) {
                    flip_scale.y *= -1.0;
                }

                let mask_position_transform = Mat4::from_translation(vec3(0.5, 0.5, 0.0))
                    * Mat4::from_scale(flip_scale)
                    * Mat4::from_scale(1.0 / mask_virt_coord.extend(1.0))
                    * transform_params.compute_total_translation().inverse();

                pass.run(builder.layer_color_blend_premultiplied(blend_type).build(
                    RenderProgramWithArguments::Mask {
                        fragment_shader,
                        vertices: VertexSource::VertexData {
                            vertices: &build_quad_vertices(|t| {
                                MaskVertex {
                                    position: t * VIRTUAL_CANVAS_SIZE_VEC,
                                    texture_position: t,
                                    mask_position: mask_position_transform
                                        .project_point3(
                                            ((t * 2.0 - 1.0) * VIRTUAL_CANVAS_SIZE_VEC / 2.0)
                                                .extend(0.0),
                                        )
                                        .xy(),
                                }
                            }),
                        },
                        texture: prerendered.render_texture,
                        mask: mask.texture.as_source(),
                        transform,
                        color_multiplier,
                        fragment_shader_param,
                        minmax: vec2(min, max),
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
        }

        clip.end(pass);

        finish_mask_for_onscreen(pass);

        pass.pop_debug();
    }
}

impl<T> DrawableLayer for LayerGroup<T>
where
    T: DrawableLayer,
{
    fn properties(&self) -> &LayerProperties {
        &self.props
    }

    fn properties_mut(&mut self) -> &mut LayerProperties {
        &mut self.props
    }
}
