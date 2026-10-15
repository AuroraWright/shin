use enum_map::{EnumMap, enum_map};
use glam::{Mat4, Vec2, Vec4, vec2, vec3, vec4};
use shin_core::{
    primitives::color::FloatColor4,
    time::{Ticks, Tweener},
    vm::command::types::{LayerId, LayerProperty},
};
use shin_render::{LayerBlendType, LayerFragmentShader};

use crate::{
    layer::{
        render_params::{
            ComposeFlags, DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams,
            multiply_transforms,
        },
        wobbler::Wobbler,
    },
    update::{AdvUpdatable, AdvUpdateContext},
};

fn initial_values() -> EnumMap<LayerProperty, i32> {
    enum_map! {
        v => v.initial_value()
    }
}

#[derive(Debug, Clone)]
struct LayerPropertiesData {
    #[allow(unused)] // it's probably unused by the game too, but let's keep it
    layer_load_counter1: u32,
    updated_frame: shin_core::primitives::update::FrameId,
    layer_id: LayerId,
    properties: EnumMap<LayerProperty, Tweener>,
    wobbler_x: Wobbler,
    wobbler_y: Wobbler,
    wobbler_alpha: Wobbler,
    wobbler_rotation: Wobbler,
    wobbler_scale_x: Wobbler,
    wobbler_scale_y: Wobbler,
}

impl LayerPropertiesData {
    pub fn from_state(state: &LayerPropertiesState, layer_id: LayerId, load_counter: u32) -> Self {
        let mut properties = Self::new();
        properties.layer_id = layer_id;
        properties.layer_load_counter1 = load_counter;
        for (property, value) in &state.properties {
            properties.properties[property].fast_forward_to(*value as f32);
        }
        properties
    }

    pub fn new() -> Self {
        // Native73858..738a8 constructs in X,Y,rotation,scaleX,scaleY,alpha order.
        let wobbler_x = Wobbler::new();
        let wobbler_y = Wobbler::new();
        let wobbler_rotation = Wobbler::new();
        let wobbler_scale_x = Wobbler::new();
        let wobbler_scale_y = Wobbler::new();
        let wobbler_alpha = Wobbler::new();
        // Native738b0 consumes one more random value after the six oscillators.
        let _ = crate::engine_random::next_seed();
        Self {
            layer_load_counter1: 0,
            updated_frame: Default::default(),
            layer_id: LayerId::new(0),
            properties: initial_values().map(|_, v| Tweener::new(v as f32)),
            wobbler_x,
            wobbler_y,
            wobbler_alpha,
            wobbler_rotation,
            wobbler_scale_x,
            wobbler_scale_y,
        }
    }

    pub fn get_value(&self, property: LayerProperty) -> f32 {
        self.properties[property].value()
    }
    #[allow(unused)]
    pub fn property_tweener(&self, property: LayerProperty) -> &Tweener {
        &self.properties[property]
    }

    pub fn property_tweener_mut(&mut self, property: LayerProperty) -> &mut Tweener {
        &mut self.properties[property]
    }

    pub fn init(&mut self) {
        for (prop, val) in initial_values() {
            self.properties[prop].fast_forward_to(val as f32);
        }
    }

    pub fn fast_forward(&mut self) {
        for (_, tweener) in &mut self.properties {
            tweener.fast_forward_unless_prohibited();
        }
    }

    pub fn get_layer_id(&self) -> LayerId {
        self.layer_id
    }

    pub fn set_layer_id(&mut self, layer_id: LayerId) {
        self.layer_id = layer_id;
    }

    pub fn set_layerload_counter1(&mut self, layer_load_counter1: u32) {
        self.layer_load_counter1 = layer_load_counter1;
    }

    fn evaluate_wobbler(
        &self,
        wobbler: &Wobbler,
        amplitude: LayerProperty,
        bias: LayerProperty,
        scale: f32,
        fallback: f32,
    ) -> f32 {
        if let Some(value) = wobbler.value_opt() {
            (value * self.get_value(amplitude) + self.get_value(bias)) * scale
        } else {
            fallback
        }
    }

    fn get_effective_alpha(&self) -> f32 {
        use LayerProperty::*;

        let base = self.get_value(MulColorAlpha) * 0.001;

        base * self.evaluate_wobbler(
            &self.wobbler_alpha,
            WobbleAlphaAmplitude,
            WobbleAlphaBias,
            0.001,
            1.0,
        )
    }

