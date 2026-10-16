use std::{fmt::Debug, sync::Arc};

use glam::{Mat4, Vec3, vec3};
use shin_render::{
    PassKind, RenderRequestBuilder, render_pass::RenderPass, shaders::types::RenderClone,
};

use crate::{
    asset::{
        bustup::{Bustup, higurashi::HigurashiBustup},
        picture::GpuPictureBlock,
    },
    layer::{
        NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
        user::picture_layer::{PictureBlockParams, PictureBlockPassKind},
    },
    update::{AdvUpdatable, AdvUpdateContext, Updatable, UpdateContext},
};

#[derive(Clone, RenderClone)]
enum BustupResource {
    Umineko(Arc<Bustup>),
    Higurashi(Arc<HigurashiBustup>),
}

impl BustupResource {
    fn origin(&self) -> (i16, i16) {
        match self {
            Self::Umineko(b) => (b.origin_x, b.origin_y),
            Self::Higurashi(b) => (b.origin_x, b.origin_y),
        }
    }
    fn character_id(&self) -> i32 {
        match self {
            Self::Umineko(b) => b.character_id.raw(),
            Self::Higurashi(b) => b.character_id.raw(),
        }
    }
    fn eye_count(&self) -> usize {
        match self {
            Self::Umineko(b) => b.eye_blocks.len(),
            Self::Higurashi(_) => 0,
        }
    }
    fn base_blocks(&self) -> impl Iterator<Item = &Arc<GpuPictureBlock>> {
        let (umineko, higurashi) = match self {
            Self::Umineko(b) => (Some(&b.base_blocks), None),
            Self::Higurashi(b) => (None, Some(&b.body.blocks)),
        };
        umineko
            .into_iter()
            .flatten()
            .chain(higurashi.into_iter().flatten().filter_map(Option::as_ref))
    }
    fn face1(&self) -> Option<&Arc<GpuPictureBlock>> {
        match self {
            Self::Umineko(b) => b.face1.as_ref(),
            Self::Higurashi(b) => b.auxiliary.as_ref().and_then(|a| a.block.as_ref()),
        }
    }
    fn face2(&self) -> Option<&Arc<GpuPictureBlock>> {
        match self {
            Self::Umineko(b) => b.face2.as_ref(),
            Self::Higurashi(b) => b.face.as_ref(),
        }
    }
    fn mouth(&self, state: u32) -> Option<&Arc<GpuPictureBlock>> {
        match self {
            Self::Umineko(b) => b.mouth_blocks.get(state as usize),
            Self::Higurashi(b) => b.mouths.get(state as usize).and_then(Option::as_ref),
        }
    }
    fn eye(&self, state: u32) -> Option<&Arc<GpuPictureBlock>> {
        match self {
            Self::Umineko(b) => b.eye_blocks.get(state as usize),
            Self::Higurashi(b) => b.eye.as_ref(),
        }
    }
    fn mouth_count(&self) -> usize {
        match self {
            Self::Umineko(b) => b.mouth_blocks.len(),
            Self::Higurashi(b) => {
                if b.disable_animations {
                    0
                } else {
                    3
                }
            }
        }
    }
    fn mouth_state(&self, enabled: bool, level: Option<u8>) -> u32 {
        match self {
            Self::Umineko(b) => {
                if !enabled {
                    return 0;
                }
                let count = b.mouth_blocks.len();
                if count == 0 {
                    0
                } else {
                    (usize::from(level.unwrap_or(0)) * count / 3).min(count - 1) as u32
                }
            }
            Self::Higurashi(b) => {
                if b.disable_animations {
                    0
                } else {
                    // Updated369a4..369c4 resets out-of-range feedback to zero.
                    // It does not read Umineko's animation property27.
                    match level.unwrap_or(0) {
                        level @ 0..=2 => u32::from(level),
                        _ => 0,
                    }
                }
            }
        }
    }
    #[cfg(test)]
    fn umineko(&self) -> &Arc<Bustup> {
        let Self::Umineko(bustup) = self else {
            panic!("Umineko resource oracle used with Higurashi")
        };
        bustup
    }
}

#[derive(Clone, RenderClone)]
pub struct BustupLayerImpl {
    bustup: BustupResource,
    label: String,
    mouth_state: u32,
    eyes_state: u32,
    blink: super::bustup_blink::BustupBlink,
    ordered_origin: bool,
    /// Gerokasu per-entry placement `T(x, -y) * S(scale)`, applied between
    /// the layer transform and the origin translation (native 351c0).
    placement: Option<[f32; 3]>,
}

