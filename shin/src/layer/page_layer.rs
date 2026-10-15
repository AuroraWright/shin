use glam::vec3;
use shin_core::{
    primitives::color::{FloatColor4, UnormColor},
    vm::command::types::{PLANES_COUNT, PlaneId},
};
use shin_render::{
    DepthStencilState, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    StencilFunction, StencilMask, StencilOperation, StencilPipelineState, StencilState,
    render_pass::RenderPass,
    shaders::types::{
        RenderClone, RenderCloneCtx,
        buffer::VertexSource,
        texture::{DepthStencilTarget, TextureTarget},
        vertices::PosVertex,
    },
};

use crate::{
    layer::{
        DrawableLayer, Layer, LayerGroup, NewDrawableLayer,
        new_drawable_layer::{NewDrawableLayerNeedsSeparatePass, NewDrawableLayerState},
        properties::LayerProperties,
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
    },
    render::PreRenderContext,
    update::{AdvUpdatable, AdvUpdateContext},
};

#[derive(Clone, Copy)]
struct LayerRenderItem {
    pub layer_index: PlaneId,
    pub stencil_ref_relative: u8,
}

pub struct PageLayer {
    update_tracker: crate::layer::update_tracker::LayerUpdateTracker,
    planes: Vec<LayerGroup>,

    stencil_bump: u8,
    needs_force_blending: bool,
    layers_to_render: Vec<LayerRenderItem>,

    new_drawable_state: NewDrawableLayerState,
    props: LayerProperties,
}

impl RenderClone for PageLayer {
    fn render_clone(&self, context: &mut RenderCloneCtx) -> Self {
        // Native9ed38 copies the drawable base before any plane groups.
        let props = self.props.clone();
        let new_drawable_state = self.new_drawable_state.render_clone(context);
        let planes = self.planes.render_clone(context);
        Self {
            update_tracker: self.update_tracker.clone(),
            planes,
            stencil_bump: self.stencil_bump,
            needs_force_blending: self.needs_force_blending,
            // Native9ed60/9ed64 starts the copied prepared plane list empty.
            layers_to_render: Vec::new(),
            new_drawable_state,
            props,
        }
    }
}

impl PageLayer {
    #[cfg(test)]
    pub(crate) fn reference_prepared_plane_state(&self) -> (usize, u8) {
        (self.layers_to_render.len(), self.stencil_bump)
    }
    #[cfg(test)]
    pub(crate) fn reference_dissolve_seed(&self) -> u32 {
        self.new_drawable_state.reference_dissolve_seed()
    }
    #[cfg(test)]
    pub(crate) fn reference_effect_phases(&self) -> [f32; 3] {
        self.new_drawable_state.reference_effect_phases()
    }

    pub fn new(plane_count: usize) -> Self {
        // can't support more, the `PlaneId` type is specialized to 4 planes
        // ideally I would like to only support 4 planes, but `BustupMode` at least uses 1
        assert!(plane_count <= PLANES_COUNT);

        // Native9e970 constructs the seeded drawable before its plane groups.
        let new_drawable_state = NewDrawableLayerState::new();

        Self {
            update_tracker: Default::default(),
            planes: (0..plane_count)
                .map(|i| LayerGroup::new(Some(format!("PageLayer/Plane{}", i))))
                .collect(),
            stencil_bump: 1,
            needs_force_blending: false,
            layers_to_render: vec![],
            new_drawable_state,
            props: LayerProperties::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn collect_voice_positions(
        &self,
        _parent: &TransformParams,
        positions: &mut Vec<(i32, f32)>,
    ) {
        for plane in &self.planes {
            for id in plane.get_used_layerbank_ids() {
                if let Some(crate::layer::user::UserLayer::Bustup(layer)) =
                    plane.get_layer(id).as_deref()
                {
                    let transform = layer.properties().get_transform_params();
                    positions.push((
                        layer.inner_ref().character_id(),
                        character_voice_pan(&transform),
                    ));
                }
            }
        }
    }

    pub fn get_plane(&self, index: PlaneId) -> &LayerGroup {
        &self.planes[index.raw() as usize]
    }

    pub fn get_plane_mut(&mut self, index: PlaneId) -> &mut LayerGroup {
        &mut self.planes[index.raw() as usize]
    }
}
struct PageLayerNewDrawableSeparatePassDelegate;

impl NewDrawableLayerNeedsSeparatePass for PageLayerNewDrawableSeparatePassDelegate {
    fn needs_separate_pass(&self, properties: &LayerProperties) -> bool {
        properties.get_clip_mode() != DrawableClipMode::None
            || properties.is_fragment_shader_nontrivial()
            || properties.is_blending_nontrivial()
    }
}

struct PageLayerNewDrawableDelegate<'a> {
    planes: &'a [LayerGroup],
    layers_to_render: Vec<LayerRenderItem>,
}

impl NewDrawableLayerNeedsSeparatePass for PageLayerNewDrawableDelegate<'_> {
    fn needs_separate_pass(&self, properties: &LayerProperties) -> bool {
        PageLayerNewDrawableSeparatePassDelegate.needs_separate_pass(properties)
    }
}