    pub fn is_visible(&self) -> bool {
        use LayerProperty::*;

        if self.get_value(ShowLayer) as i32 == 0
            || self.get_value(ScaleX) as i32 == 0
            || self.get_value(ScaleY) as i32 == 0
        {
            return false;
        }

        self.get_effective_alpha() > 0.0
    }

    pub fn get_color_multiplier(&self) -> FloatColor4 {
        use LayerProperty::*;

        // NB: we initially multiply the color channels by 0.0005 instead of 0.001
        // this is to allow values between 0 and 2000 to be passed through the clamp below
        // the range is later restored, so in the end our color channel multipliers are [0.0; 2.0]
        let r = self.get_value(MulColorRed) * 0.0005;
        let g = self.get_value(MulColorGreen) * 0.0005;
        let b = self.get_value(MulColorBlue) * 0.0005;
        let a = self.get_effective_alpha();

        let mut color = vec4(r, g, b, a);
        color = color.clamp(Vec4::splat(0.0), Vec4::splat(1.0));
        // restore the original range
        color *= vec4(2.0, 2.0, 2.0, 1.0);

        FloatColor4::from_vec4(color)
    }

    pub fn get_blend_type(&self) -> LayerBlendType {
        use LayerProperty::*;

        let value = self.get_value(BlendType) as i32;
        match value {
            0 => LayerBlendType::Type1,
            1 => LayerBlendType::Type2,
            2 => LayerBlendType::Type3,
            _ => LayerBlendType::Type1,
        }
    }

    pub fn get_fragment_shader(&self) -> LayerFragmentShader {
        use LayerProperty::*;

        let value = self.get_value(FragmentShader) as i32;
        match value {
            0 => LayerFragmentShader::Default,
            1 => LayerFragmentShader::Mono,
            2 => LayerFragmentShader::Fill,
            3 => LayerFragmentShader::Fill2,
            4 => LayerFragmentShader::Negative,
            5 => LayerFragmentShader::Gamma,
            _ => LayerFragmentShader::Default,
        }
    }

    pub fn get_fragment_shader_param(&self) -> Vec4 {
        use LayerProperty::*;

        vec4(
            self.get_value(ShaderParamX) * 0.001,
            self.get_value(ShaderParamY) * 0.001,
            self.get_value(ShaderParamZ) * 0.001,
            self.get_value(ShaderParamW) * 0.001,
        )
    }

    pub fn is_fragment_shader_nontrivial(&self) -> bool {
        if self.get_color_multiplier() != FloatColor4::WHITE {
            return true;
        }

        let fragment_shader = self.get_fragment_shader();
        let shader_param = self.get_fragment_shader_param();

        !fragment_shader.is_equivalent_to_default(shader_param)
    }

    pub fn is_blending_nontrivial(&self) -> bool {
        use LayerProperty::*;

        // NB: not using get_effective_alpha here, wobbler alpha is not taken into account
        // this might be a bug in the original code
        self.get_value(MulColorAlpha) * 0.001 < 1.0
            || self.get_blend_type() != LayerBlendType::Type1
    }

    pub fn get_drawable_params(&self) -> DrawableParams {
        let color_multiplier = self.get_color_multiplier();
        let blend_type = self.get_blend_type();
        let fragment_shader = self.get_fragment_shader();

        let shader_param = self.get_fragment_shader_param();

        DrawableParams {
            color_multiplier,
            blend_type,
            fragment_shader,
            shader_param,
        }
    }

    pub fn get_clip_mode(&self) -> DrawableClipMode {
        use LayerProperty::*;

        let value = self.get_value(ClipMode) as i32;
        match value {
            0 => DrawableClipMode::None,
            1 => DrawableClipMode::Clip,
            2 => DrawableClipMode::ClipIgnoreTransform,
            _ => DrawableClipMode::None,
        }
    }

    pub fn get_clip_params(&self) -> DrawableClipParams {
        use LayerProperty::*;

        let mode = self.get_clip_mode();
        let from_x = self.get_value(ClipFromX);
        let to_x = self.get_value(ClipToX);
        let from_y = self.get_value(ClipFromY);
        let to_y = self.get_value(ClipToY);

        let (from_x, to_x) = if from_x < to_x {
            (from_x, to_x)
        } else {
            (to_x, from_x)
        };
        let (from_y, to_y) = if from_y < to_y {
            (from_y, to_y)
        } else {
            (to_y, from_y)
        };

        let width = to_x - from_x;
        let height = to_y - from_y;

        DrawableClipParams {
            mode,
            area: vec4(from_x, from_y, width, height),
        }
    }