impl BustupLayerImpl {
    pub(super) fn new_higurashi(bustup: Arc<HigurashiBustup>, name: Option<String>) -> Self {
        Self::from_resource(BustupResource::Higurashi(bustup), name)
    }

    pub(super) fn origin(&self) -> (i16, i16) {
        self.bustup.origin()
    }

    pub(super) fn update_higurashi_voice(
        &mut self,
        properties: &crate::layer::higurashi_properties::HigurashiProperties,
        callback: Option<&dyn Fn(i32, f32) -> Option<u8>>,
    ) {
        let BustupResource::Higurashi(bustup) = &self.bustup else {
            panic!("Higurashi update requires an older resource");
        };
        let level = if bustup.disable_animations {
            None
        } else {
            callback.and_then(|callback| {
                let pan = properties
                    .local_transform()
                    .compute_final_transform()
                    .transform_point3(Vec3::ZERO)
                    .x
                    .clamp(-1.0, 1.0);
                callback(self.character_id(), pan)
            })
        };
        self.mouth_state = self.bustup.mouth_state(true, level);
    }

    pub fn new(bustup: Arc<Bustup>, bustup_name: Option<String>) -> Self {
        Self::from_resource(BustupResource::Umineko(bustup), bustup_name)
    }

    fn from_resource(bustup: BustupResource, bustup_name: Option<String>) -> Self {
        let seed = match &bustup {
            BustupResource::Umineko(_) => crate::engine_random::next_seed(),
            BustupResource::Higurashi(_) => 0,
        };
        let blink = super::bustup_blink::BustupBlink::new(bustup.eye_count(), seed);
        Self {
            blink,
            bustup,
            label: bustup_name.unwrap_or_else(|| "unnamed".to_string()),
            mouth_state: 0,
            eyes_state: 0,
            ordered_origin: false,
            placement: None,
        }
    }

    pub fn character_id(&self) -> i32 {
        self.bustup.character_id()
    }

    pub(crate) fn new_gerokasu2(bustup: Arc<Bustup>, name: String, seed: u32) -> Self {
        Self {
            blink: super::bustup_blink::BustupBlink::new_gerokasu2(bustup.eye_blocks.len(), seed),
            bustup: BustupResource::Umineko(bustup),
            label: name,
            mouth_state: 0,
            eyes_state: 0,
            ordered_origin: true,
            placement: None,
        }
    }

    pub(crate) fn set_placement(&mut self, [scale, x, y]: [f32; 3]) {
        self.placement = Some([scale, x, y]);
    }

    /// MSGFACE installs neutral property27, so its animation permission is
    /// independent of the root's animation flag (native3b0a0/3b16c).
    pub(crate) fn update_portrait(&mut self, ticks: f32, level: Option<u8>) {
        let count = self.bustup.mouth_count();
        self.mouth_state = if count == 0 {
            0
        } else {
            (usize::from(level.unwrap_or(0)) * count / 3).min(count - 1) as u32
        };
        self.eyes_state = self.blink.update(ticks, true);
    }

    #[cfg(test)]
    pub(crate) fn portrait_animation_state(&self) -> (u32, u32) {
        (self.mouth_state, self.eyes_state)
    }

    pub(crate) fn render_gerokasu2(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        params: PictureBlockParams,
        transform: Mat4,
    ) {
        let (origin_x, origin_y) = self.bustup.origin();
        let mut translation =
            Mat4::from_translation(vec3(-origin_x as f32, -origin_y as f32, 0.0));
        if let Some([scale, x, y]) = self.placement {
            translation = Mat4::from_translation(vec3(x, -y, 0.0))
                * Mat4::from_scale(vec3(scale, scale, 1.0))
                * translation;
        }
        let transform = crate::layer::render_params::multiply_transforms(transform, translation);
        self.render_blocks_with_asset_transform(pass, builder, params, transform);
    }

    #[tracing::instrument(skip_all)]
    fn render_impl(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        params: PictureBlockParams,
        transform: Mat4,
    ) {
        if self.ordered_origin {
            self.render_gerokasu2(pass, builder, params, transform);
            return;
        }
        let (origin_x, origin_y) = self.bustup.origin();
        let translation = Mat4::from_translation(vec3(-origin_x as f32, -origin_y as f32, 0.0));
        // NOTE: this scale is a ¿device-specific? scale (which is 1.0 on switch)
        let scale = Mat4::from_scale(Vec3::ONE);

        let transform = transform * scale * translation;

        self.render_blocks_with_asset_transform(pass, builder, params, transform);
    }