impl NewDrawableLayer for PageLayerNewDrawableDelegate<'_> {
    #[tracing::instrument(skip_all)]
    fn render_drawable_indirect(
        &mut self,
        context: &mut PreRenderContext,
        props: &LayerProperties,
        target: TextureTarget,
        depth_stencil: DepthStencilTarget,
        transform: &TransformParams,
    ) -> PassKind {
        let mut pass = context.begin_pass(target, Some(depth_stencil), "PageLayer/indirect");

        if !props.is_visible() {
            pass.clear(Some(UnormColor::BLACK), None, None);
        } else {
            let self_transform = props.get_composed_transform_params(transform);

            pass.clear(Some(UnormColor::BLACK), Some(0), None);

            for render_item in self.layers_to_render.iter().rev() {
                self.planes[render_item.layer_index.raw() as usize].render(
                    &mut pass,
                    &self_transform,
                    render_item.stencil_ref_relative,
                    PassKind::Opaque,
                );
            }
            for render_item in &self.layers_to_render {
                self.planes[render_item.layer_index.raw() as usize].render(
                    &mut pass,
                    &self_transform,
                    render_item.stencil_ref_relative,
                    PassKind::Transparent,
                );
            }
        }

        self.layers_to_render.clear();

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
        // direct rendering is always done by the PageLayer without relying on NewDrawableLayer
        unreachable!()
    }
}

impl AdvUpdatable for PageLayer {
    fn update(&mut self, context: &AdvUpdateContext) {
        if !self.update_tracker.update(context.frame_id) {
            return;
        }
        self.props.update(context);
        self.new_drawable_state.update(context, &self.props);

        for plane in self.planes.iter_mut() {
            plane.update(context);
        }
    }
}

impl Layer for PageLayer {
    fn fast_forward(&mut self) {
        self.props.fast_forward();

        for plane in &mut self.planes {
            plane.fast_forward();
        }
    }

    fn get_stencil_bump(&self) -> u8 {
        self.stencil_bump
    }

    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        let props = &self.props;
        let self_transform = props.get_composed_transform_params(transform);

        let mut layer_stack = Vec::new();

        for index in 0..self.planes.len() {
            let layer_group = &mut self.planes[index];

            layer_group.pre_render(context, &self_transform);
            if layer_group.needs_rendering() {
                // if a layer is rendered opaquely, we won't be able to see anything below it
                if layer_group.is_rendered_opaquely() {
                    layer_stack.clear();
                }

                layer_stack.push(index);
            }
        }

        self.layers_to_render.clear();
        let mut layers_to_render = std::mem::take(&mut self.layers_to_render);
        self.needs_force_blending = props.is_blending_nontrivial();
        let mut stencil_value = 1;

        for index in layer_stack {
            layers_to_render.push(LayerRenderItem {
                layer_index: PlaneId::new(index as _),
                stencil_ref_relative: stencil_value,
            });

            stencil_value += self.planes[index].get_stencil_bump();
        }
        self.stencil_bump = stencil_value;

        let mut delegate = PageLayerNewDrawableDelegate {
            planes: &self.planes,
            layers_to_render,
        };

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
        let props = self.properties();
        if self.new_drawable_state.try_finish_indirect_render(
            props,
            pass,
            transform,
            stencil_ref,
            pass_kind,
        ) {
            return;
        }

        if !props.is_visible() {
            return;
        }

        let self_transform = props.get_composed_transform_params(transform);

        pass.push_debug(&format!(
            "PageLayer/{}",
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
                    self.planes[layer_index.raw() as usize].render(
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
                    self.planes[layer_index.raw() as usize].render(
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

impl DrawableLayer for PageLayer {
    fn properties(&self) -> &LayerProperties {
        &self.props
    }

    fn properties_mut(&mut self) -> &mut LayerProperties {
        &mut self.props
    }
}

// Native bustup 0x369a0 applies its local property projection to local origin,
// before the texture's origin offset, then clamps clip X to [-1, 1].
#[cfg(test)]
fn character_voice_pan(transform: &TransformParams) -> f32 {
    transform
        .compute_final_transform()
        .transform_point3(glam::Vec3::ZERO)
        .x
        .clamp(-1.0, 1.0)
}

#[cfg(test)]
mod voice_pan_tests {
    use super::*;
    use crate::{
        asset::bustup::{Bustup, CharacterId},
        layer::{DrawableLayer, user::BustupLayer},
    };
    use shin_core::vm::command::types::{LayerProperty, LayerbankId};
    use std::sync::Arc;

    #[test]
    fn voice_pan_uses_local_properties_without_parent_composition() {
        let mut page = PageLayer::new(1);
        page.props
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(240.0);
        let plane = page.get_plane_mut(PlaneId::new(0));
        plane
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(120.0);
        let asset = Arc::new(Bustup {
            character_id: CharacterId::new(19),
            origin_x: 1000,
            origin_y: 500,
            effective_width: 0,
            effective_height: 0,
            bustup_id: shin_core::format::bustup::BustupId::new(0),
            base_blocks: vec![],
            face1: None,
            face2: None,
            mouth_blocks: vec![],
            eye_blocks: vec![],
        });
        let mut bustup = BustupLayer::new(asset, None);
        bustup
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(-600.0);
        plane.add_layer(LayerbankId::new(0), bustup.into());
        let parent = TransformParams {
            wobble_translation: glam::vec2(-240.0, 0.0),
            ..Default::default()
        };
        let mut positions = Vec::new();
        page.collect_voice_positions(&parent, &mut positions);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].0, 19);
        assert!((positions[0].1 + 0.625).abs() < 0.00001, "{positions:?}");
        for (x, expected) in [(-1920.0, -1.0), (0.0, 0.0), (480.0, 0.5), (1920.0, 1.0)] {
            let params = TransformParams {
                transform: glam::Mat4::from_translation(glam::vec3(x, 0.0, 0.0)),
                ..Default::default()
            };
            assert!((character_voice_pan(&params) - expected).abs() < 0.00001);
        }
    }
}