    fn get_transform(&self) -> Mat4 {
        use LayerProperty::*;

        let mut result = Mat4::IDENTITY;

        bitflags::bitflags! {
            #[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
            struct FlipFlags: i32 {
                const FLIP_HORIZONTAL = 0b01;
                const FLIP_VERTICAL = 0b10;
            }
        }
        let flip = FlipFlags::from_bits_truncate(self.get_value(Flip) as i32);
        let flip_scale = vec2(
            if flip.contains(FlipFlags::FLIP_HORIZONTAL) {
                -1.0
            } else {
                1.0
            },
            if flip.contains(FlipFlags::FLIP_VERTICAL) {
                -1.0
            } else {
                1.0
            },
        );

        let scale_origin = vec3(
            self.get_value(ScaleOriginX),
            self.get_value(ScaleOriginY),
            0.0,
        );

        let total_scale = vec2(
            (self.get_value(ScaleX) * 0.001)
                * (self.get_value(ScaleX2) * 0.001)
                * self.evaluate_wobbler(
                    &self.wobbler_scale_x,
                    WobbleScaleXAmplitude,
                    WobbleScaleXBias,
                    0.001,
                    1.0,
                ),
            (self.get_value(ScaleY) * 0.001)
                * (self.get_value(ScaleY2) * 0.001)
                * self.evaluate_wobbler(
                    &self.wobbler_scale_y,
                    WobbleScaleYAmplitude,
                    WobbleScaleYBias,
                    0.001,
                    1.0,
                ),
        ) * flip_scale;

        let rotation_origin = vec3(
            self.get_value(RotationOriginX),
            self.get_value(RotationOriginY),
            0.0,
        );

        let total_rotation = (self.get_value(Rotation) * 0.001
            + self.get_value(Rotation2) * 0.001
            + self.evaluate_wobbler(
                &self.wobbler_rotation,
                WobbleRotationAmplitude,
                WobbleRotationBias,
                0.001,
                0.0,
            ))
            * std::f32::consts::TAU;

        let total_translation = vec3(
            self.get_value(TranslateX) + self.get_value(TranslateX2)
                - self.get_value(NegatedTranslationX),
            self.get_value(TranslateY) + self.get_value(TranslateY2)
                - self.get_value(NegatedTranslationY),
            self.get_value(TranslateZ),
        );

        result = multiply_transforms(Mat4::from_translation(-scale_origin), result);
        result = multiply_transforms(Mat4::from_scale(total_scale.extend(1.0)), result);
        result = multiply_transforms(
            Mat4::from_translation(scale_origin - rotation_origin),
            result,
        );
        result = multiply_transforms(Mat4::from_rotation_z(total_rotation), result);
        result = multiply_transforms(
            Mat4::from_translation(total_translation + rotation_origin),
            result,
        );

        result
    }

    fn get_wobble_translation(&self) -> Vec2 {
        use LayerProperty::*;

        vec2(
            self.evaluate_wobbler(&self.wobbler_x, WobbleXAmplitude, WobbleXBias, 1.0, 0.0),
            self.evaluate_wobbler(&self.wobbler_y, WobbleYAmplitude, WobbleYBias, 1.0, 0.0),
        )
    }

    pub fn get_transform_params(&self) -> TransformParams {
        use LayerProperty::*;

        let transform = self.get_transform();

        let camera_position = vec3(
            self.get_value(CameraPositionX),
            self.get_value(CameraPositionY),
            self.get_value(CameraPositionZ),
        );
        let unconditionally_inherited_translation = vec2(
            self.get_value(UnconditionallyInheritedTranslationX),
            self.get_value(UnconditionallyInheritedTranslationY),
        );
        let wobble_translation = self.get_wobble_translation();

        TransformParams {
            transform,
            camera_position,
            unconditionally_inherited_translation,
            wobble_translation,
        }
    }

    pub fn get_compose_flags(&self) -> ComposeFlags {
        // Native754b4/754b8: clamp below zero, then saturating unsigned conversion.
        let flags = self.get_value(LayerProperty::ComposeFlags).max(0.0) as u32;
        ComposeFlags::from_bits_truncate(flags as i32)
    }

    pub fn get_composed_transform_params(
        &self,
        parent_transform: &TransformParams,
    ) -> TransformParams {
        let mut transform = self.get_transform_params();
        transform.compose_with(parent_transform, self.get_compose_flags());

        transform
    }

