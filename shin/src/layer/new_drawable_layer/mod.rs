mod dissolve;
mod effect_passes;
mod effect_phases;
#[cfg(test)]
pub(crate) use effect_phases::EffectPhases;

use glam::Vec4;
use shin_core::{
    primitives::color::{FloatColor4, UnormColor},
    vm::command::types::LayerProperty,
};
use shin_render::{
    ColorBlendType, DepthStencilState, DrawPrimitive, LayerBlendType, LayerFragmentShader,
    LayerShaderOutputKind, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    StencilFunction, StencilOperation, StencilPipelineState, StencilState,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    shaders::types::{
        RenderClone,
        buffer::VertexSource,
        texture::{DepthStencilTarget, TextureSource, TextureTarget},
        vertices::PosTexVertex,
    },
};

use crate::{
    layer::{
        DrawableLayer, Layer, LayerProperties,
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
    },
    render::{
        PreRenderContext, VIRTUAL_CANVAS_SIZE_VEC, render_texture_holder::RenderTextureHolder,
        top_left_projection_matrix,
    },
    update::{AdvUpdatable, AdvUpdateContext},
};

pub trait NewDrawableLayerNeedsSeparatePass {
    fn needs_separate_pass(&self, #[expect(unused)] props: &LayerProperties) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawablePropertyPolicy {
    Full,
    /// A plain drawable can read transforms without advancing property clocks
    /// or applying NewDrawableLayer visibility, shading, clipping and effects.
    TransformOnly,
    /// A plain parent captures its transformed child each frame and presents
    /// that viewport texture without generic property shading or effects.
    Captured,
}

pub trait NewDrawableLayer: NewDrawableLayerNeedsSeparatePass {
    fn stencil_bump(&self) -> u8 {
        1
    }
    fn property_policy(&self) -> DrawablePropertyPolicy {
        DrawablePropertyPolicy::Full
    }
    /// Plain renderers can reject a pass before touching lazy properties.
    fn renders_pass(&self, _pass_kind: PassKind) -> bool {
        true
    }
    /// Native Tile selects alpha/pass before beginning its clip work.
    fn handles_direct_clip(&self) -> bool {
        false
    }
    fn update_with_properties(&mut self, context: &AdvUpdateContext, _props: &LayerProperties)
    where
        Self: AdvUpdatable,
    {
        self.update(context);
    }

    #[tracing::instrument(skip_all)]
    fn render_drawable_indirect(
        &mut self,
        context: &mut PreRenderContext,
        props: &LayerProperties,
        target: TextureTarget,
        depth_stencil: DepthStencilTarget,
        transform: &TransformParams,
    ) -> PassKind {
        // Native 0x79160: clear a transparent source, draw both geometry passes
        // with neutral shading, and defer layer tint/blend/effects to compositing.
        let drawable = DrawableParams {
            color_multiplier: FloatColor4::WHITE,
            blend_type: LayerBlendType::Type1,
            fragment_shader: LayerFragmentShader::Default,
            shader_param: Vec4::ONE,
        };
        let clip = DrawableClipParams {
            mode: DrawableClipMode::None,
            area: Vec4::ZERO,
        };
        let transform = props.get_composed_transform_params(transform);
        let mut pass = context.begin_pass(target, Some(depth_stencil), "Drawable source");
        pass.clear(Some(UnormColor::from_rgba(0, 0, 0, 0)), Some(0), None);
        for pass_kind in [PassKind::Opaque, PassKind::Transparent] {
            self.render_drawable_direct(&mut pass, &transform, &drawable, &clip, 1, pass_kind);
        }
        PassKind::Transparent
    }
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        drawable: &DrawableParams,
        clip: &DrawableClipParams,
        // TODO: make a strong type for this?
        stencil_ref: u8,
        pass_kind: PassKind,
    );
}

pub trait NewDrawableLayerFastForward {
    /// Derived native skip handlers can preserve base property clocks.
    fn fast_forward_properties(&self) -> bool {
        true
    }
    fn fast_forward(&mut self);
}