    pub(super) fn render_blocks_with_asset_transform(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        params: PictureBlockParams,
        transform: Mat4,
    ) {
        let render_block = match &self.bustup {
            BustupResource::Umineko(_) => super::picture_layer::render_block,
            BustupResource::Higurashi(_) => super::higurashi::block::render,
        };
        self.render_blocks_using(pass, builder, params, transform, render_block);
    }

    /// Older caller supplies the native matrix for conservative Core culling.
    pub(super) fn render_higurashi_blocks_with_asset_transform(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        params: PictureBlockParams,
        native_transform: Mat4,
    ) {
        debug_assert!(matches!(self.bustup, BustupResource::Higurashi(_)));
        self.render_blocks_using(
            pass,
            builder,
            params,
            native_transform,
            super::higurashi::block::render_native,
        );
    }

    fn render_blocks_using(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        params: PictureBlockParams,
        transform: Mat4,
        render_block: fn(
            &GpuPictureBlock,
            &mut RenderPass,
            RenderRequestBuilder,
            PictureBlockParams,
            Mat4,
        ),
    ) {
        pass.push_debug(&format!(
            "BustupLayer[{}]/{}",
            &self.label,
            match params.pass_kind {
                PictureBlockPassKind::OpaqueOnly => "opaque",
                PictureBlockPassKind::TransparentOnly => "transparent",
                PictureBlockPassKind::OpaqueAndTransparent => "opaque_and_transparent",
            }
        ));

        pass.push_debug("Base");
        for block in self.bustup.base_blocks() {
            render_block(block, pass, builder, params, transform);
        }
        pass.pop_debug();

        if let Some(block) = self.bustup.face1() {
            pass.push_debug("Face1");
            render_block(block, pass, builder, params, transform);
            pass.pop_debug();
        }

        if let Some(block) = self.bustup.face2() {
            pass.push_debug("Face2");
            render_block(block, pass, builder, params, transform);
            pass.pop_debug();
        }

        if let Some(block) = self.bustup.mouth(self.mouth_state) {
            pass.push_debug("Mouth");
            render_block(block, pass, builder, params, transform);
            pass.pop_debug();
        }

        if let Some(block) = self.bustup.eye(self.eyes_state) {
            pass.push_debug("Eyes");
            render_block(block, pass, builder, params, transform);
            pass.pop_debug();
        }

        pass.pop_debug();
    }
}

pub type BustupLayer = NewDrawableLayerWrapper<BustupLayerImpl>;

impl BustupLayer {
    #[cfg(test)]
    pub(crate) fn reference_resource_owner(&self) -> std::sync::Weak<Bustup> {
        Arc::downgrade(self.inner_ref().bustup.umineko())
    }
    #[cfg(test)]
    pub(crate) fn reference_resource_identity(&self) -> usize {
        Arc::as_ptr(self.inner_ref().bustup.umineko()) as usize
    }

    #[cfg(test)]
    pub(crate) fn reference_blink_state(&self) -> (u32, u32, u32, Vec<u32>) {
        self.inner_ref().blink.reference_state()
    }

    #[cfg(test)]
    pub(crate) fn reference_resource_clone_blink(&mut self) {
        // Match the bounded native resource oracle's seed service and prepared
        // elapsed clock. The fixture compares cloning,not RNG/update timing.
        let inner = self.inner_mut();
        inner.blink = super::bustup_blink::BustupBlink::new(inner.bustup.eye_count(), 0x12345678);
        inner.blink.update(1.0, true);
    }

    #[cfg(test)]
    pub(crate) fn reference_advance_blink(&mut self) {
        self.inner_mut().blink.update(1.0, true);
    }
    #[cfg(test)]
    pub(crate) fn set_voice_lipsync(&mut self, level: Option<u8>) {
        use crate::layer::DrawableLayer;
        use shin_core::vm::command::types::LayerProperty;
        let enabled = self.properties().get_value(LayerProperty::Prop27) as i32 != 0;
        let inner = self.inner_mut();
        inner.mouth_state = inner.bustup.mouth_state(enabled, level);
    }

    #[cfg(test)]
    pub(crate) fn reference_mouth_state(&self) -> u32 {
        self.inner_ref().mouth_state
    }
    #[cfg(test)]
    pub(crate) fn reference_frame_counts(&self) -> (usize, usize) {
        let asset = self.inner_ref().bustup.umineko();
        (asset.mouth_blocks.len(), asset.eye_blocks.len())
    }