    pub fn get_ghosting_transform(&self) -> Mat4 {
        use LayerProperty::*;

        let transform_origin = vec2(
            self.get_value(GhostingTransformOriginX),
            self.get_value(GhostingTransformOriginY),
        )
        .extend(0.0);

        Mat4::from_translation(transform_origin)
            * Mat4::from_rotation_z(
                self.get_value(GhostingRotation) * 0.001 * std::f32::consts::TAU,
            )
            * Mat4::from_scale((Vec2::ONE * self.get_value(GhostingZoom) * 0.001).extend(1.0))
            * Mat4::from_translation(-transform_origin)
    }
}

impl LayerPropertiesData {
    fn advance(&mut self, dt: Ticks, are_animations_allowed: bool) {
        if are_animations_allowed {
            for property in self.properties.values_mut() {
                property.update(dt);
            }
        }

        macro_rules! get {
            ($property:ident) => {
                self.get_value(LayerProperty::$property)
            };
        }

        macro_rules! get_ticks {
            ($property:ident) => {
                Ticks::from_f32(get!($property))
            };
        }

        macro_rules! wobble {
            ($wobbler_name:ident, $wobble_mode:ident, $wobble_period:ident) => {
                self.$wobbler_name
                    .update(dt, get!($wobble_mode), get_ticks!($wobble_period));
            };
        }

        wobble!(wobbler_x, WobbleXMode, WobbleXPeriod);
        wobble!(wobbler_y, WobbleYMode, WobbleYPeriod);
        wobble!(wobbler_alpha, WobbleAlphaMode, WobbleAlphaPeriod);
        wobble!(wobbler_rotation, WobbleRotationMode, WobbleRotationPeriod);
        wobble!(wobbler_scale_x, WobbleScaleXMode, WobbleScaleXPeriod);
        wobble!(wobbler_scale_y, WobbleScaleYMode, WobbleScaleYPeriod);
    }
}

/// The native drawable keeps a null property pointer until its accessor75c00.
/// Copies preserve that absence; update/fast-forward do not allocate it.
#[derive(Debug, Default)]
struct PropertyCell {
    state: std::sync::OnceLock<std::sync::Arc<parking_lot::Mutex<LayerPropertiesData>>>,
}
impl PropertyCell {
    fn lock(&self) -> parking_lot::ArcMutexGuard<parking_lot::RawMutex, LayerPropertiesData> {
        self.state
            .get_or_init(|| {
                std::sync::Arc::new(parking_lot::Mutex::new(LayerPropertiesData::new()))
            })
            .lock_arc()
    }
    fn initialized(data: LayerPropertiesData) -> Self {
        Self {
            state: std::sync::OnceLock::from(std::sync::Arc::new(parking_lot::Mutex::new(data))),
        }
    }
}

/// The pointer belongs to a layer; the pointed-to cell belongs to a property
/// object. Retained layer aliases share pointer replacement, while explicit
/// property sharing retains only the current cell.
#[derive(Debug)]
struct PropertyOwner {
    current: parking_lot::RwLock<std::sync::Arc<PropertyCell>>,
}
impl PropertyOwner {
    fn cell(&self) -> std::sync::Arc<PropertyCell> {
        self.current.read().clone()
    }
    fn lock(&self) -> parking_lot::ArcMutexGuard<parking_lot::RawMutex, LayerPropertiesData> {
        self.cell().lock()
    }
}

pub struct PropertyTweenerGuard {
    state: parking_lot::ArcMutexGuard<parking_lot::RawMutex, LayerPropertiesData>,
    property: LayerProperty,
}
impl std::ops::Deref for PropertyTweenerGuard {
    type Target = Tweener;
    fn deref(&self) -> &Tweener {
        self.state.property_tweener(self.property)
    }
}
impl std::ops::DerefMut for PropertyTweenerGuard {
    fn deref_mut(&mut self) -> &mut Tweener {
        self.state.property_tweener_mut(self.property)
    }
}