pub struct PrerenderedDrawable<'a> {
    pub render_texture: TextureSource<'a>,
    pub target_pass: PassKind,
}

#[derive(Debug, RenderClone)]
pub struct NewDrawableLayerState {
    #[render_clone(needs_render)]
    render_texture_src: RenderTextureHolder,
    // Scratch target for effects that need a separate destination.
    #[render_clone(needs_render)]
    render_texture_target: Option<RenderTexture>,
    #[render_clone(needs_render)]
    render_texture_prev_frame: Option<RenderTexture>,
    target_pass: PassKind,
    dissolve_seed: u32,
    effect_phases: effect_phases::EffectPhases,
}

impl NewDrawableLayerState {
    fn pre_render_captured<T: NewDrawableLayer>(
        &mut self,
        context: &mut PreRenderContext,
        props: &LayerProperties,
        delegate: &mut T,
        transform: &TransformParams,
    ) {
        let target = self
            .render_texture_src
            .get_or_init(context)
            .as_texture_target();
        self.target_pass = delegate.render_drawable_indirect(
            context,
            props,
            target,
            context.depth_stencil,
            transform,
        );
    }

    fn render_captured(&self, pass: &mut RenderPass, stencil_ref: u8, pass_kind: PassKind) {
        if pass_kind != PassKind::Opaque {
            return;
        }
        let Some(texture) = self.render_texture_src.get() else {
            return;
        };
        // Native Murderer9b4a0 submits the captured viewport with sprite2,
        // opaque pass/blending and >= stencil, independently of properties.
        shin_render::quad_vertices::QuadVertices::new()
            .with_box(
                0.0,
                0.0,
                VIRTUAL_CANVAS_SIZE_VEC.x,
                VIRTUAL_CANVAS_SIZE_VEC.y,
            )
            .render_sprite(
                pass,
                RenderRequestBuilder::new()
                    .depth_stencil_shorthand(stencil_ref, true, false)
                    .color_blend_type(ColorBlendType::Opaque),
                texture.as_texture_source(),
                top_left_projection_matrix(),
            );
    }

    #[cfg(test)]
    pub(crate) fn reference_effect_phases(&self) -> [f32; 3] {
        self.effect_phases.0
    }
    #[cfg(test)]
    pub(crate) fn reference_dissolve_seed(&self) -> u32 {
        self.dissolve_seed
    }
    pub fn new() -> Self {
        Self::with_dissolve_seed(crate::engine_random::next_seed())
    }

    fn with_dissolve_seed(dissolve_seed: u32) -> Self {
        Self {
            render_texture_src: RenderTextureHolder::new("NewDrawableLayerState/render_texture"),
            render_texture_target: None,
            render_texture_prev_frame: None,
            target_pass: PassKind::Transparent,
            effect_phases: Default::default(),
            dissolve_seed,
        }
    }

    pub fn get_prerendered_tex(&self) -> Option<PrerenderedDrawable<'_>> {
        let tex = self.render_texture_src.get()?;

