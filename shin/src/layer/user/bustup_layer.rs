use std::{fmt::Debug, sync::Arc};

use glam::{Mat4, Vec3, vec3};
use shin_render::{
    PassKind, RenderRequestBuilder, render_pass::RenderPass, shaders::types::RenderClone,
};

use crate::{
    asset::bustup::Bustup,
    layer::{
        NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
        user::picture_layer::{PictureBlockParams, PictureBlockPassKind},
    },
    update::{AdvUpdatable, AdvUpdateContext, Updatable, UpdateContext},
};

#[derive(Clone, RenderClone)]
pub struct BustupLayerImpl {
    bustup: Arc<Bustup>,
    label: String,
    mouth_state: u32,
    eyes_state: u32,
    blink: super::bustup_blink::BustupBlink,
}

impl BustupLayerImpl {
    pub fn new(bustup: Arc<Bustup>, bustup_name: Option<String>) -> Self {
        let seed = crate::engine_random::next_seed();
        let blink = super::bustup_blink::BustupBlink::new(bustup.eye_blocks.len(), seed);
        Self {
            blink,
            bustup,
            label: bustup_name.unwrap_or_else(|| "unnamed".to_string()),
            mouth_state: 0,
            eyes_state: 0,
        }
    }

    pub fn character_id(&self) -> i32 {
        self.bustup.character_id.raw()
    }

    #[tracing::instrument(skip_all)]
    fn render_impl(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        params: PictureBlockParams,
        transform: Mat4,
    ) {
        let translation = Mat4::from_translation(vec3(
            -self.bustup.origin_x as f32,
            -self.bustup.origin_y as f32,
            0.0,
        ));
        // NOTE: this scale is a ¿device-specific? scale (which is 1.0 on switch)
        let scale = Mat4::from_scale(Vec3::ONE);

        let transform = transform * scale * translation;

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
        for block in &self.bustup.base_blocks {
            super::picture_layer::render_block(block, pass, builder, params, transform);
        }
        pass.pop_debug();

        if let Some(block) = &self.bustup.face1 {
            pass.push_debug("Face1");
            super::picture_layer::render_block(block, pass, builder, params, transform);
            pass.pop_debug();
        }

        if let Some(block) = &self.bustup.face2 {
            pass.push_debug("Face2");
            super::picture_layer::render_block(block, pass, builder, params, transform);
            pass.pop_debug();
        }

        if !self.bustup.mouth_blocks.is_empty() {
            pass.push_debug("Mouth");
            super::picture_layer::render_block(
                &self.bustup.mouth_blocks[self.mouth_state as usize],
                pass,
                builder,
                params,
                transform,
            );
            pass.pop_debug();
        }

        if !self.bustup.eye_blocks.is_empty() {
            pass.push_debug("Eyes");
            super::picture_layer::render_block(
                &self.bustup.eye_blocks[self.eyes_state as usize],
                pass,
                builder,
                params,
                transform,
            );
            pass.pop_debug();
        }

        pass.pop_debug();
    }
}

pub type BustupLayer = NewDrawableLayerWrapper<BustupLayerImpl>;

impl BustupLayer {
    #[cfg(test)]
    pub(crate) fn reference_resource_owner(&self) -> std::sync::Weak<Bustup> {
        Arc::downgrade(&self.inner_ref().bustup)
    }
    #[cfg(test)]
    pub(crate) fn reference_resource_identity(&self) -> usize {
        Arc::as_ptr(&self.inner_ref().bustup) as usize
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
        inner.blink =
            super::bustup_blink::BustupBlink::new(inner.bustup.eye_blocks.len(), 0x12345678);
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
        let count = inner.bustup.mouth_blocks.len();
        inner.mouth_state = if enabled && count != 0 {
            (usize::from(level.unwrap_or(0)) * count / 3).min(count - 1) as u32
        } else {
            0
        };
    }

    #[cfg(test)]
    pub(crate) fn reference_mouth_state(&self) -> u32 {
        self.inner_ref().mouth_state
    }
    #[cfg(test)]
    pub(crate) fn reference_frame_counts(&self) -> (usize, usize) {
        let asset = &self.inner_ref().bustup;
        (asset.mouth_blocks.len(), asset.eye_blocks.len())
    }

    pub fn new(picture: Arc<Bustup>, picture_name: Option<String>) -> Self {
        Self::from_inner_constructor(|| BustupLayerImpl::new(picture, picture_name))
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
        let count = self.bustup.mouth_blocks.len();
        self.mouth_state = if enabled && count != 0 {
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
            (usize::from(level.unwrap_or(0)) * count / 3).min(count - 1) as u32
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
        let original = &layer.inner_ref().bustup;
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