    pub fn new_higurashi(bustup: Arc<HigurashiBustup>, name: Option<String>) -> Self {
        Self::from_inner_constructor(|| BustupLayerImpl::new_higurashi(bustup, name))
    }

    pub fn new(picture: Arc<Bustup>, picture_name: Option<String>) -> Self {
        Self::from_inner_constructor(|| BustupLayerImpl::new(picture, picture_name))
    }
    pub(crate) fn new_gerokasu2(bustup: Arc<Bustup>, name: String) -> Self {
        Self::from_inner_constructor(|| {
            BustupLayerImpl::new_gerokasu2(bustup, name, crate::engine_random::next_seed())
        })
    }
}

impl NewDrawableLayerNeedsSeparatePass for BustupLayerImpl {}

impl NewDrawableLayer for BustupLayerImpl {
    fn update_with_properties(
        &mut self,
        context: &AdvUpdateContext,
        props: &crate::layer::LayerProperties,
    ) {
        use shin_core::vm::command::types::LayerProperty;
        // Native0x36aa0 passes the animation permission to the property update,
        // but its blink clock is controlled by integer property27 alone.
        let enabled = props.get_value(LayerProperty::Prop27) as i32 != 0;
        let count = self.bustup.mouth_count();
        let poll_voice = match &self.bustup {
            BustupResource::Umineko(_) => enabled && count != 0,
            BustupResource::Higurashi(b) => !b.disable_animations,
        };
        self.mouth_state = if poll_voice {
            let level = context.voice_callback.and_then(|callback| {
                // Native369a0 ->754d0 ->71b70 projects the local property
                // transform and its own wobble, without composing a parent.
                let transform = props.get_transform_params();
                let pan = transform
                    .compute_final_transform()
                    .transform_point3(Vec3::ZERO)
                    .x
                    .clamp(-1.0, 1.0);
                callback(self.character_id(), pan)
            });
            self.bustup.mouth_state(enabled, level)
        } else {
            0
        };
        self.eyes_state = self.blink.update(context.delta_ticks.as_f32(), enabled);
    }

    #[tracing::instrument(skip_all)]
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        drawable: &DrawableParams,
        clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        let Some(params) = PictureBlockParams::setup(pass_kind, drawable) else {
            return;
        };

        assert_eq!(
            clip.mode,
            DrawableClipMode::None,
            "Clipping effect is not implemented"
        );

        let builder =
            RenderRequestBuilder::new().depth_stencil_shorthand(stencil_ref, false, false);
        let transform = transform.compute_final_transform();

        self.render_impl(pass, builder, params, transform);
    }
}

impl NewDrawableLayerFastForward for BustupLayerImpl {
    fn fast_forward(&mut self) {}
}

impl AdvUpdatable for BustupLayerImpl {
    fn update(&mut self, _ctx: &AdvUpdateContext) {}
}

impl Debug for BustupLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BustupLayer").field(&self.label).finish()
    }
}