        Some(PrerenderedDrawable {
            render_texture: tex.as_texture_source(),
            target_pass: self.target_pass,
        })
    }

    pub fn update(&mut self, context: &AdvUpdateContext, props: &LayerProperties) {
        let periods = [
            props.get_value(LayerProperty::RasterHorizontalTPeriod),
            props.get_value(LayerProperty::RasterVerticalTPeriod),
            props.get_value(LayerProperty::RippleTPeriod),
        ];
        if props.policy() == super::PropertyPolicy::Gerokasu2 {
            self.effect_phases
                .advance_gerokasu2(context.delta_ticks.as_f32(), periods);
        } else {
            self.effect_phases
                .advance(context.delta_ticks.as_f32(), periods);
        }
    }

    pub fn is_rendered_opaquely<T: NewDrawableLayerNeedsSeparatePass>(
        &self,
        props: &LayerProperties,
        delegate: &T,
    ) -> bool {
        let Some(tex) = self.get_prerendered_tex() else {
            return true;
        };

        if delegate.needs_separate_pass(props) {
            // weird! I think this is too conservative
            return false;
        }

        tex.target_pass == PassKind::Opaque
    }

    pub fn pre_render<T: NewDrawableLayer>(
        &mut self,
        context: &mut PreRenderContext,
        props: &LayerProperties,
        delegate: &mut T,
        transform: &TransformParams,
    ) {
        if !props.is_visible() {
            return;
        }

        let blur_radius = props.get_value(LayerProperty::BlurRadius) * 0.001;
        let prop70 = props.get_value(LayerProperty::Prop70) * 0.001;
        let mosaic_size = props.get_value(LayerProperty::MosaicSize) as i32;
        let raster_horizontal_amplitude = props.get_value(LayerProperty::RasterHorizontalAmplitude);
        let raster_vertical_amplitude = props.get_value(LayerProperty::RasterVerticalAmplitude);
        let ripple_amplitude = props.get_value(LayerProperty::RippleAmplitude);
        let dissolve_intensity = props.get_value(LayerProperty::DissolveIntensity) * 0.001;
        let ghosting_alpha = props.get_value(LayerProperty::GhostingAlpha) * 0.001;

        if blur_radius.abs() < f32::EPSILON
            && prop70.abs() < f32::EPSILON
            && mosaic_size <= 0
            && raster_horizontal_amplitude.abs() < f32::EPSILON
            && raster_vertical_amplitude.abs() < f32::EPSILON
            && ripple_amplitude.abs() < f32::EPSILON
            && dissolve_intensity <= 0.0
            && ghosting_alpha <= 0.0
            && !delegate.needs_separate_pass(props)
        {
            self.render_texture_src.clear();
            self.render_texture_target = None;
            self.render_texture_prev_frame = None;
            return;
        }

        if ghosting_alpha <= 0.0 {
            self.render_texture_prev_frame = None;
        } else {
            std::mem::swap(
                &mut self.render_texture_prev_frame,
                &mut self.render_texture_src.as_inner_mut(),
            );
        }

        let render_texture_src = self.render_texture_src.get_or_init(context);
        self.target_pass = delegate.render_drawable_indirect(
            context,
            props,
            render_texture_src.as_texture_target(),
            context.depth_stencil,
            transform,
        );

        if blur_radius.abs() >= f32::EPSILON {
            let target = self
                .render_texture_target
                .get_or_insert_with(|| context.new_render_texture("Drawable effect target".into()));
            effect_passes::apply_blur(context, render_texture_src, target, blur_radius);
        }
        if prop70.abs() >= f32::EPSILON {
            let target = self
                .render_texture_target
                .get_or_insert_with(|| context.new_render_texture("Drawable effect target".into()));
            effect_passes::apply_zoom_blur(context, props, render_texture_src, target, prop70);
        }
        if mosaic_size > 0 {
            let target = self
                .render_texture_target
                .get_or_insert_with(|| context.new_render_texture("Drawable effect target".into()));
            effect_passes::apply_mosaic(context, render_texture_src, target, mosaic_size);
            std::mem::swap(render_texture_src, target);
        }
        if raster_horizontal_amplitude.abs() >= f32::EPSILON
            || raster_vertical_amplitude.abs() >= f32::EPSILON
        {
            let target = self
                .render_texture_target
                .get_or_insert_with(|| context.new_render_texture("Drawable effect target".into()));
            effect_passes::apply_raster(
                context,
                props,
                render_texture_src,
                target,
                self.effect_phases.0,
            );
            std::mem::swap(render_texture_src, target);
        }
        if ripple_amplitude.abs() >= f32::EPSILON {
            let target = self
                .render_texture_target
                .get_or_insert_with(|| context.new_render_texture("Drawable effect target".into()));
            effect_passes::apply_ripple(
                context,
                props,
                render_texture_src,
                target,
                self.effect_phases.0[2],
            );
            std::mem::swap(render_texture_src, target);
        }
        if dissolve_intensity > 0.0 {
            let target = self.render_texture_target.get_or_insert_with(|| {
                context.new_render_texture("Drawable dissolve target".into())
            });
            effect_passes::apply_dissolve(
                context,
                render_texture_src,
                target,
                dissolve_intensity,
                props.get_value(LayerProperty::DissolveMode) as i32,
                self.dissolve_seed,
            );
            std::mem::swap(render_texture_src, target);
            self.target_pass = PassKind::Transparent;
        }
        match (ghosting_alpha > 0.0, &mut self.render_texture_prev_frame) {
            (true, Some(render_texture_prev_frame)) => {
                effect_passes::apply_ghosting(
                    context,
                    props,
                    render_texture_src,
                    render_texture_prev_frame,
                    ghosting_alpha,
                );
            }
            _ => self.render_texture_prev_frame = None,
        }
    }

    pub fn try_finish_indirect_render(
        &self,
        props: &LayerProperties,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) -> bool {
        let Some(tex) = self.render_texture_src.get() else {
            return false;
        };

        if pass_kind != self.target_pass {
            return true;
        }

        let color_multiplier = props.get_color_multiplier().premultiply();
        let blend_type = props.get_blend_type();
        let fragment_shader = props.get_fragment_shader();
        let fragment_shader_param = props.get_fragment_shader_param();

        // NOTE: the transform is actually used just for clipping
        // we still compute it just in case
        let self_transform = props.get_composed_transform_params(transform);

        let clip_params = props.get_clip_params();
        clip_params.begin(pass, &self_transform);

        let transform = top_left_projection_matrix();

        let vertices = &build_quad_vertices(|t| PosTexVertex {
            position: t * VIRTUAL_CANVAS_SIZE_VEC,
            texture_position: t,
        });

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
                            ..Default::default()
                        },
                        stencil_reference: stencil_ref,
                    },
                })
                .color_blend_type(match pass_kind {
                    PassKind::Opaque => ColorBlendType::Opaque,
                    PassKind::Transparent => ColorBlendType::from_premultiplied_layer(blend_type),
                })
                .build(
                    RenderProgramWithArguments::Layer {
                        output_kind: match pass_kind {
                            PassKind::Opaque => LayerShaderOutputKind::Layer,
                            PassKind::Transparent => LayerShaderOutputKind::LayerDiscard,
                        },
                        fragment_shader,
                        vertices: VertexSource::VertexData { vertices },
                        texture: tex.as_texture_source(),
                        transform,
                        color_multiplier,
                        fragment_shader_param,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );

        clip_params.end(pass);
        true
    }

    #[tracing::instrument(skip_all)]
    pub fn render<T: NewDrawableLayer>(
        &self,
        props: &LayerProperties,
        delegate: &T,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if !props.is_visible() {
            return;
        }

        if self.try_finish_indirect_render(props, pass, transform, stencil_ref, pass_kind) {
            return;
        }

        let self_transform = props.get_composed_transform_params(transform);

        let drawable = props.get_drawable_params();
        let clip = props.get_clip_params();

        if delegate.handles_direct_clip() {
            delegate.render_drawable_direct(
                pass,
                &self_transform,
                &drawable,
                &clip,
                stencil_ref,
                pass_kind,
            );
            return;
        }
        clip.begin(pass, &self_transform);
        let source_clip = DrawableClipParams {
            mode: DrawableClipMode::None,
            area: Vec4::ZERO,
        };
        delegate.render_drawable_direct(
            pass,
            &self_transform,
            &drawable,
            &source_clip,
            stencil_ref,
            pass_kind,
        );
        clip.end(pass);
    }
}