/// Independent clones copy property state; explicit sharing retains its owner.
#[derive(Debug)]
pub struct LayerProperties {
    inner: std::sync::Arc<PropertyOwner>,
}
impl Clone for LayerProperties {
    fn clone(&self) -> Self {
        // Native75988..759ac copies only if the source pointer is nonnull.
        let cell = self.inner.cell();
        let inner = if let Some(state) = cell.state.get() {
            // Native73ec4..73f24 initializes six destination oscillators, then
            //74008..74050 overwrites their full state with the source state.
            for _ in 0..6 {
                let _ = crate::engine_random::next_seed();
            }
            PropertyCell::initialized(state.lock().clone())
        } else {
            PropertyCell::default()
        };
        Self::from_cell(std::sync::Arc::new(inner))
    }
}
impl LayerProperties {
    #[cfg(test)]
    pub(crate) fn reference_copy_words(&self) -> Option<Vec<u32>> {
        self.inner.cell().state.get().map(|state| {
            let data = state.lock();
            let mut words: Vec<_> = data
                .properties
                .values()
                .map(|t| t.value().to_bits())
                .collect();
            for wobbler in [
                &data.wobbler_x,
                &data.wobbler_y,
                &data.wobbler_rotation,
                &data.wobbler_scale_x,
                &data.wobbler_scale_y,
                &data.wobbler_alpha,
            ] {
                words.extend(wobbler.reference_copy_words());
            }
            words
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_initialized_frame(
        &self,
    ) -> Option<shin_core::primitives::update::FrameId> {
        self.inner
            .cell()
            .state
            .get()
            .map(|state| state.lock().updated_frame)
    }
    #[cfg(test)]
    pub(crate) fn reference_tweener_snapshot(&self, property: LayerProperty) -> Option<Tweener> {
        // Observing a wait must not initialize lazy properties or consume RNG.
        self.inner
            .cell()
            .state
            .get()
            .map(|state| state.lock().property_tweener(property).clone())
    }
    #[cfg(test)]
    pub(crate) fn reference_install_native_state(
        &mut self,
        values: &[f32; 90],
        wobblers: &[(u32, i32, f32); 6],
        advance: Option<f32>,
    ) {
        let mut data = self.inner.lock();
        for (prop, tween) in &mut data.properties {
            tween.fast_forward_to(values[prop as usize]);
        }
        [
            data.wobbler_x,
            data.wobbler_y,
            data.wobbler_scale_x,
            data.wobbler_scale_y,
            data.wobbler_rotation,
            data.wobbler_alpha,
        ] = wobblers.map(|(seed, mode, time)| Wobbler::reference_with_state(seed, mode, time));
        if let Some(delta) = advance {
            data.advance(Ticks::from_f32(delta), false);
        }
    }
    pub fn new() -> Self {
        Self::from_cell(std::sync::Arc::new(PropertyCell::default()))
    }
    pub fn from_state(state: &LayerPropertiesState, layer_id: LayerId, load_counter: u32) -> Self {
        Self::from_cell(std::sync::Arc::new(PropertyCell::initialized(
            LayerPropertiesData::from_state(state, layer_id, load_counter),
        )))
    }
    fn from_cell(cell: std::sync::Arc<PropertyCell>) -> Self {
        Self {
            inner: std::sync::Arc::new(PropertyOwner {
                current: parking_lot::RwLock::new(cell),
            }),
        }
    }
    pub(crate) fn rebind(&mut self, properties: Self) {
        let cell = properties.inner.cell();
        *self.inner.current.write() = cell;
    }
    pub(crate) fn share(&self) -> Self {
        // LAYERLOAD retains the result of the native property accessor.
        let cell = self.inner.cell();
        drop(cell.lock());
        Self::from_cell(cell)
    }
    pub(crate) fn retain_for_clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
    pub(crate) fn identity(&self) -> usize {
        std::sync::Arc::as_ptr(&self.inner.cell()) as usize
    }
    #[cfg(test)]
    pub(crate) fn initialized_identity(&self) -> Option<usize> {
        let cell = self.inner.cell();
        cell.state
            .get()
            .map(|_| std::sync::Arc::as_ptr(&cell) as usize)
    }
    pub fn property_tweener(&self, property: LayerProperty) -> Tweener {
        self.inner.lock().property_tweener(property).clone()
    }
    pub fn property_tweener_mut(&mut self, property: LayerProperty) -> PropertyTweenerGuard {
        PropertyTweenerGuard {
            state: self.inner.lock(),
            property,
        }
    }
    pub fn get_value(&self, property: LayerProperty) -> f32 {
        self.inner.lock().get_value(property)
    }
    pub fn init(&mut self) {
        self.inner.lock().init()
    }
    pub fn fast_forward(&mut self) {
        if let Some(state) = self.inner.cell().state.get() {
            state.lock().fast_forward();
        }
    }
    pub fn get_layer_id(&self) -> LayerId {
        self.inner.lock().get_layer_id()
    }
    pub fn set_layer_id(&mut self, layer_id: LayerId) {
        self.inner.lock().set_layer_id(layer_id)
    }
    pub fn set_layerload_counter1(&mut self, layer_load_counter1: u32) {
        self.inner
            .lock()
            .set_layerload_counter1(layer_load_counter1)
    }
    pub(crate) fn layer_load_counter(&self) -> u32 {
        self.inner.lock().layer_load_counter1
    }
    /// Native container restoration preserves identifiers and oscillator state.
    pub(crate) fn restore_values(&mut self, state: &LayerPropertiesState) {
        let mut data = self.inner.lock();
        for (property, value) in &state.properties {
            data.properties[property].fast_forward_to(*value as f32);
        }
    }
    /// Native hydration resets the existing 90 tween values and identifiers,
    /// preserving the retained drawable's oscillator state and property owner.
    pub(crate) fn restore_state(
        &mut self,
        state: &LayerPropertiesState,
        layer_id: LayerId,
        load_counter: u32,
    ) {
        let mut data = self.inner.lock();
        for (property, value) in &state.properties {
            data.properties[property].fast_forward_to(*value as f32);
        }
        data.layer_id = layer_id;
        data.layer_load_counter1 = load_counter;
    }
    pub fn is_visible(&self) -> bool {
        self.inner.lock().is_visible()
    }
    pub fn get_color_multiplier(&self) -> FloatColor4 {
        self.inner.lock().get_color_multiplier()
    }
    pub fn get_blend_type(&self) -> LayerBlendType {
        self.inner.lock().get_blend_type()
    }
    pub fn get_fragment_shader(&self) -> LayerFragmentShader {
        self.inner.lock().get_fragment_shader()
    }
    pub fn get_fragment_shader_param(&self) -> Vec4 {
        self.inner.lock().get_fragment_shader_param()
    }
    pub fn is_fragment_shader_nontrivial(&self) -> bool {
        self.inner.lock().is_fragment_shader_nontrivial()
    }
    pub fn is_blending_nontrivial(&self) -> bool {
        self.inner.lock().is_blending_nontrivial()
    }
    pub fn get_drawable_params(&self) -> DrawableParams {
        self.inner.lock().get_drawable_params()
    }
    pub fn get_clip_mode(&self) -> DrawableClipMode {
        self.inner.lock().get_clip_mode()
    }
    pub fn get_clip_params(&self) -> DrawableClipParams {
        self.inner.lock().get_clip_params()
    }
    pub fn get_transform_params(&self) -> TransformParams {
        self.inner.lock().get_transform_params()
    }
    pub fn get_composed_transform_params(
        &self,
        parent_transform: &TransformParams,
    ) -> TransformParams {
        self.inner
            .lock()
            .get_composed_transform_params(parent_transform)
    }
    pub fn get_ghosting_transform(&self) -> Mat4 {
        self.inner.lock().get_ghosting_transform()
    }
}
impl LayerProperties {
    fn advance(
        &mut self,
        frame: shin_core::primitives::update::FrameId,
        delta: Ticks,
        allowed: bool,
    ) {
        // Native75e08/78848 skip the property update when still absent.
        let cell = self.inner.cell();
        let Some(state) = cell.state.get() else {
            return;
        };
        let mut state = state.lock();
        // Native6aa40 guards every property object, including independent
        // copies, before updating90 tweens and all six wobblers757d0.
        if state.updated_frame == frame {
            return;
        }
        state.updated_frame = frame;
        state.advance(delta, allowed);
    }
}

impl AdvUpdatable for LayerProperties {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.advance(
            context.frame_id,
            context.delta_ticks,
            context.are_animations_allowed,
        );
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod native_transform_tests;

#[cfg(test)]
mod native_queue_tests;

#[cfg(test)]
mod native_copy_tests;

/// Stores only target property values.
/// Used to implement save/load (to quickly restore the state of the scene).
#[derive(Debug, Clone)]
pub struct LayerPropertiesState {
    // The game can actually only set integer values
    // hence the the use of i32 instead of f32
    properties: EnumMap<LayerProperty, i32>,
}

impl LayerPropertiesState {
    pub fn new() -> Self {
        Self {
            properties: initial_values(),
        }
    }

    pub fn init(&mut self) {
        self.properties = initial_values();
    }

    #[allow(unused)]
    pub fn get_property(&self, property: LayerProperty) -> i32 {
        self.properties[property]
    }

    pub fn set_property(&mut self, property: LayerProperty, value: i32) {
        self.properties[property] = value;
    }
}