#[cfg(test)]
impl BustupLayer {
    pub(crate) fn assert_reference_lipsync(
        context: &mut crate::render::PreRenderContext,
        asset: Arc<Bustup>,
        server: &Arc<crate::asset::system::AssetServer>,
    ) {
        use crate::layer::{DrawableLayer, Layer};
        use shin_core::{primitives::color::UnormColor, vm::command::types::LayerProperty};
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        super::bustup_voice_tests::reference_callbacks(context, server, asset.clone());
        let count = asset.mouth_blocks.len();
        assert!(
            count >= 3,
            "Reference bustup requires three mouth frames, found{count}"
        );
        let same_blocks: Vec<_> = (0..2)
            .map(|level| {
                Arc::ptr_eq(
                    &asset.mouth_blocks[level * count / 3],
                    &asset.mouth_blocks[(level + 1) * count / 3],
                )
            })
            .collect();
        let mut layer = Self::new(asset, None);
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Reference bustup mouths"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let render = |layer: &mut Self, context: &mut crate::render::PreRenderContext| {
            layer.pre_render(context, &Default::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Reference bustup mouth",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), None);
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    layer.render(&mut pass, &Default::default(), 1, kind);
                }
            }
            super::movie_layer::readback_test_texture(context, &target)
        };
        let mut frames = Vec::new();
        for level in 0..3 {
            layer.set_voice_lipsync(Some(level));
            assert_eq!(
                layer.reference_mouth_state(),
                u32::from(level) * count as u32 / 3
            );
            frames.push(render(&mut layer, context));
        }
        let mut distinct_pixels = 0;
        for (index, pair) in frames.windows(2).enumerate() {
            let changed = pair[0]
                .chunks_exact(4)
                .zip(pair[1].chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count();
            if same_blocks[index] {
                assert_eq!(
                    changed, 0,
                    "Shared authored mouth block must retain its pixels"
                );
            } else {
                assert!(
                    changed > 100,
                    "Distinct mouth block must change visible pixels: {changed}"
                );
                distinct_pixels += changed;
            }
            println!("Reference native bustup mouths{count}: changed{changed} pixels");
        }
        assert!(distinct_pixels > 100);
        layer.set_voice_lipsync(None);
        assert_eq!(render(&mut layer, context), frames[0]);
        layer.set_voice_lipsync(Some(2));
        layer
            .properties_mut()
            .property_tweener_mut(LayerProperty::Prop27)
            .fast_forward_to(0.0);
        layer.set_voice_lipsync(Some(2));
        assert_eq!(layer.reference_mouth_state(), 0);
        assert_eq!(render(&mut layer, context), frames[0]);
        // No supplied Umineko expression has eye blocks. Exercise the native
        // eye-render path using three distinct uploaded authored mouth images.
        let original = layer.inner_ref().bustup.umineko();
        let synthetic = Arc::new(Bustup {
            character_id: original.character_id,
            origin_x: original.origin_x,
            origin_y: original.origin_y,
            effective_width: original.effective_width,
            effective_height: original.effective_height,
            bustup_id: original.bustup_id,
            base_blocks: original.base_blocks.clone(),
            face1: original.face1.clone(),
            face2: original.face2.clone(),
            mouth_blocks: Vec::new(),
            eye_blocks: vec![
                original.mouth_blocks[0].clone(),
                original.mouth_blocks[1].clone(),
                original.mouth_blocks[2].clone(),
            ],
        });
        let mut blinking = Self::new(synthetic, None);
        blinking.inner_mut().blink = super::bustup_blink::BustupBlink::new(3, 1);
        let mut eye_frames = Vec::new();
        let mut frame_id = shin_core::primitives::update::FrameId::default();
        let update =
            |layer: &mut Self, context: &crate::render::PreRenderContext, frame_id, ticks| {
                let update = AdvUpdateContext {
                    frame_id,
                    delta_ticks: shin_core::time::Ticks::from_f32(ticks),
                    asset_server: server,
                    device: context.device,
                    queue: context.queue,
                    are_animations_allowed: false,
                    voice_callback: None,
                    movie_volume_settings: Default::default(),
                };
                crate::update::AdvUpdatable::update(layer, &update);
            };
        for (ticks, expected) in [(320.0, 0), (4.0, 1), (4.0, 2), (4.0, 1), (4.0, 0), (4.0, 0)] {
            frame_id.advance();
            update(&mut blinking, context, frame_id, ticks);
            assert_eq!(blinking.inner_ref().eyes_state, expected);
            eye_frames.push(render(&mut blinking, context));
        }
        let changed = |a: &[u8], b: &[u8]| {
            a.chunks_exact(4)
                .zip(b.chunks_exact(4))
                .filter(|(a, b)| a != b)
                .count()
        };
        assert!(changed(&eye_frames[0], &eye_frames[1]) > 100);
        assert!(changed(&eye_frames[1], &eye_frames[2]) > 100);
        assert_eq!(eye_frames[1], eye_frames[3]);
        assert_eq!(eye_frames[0], eye_frames[4]);
        assert_eq!(eye_frames[0], eye_frames[5]);
        blinking.inner_mut().blink = super::bustup_blink::BustupBlink::new(3, 1);
        frame_id.advance();
        update(&mut blinking, context, frame_id, 328.0);
        assert_eq!(blinking.inner_ref().eyes_state, 2);
        blinking
            .properties_mut()
            .property_tweener_mut(LayerProperty::Prop27)
            .fast_forward_to(0.0);
        frame_id.advance();
        update(&mut blinking, context, frame_id, 5.0);
        assert_eq!(blinking.inner_ref().eyes_state, 0);
        assert_eq!(render(&mut blinking, context), eye_frames[0]);
        println!(
            "Synthetic native blink: forward/backward frames and property27 reset match uploaded source images; animation permission false still advances the native blink clock"
        );
    }
}