#[derive(Debug)]
enum DrawableStateOwner {
    Unique(NewDrawableLayerState),
    Retained(std::sync::Arc<parking_lot::Mutex<NewDrawableLayerState>>),
}
impl DrawableStateOwner {
    fn retain(self) -> Self {
        match self {
            Self::Unique(state) => {
                Self::Retained(std::sync::Arc::new(parking_lot::Mutex::new(state)))
            }
            retained => retained,
        }
    }
    fn with<R>(&self, action: impl FnOnce(&NewDrawableLayerState) -> R) -> R {
        match self {
            Self::Unique(state) => action(state),
            Self::Retained(state) => action(&state.lock()),
        }
    }
    fn with_mut<R>(&mut self, action: impl FnOnce(&mut NewDrawableLayerState) -> R) -> R {
        match self {
            Self::Unique(state) => action(state),
            Self::Retained(state) => action(&mut state.lock()),
        }
    }
    #[cfg(test)]
    fn retained_identity(&self) -> Option<usize> {
        match self {
            Self::Unique(_) => None,
            Self::Retained(state) => Some(std::sync::Arc::as_ptr(state) as usize),
        }
    }
}
impl RenderClone for DrawableStateOwner {
    fn render_clone(&self, context: &mut shin_render::shaders::types::RenderCloneCtx) -> Self {
        match self {
            Self::Unique(state) => Self::Unique(state.render_clone(context)),
            Self::Retained(state) => Self::Retained(state.clone()),
        }
    }
}

// packages NewDrawableLayerState and LayerProperties to implement simple NewDrawable-based layers
#[derive(Debug)]
pub struct NewDrawableLayerWrapper<T> {
    update_tracker: crate::layer::update_tracker::LayerUpdateTracker,
    inner_layer: T,
    state: DrawableStateOwner,
    props: LayerProperties,
    retain_owner: bool,
}

impl<T: RenderClone> RenderClone for NewDrawableLayerWrapper<T> {
    fn render_clone(&self, context: &mut shin_render::shaders::types::RenderCloneCtx) -> Self {
        Self {
            update_tracker: self.update_tracker.clone(),
            inner_layer: self.inner_layer.render_clone(context),
            state: self.state.render_clone(context),
            props: if self.retain_owner {
                self.props.retain_for_clone()
            } else {
                self.props.clone()
            },
            retain_owner: self.retain_owner,
        }
    }
}

impl<T: NewDrawableLayer> NewDrawableLayerWrapper<T> {
    #[cfg(test)]
    pub(crate) fn reference_dissolve_seed(&self) -> u32 {
        self.state.with(|state| state.dissolve_seed)
    }
    #[cfg(test)]
    pub(crate) fn reference_effect_phases(&self) -> [f32; 3] {
        self.state.with(|state| state.reference_effect_phases())
    }
    #[cfg(test)]
    pub(crate) fn reference_retained_render_identity(&self) -> Option<usize> {
        self.state.retained_identity()
    }

    #[cfg(test)]
    pub(crate) fn reference_with_capture<R>(
        &self,
        read: impl FnOnce(&RenderTexture) -> R,
    ) -> Option<R> {
        self.state
            .with(|state| state.render_texture_src.get().map(read))
    }
    pub fn from_inner(inner_layer: T) -> Self {
        Self::from_inner_constructor(|| inner_layer)
    }

    pub(crate) fn from_inner_constructor(make_inner: impl FnOnce() -> T) -> Self {
        // Native75efc consumes the drawable's seed before a derived constructor
        // consumes its own seed (e.g. Bustup35c50 and Rainc9a94).
        let state = NewDrawableLayerState::new();
        Self {
            inner_layer: make_inner(),
            update_tracker: Default::default(),
            state: DrawableStateOwner::Unique(state),
            props: LayerProperties::new(),
            retain_owner: false,
        }
    }

    /// Copy handlers that return the same native drawable retain clocks,
    /// property pointer replacement and prepared render state together.
    pub(crate) fn from_retained_inner_constructor(make_inner: impl FnOnce() -> T) -> Self {
        let mut layer = Self::from_inner_constructor(make_inner);
        layer.update_tracker = crate::layer::update_tracker::LayerUpdateTracker::retained();
        layer.state = layer.state.retain();
        layer.retain_owner = true;
        layer
    }

    pub(crate) fn from_plain_layer_constructor(make_inner: impl FnOnce() -> T) -> Self {
        // Native InteractiveLayer constructors call75940 (DrawableLayer), not
        // NewDrawableLayer75e50. The rendering adapter needs scratch state,
        // but must not consume a native drawable seed that these layers lack.
        Self {
            inner_layer: make_inner(),
            update_tracker: Default::default(),
            state: DrawableStateOwner::Unique(NewDrawableLayerState::with_dissolve_seed(0)),
            props: LayerProperties::new(),
            retain_owner: false,
        }
    }

    pub(crate) fn from_retained_plain_layer_constructor(make_inner: impl FnOnce() -> T) -> Self {
        let mut layer = Self::from_plain_layer_constructor(make_inner);
        layer.update_tracker = crate::layer::update_tracker::LayerUpdateTracker::retained();
        layer.state = layer.state.retain();
        layer.retain_owner = true;
        layer
    }

    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner_layer
    }

    pub fn inner_ref(&self) -> &T {
        &self.inner_layer
    }
}

impl<T: NewDrawableLayer + AdvUpdatable> AdvUpdatable for NewDrawableLayerWrapper<T> {
    fn update(&mut self, context: &AdvUpdateContext) {
        if !self.update_tracker.update(context.frame_id) {
            return;
        }
        if self.inner_layer.property_policy() == DrawablePropertyPolicy::Full {
            self.props.update(context);
            // Native78820 advances base effect phases before the derived body:
            // bustup36ac4, rainca700 ->251d0, and moviec34d0 ->251d0.
            let props = &self.props;
            self.state.with_mut(|state| state.update(context, props));
        }
        self.inner_layer
            .update_with_properties(context, &self.props);
    }
}

impl<T: NewDrawableLayer + AdvUpdatable + NewDrawableLayerFastForward> Layer
    for NewDrawableLayerWrapper<T>
{
    fn get_stencil_bump(&self) -> u8 {
        self.inner_layer.stencil_bump()
    }
    fn fast_forward(&mut self) {
        if self.inner_layer.property_policy() == DrawablePropertyPolicy::Full
            && self.inner_layer.fast_forward_properties()
        {
            self.props.fast_forward();
        }
        self.inner_layer.fast_forward();
    }

    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        if self.inner_layer.property_policy() == DrawablePropertyPolicy::TransformOnly {
            return;
        }
        let props = &self.props;
        let inner = &mut self.inner_layer;
        if inner.property_policy() == DrawablePropertyPolicy::Captured {
            self.state
                .with_mut(|state| state.pre_render_captured(context, props, inner, transform));
            return;
        }
        self.state
            .with_mut(|state| state.pre_render(context, props, inner, transform));
    }

    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if self.inner_layer.property_policy() == DrawablePropertyPolicy::Captured {
            self.state
                .with(|state| state.render_captured(pass, stencil_ref, pass_kind));
            return;
        }
        if self.inner_layer.property_policy() == DrawablePropertyPolicy::TransformOnly {
            if !self.inner_layer.renders_pass(pass_kind) {
                return;
            }
            let transform = self.props.get_composed_transform_params(transform);
            let drawable = DrawableParams {
                color_multiplier: FloatColor4::WHITE,
                blend_type: LayerBlendType::Type1,
                fragment_shader: LayerFragmentShader::Default,
                shader_param: Vec4::ONE,
            };
            let clip = DrawableClipParams {
                mode: DrawableClipMode::None,
                area: Vec4::ZERO,
            };
            self.inner_layer.render_drawable_direct(
                pass,
                &transform,
                &drawable,
                &clip,
                stencil_ref,
                pass_kind,
            );
            return;
        }
        self.state.with(|state| {
            state.render(
                &self.props,
                &self.inner_layer,
                pass,
                transform,
                stencil_ref,
                pass_kind,
            )
        });
    }
}

impl<T: NewDrawableLayer + AdvUpdatable + NewDrawableLayerFastForward> DrawableLayer
    for NewDrawableLayerWrapper<T>
{
    fn set_properties(&mut self, properties: LayerProperties) {
        if self.retain_owner {
            self.props.rebind(properties);
        } else {
            self.props = properties;
        }
    }
    fn properties(&self) -> &LayerProperties {
        &self.props
    }

    fn properties_mut(&mut self) -> &mut LayerProperties {
        &mut self.props
    }
}
