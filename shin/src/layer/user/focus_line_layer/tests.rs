use super::*;
use crate::{
    layer::{DrawableLayer, Layer},
    render::PreRenderContext,
};
use shin_core::{
    primitives::{color::UnormColor, update::FrameId},
    time::{Ticks, Tweener},
    vm::command::types::LayerProperty,
};
use shin_render::shaders::types::{
    RenderClone,
    texture::{TextureTarget, TextureTargetKind},
};

impl FocusLineLayer {
    pub(crate) fn assert_reference_render(
        &mut self,
        context: &mut PreRenderContext,
        server: &Arc<AssetServer>,
    ) {
        assert_eq!(self.inner_ref().offset, Vec2::new(-200.0, 100.0));
        let textures = self.inner_ref().textures.clone();
        *self = Self::from_inner(FocusLineLayerImpl {
            textures,
            offset: Vec2::new(-200.0, 100.0),
            rays: FocusModel::Legacy(FocusLineRays::new(1)),
        });
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Native focus line"),
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
        let draw = |layer: &Self, context: &mut PreRenderContext| {
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Native focus line",
                );
                pass.clear(Some(UnormColor::BLACK), None, None);
                layer.inner_ref().render_drawable_direct(
                    &mut pass,
                    &TransformParams::default(),
                    &layer.properties().get_drawable_params(),
                    &layer.properties().get_clip_params(),
                    0,
                    PassKind::Transparent,
                );
            }
            super::super::movie_layer::readback_test_texture(context, &target)
        };
        let initial = draw(self, context);
        let visible = |pixels: &[u8]| {
            pixels
                .chunks_exact(4)
                .filter(|p| p[0].max(p[1]).max(p[2]) > 5)
                .count()
        };
        assert!(
            visible(&initial) > 1000,
            "Focus texture and radial rays must be visible"
        );
        let copy = self.render_clone(&mut context.render_clone_ctx());
        assert_eq!(
            initial,
            draw(&copy, context),
            "Clone preserves ray angles/alpha"
        );
        self.properties_mut()
            .property_tweener_mut(LayerProperty::Prop27)
            .fast_forward_to(0.0);
        let mut frame = FrameId::default();
        frame.advance();
        let mut update = AdvUpdateContext {
            frame_id: frame,
            delta_ticks: Ticks::from_f32(50.0),
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: false,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        };
        self.update(&update);
        assert!(
            self.inner_ref()
                .rays
                .legacy()
                .rays
                .iter()
                .all(|r| r.alpha.is_idle())
        );
        assert_eq!(
            visible(&draw(self, context)),
            0,
            "Native expiry leaves invisible rays until next update"
        );
        update.delta_ticks = Ticks::ZERO;
        self.update(&update);
        assert!(
            self.inner_ref()
                .rays
                .legacy()
                .rays
                .iter()
                .all(|r| r.alpha.is_idle()),
            "Duplicate frame cannot refill"
        );
        frame.advance();
        update.frame_id = frame;
        self.update(&update);
        assert_eq!(self.inner_ref().rays.legacy().rays.len(), 40);
        assert!(
            self.inner_ref()
                .rays
                .legacy()
                .rays
                .iter()
                .all(|r| r.alpha.value() == 1.0)
        );
        assert!(visible(&draw(self, context)) > 1000);
        let before: Vec<_> = self
            .inner_ref()
            .rays
            .legacy()
            .rays
            .iter()
            .map(|r| (r.angle, r.alpha.value()))
            .collect();
        self.fast_forward();
        assert_eq!(
            before,
            self.inner_ref()
                .rays
                .legacy()
                .rays
                .iter()
                .map(|r| (r.angle, r.alpha.value()))
                .collect::<Vec<_>>()
        );
        // A single radial ray isolates the native quad and parameter offset.
        self.inner_mut().rays.legacy_mut().rays = vec![super::super::focus_line_model::Ray {
            angle: 0.0,
            alpha: Tweener::new(1.0),
        }];
        let one = draw(self, context);
        let mut count = 0;
        for (index, pixel) in one.chunks_exact(4).enumerate() {
            if pixel[0].max(pixel[1]).max(pixel[2]) > 5 {
                let (x, y) = (index % 1920, index / 1920);
                assert!(
                    (910..1920).contains(&x) && (625..655).contains(&y),
                    "Native ray bounds:{x},{y}"
                );
                count += 1;
            }
        }
        assert!(count > 100);
        self.properties_mut()
            .property_tweener_mut(LayerProperty::MulColorAlpha)
            .fast_forward_to(0.0);
        assert_eq!(visible(&draw(self, context)), 0);
        println!(
            "Native FocusLine: actual type7 loader/texture,40 warmed rays, clone pixels, animation/Prop27-independent fade, expiry-before-refill, duplicate guard, fast-forward retention,150..2070 ray bounds and parameter offsets pass"
        );
    }
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU or audio required"]
fn reference_native_focus_line_factory_rng_lifecycle_and_geometry() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
        .step_by(24)
        .map(|offset| {
            (
                u64::from_le_bytes(exe[offset + 256..offset + 264].try_into().unwrap()),
                u64::from_le_bytes(exe[offset + 272..offset + 280].try_into().unwrap()),
            )
        })
        .collect();
    for (address, expected) in [
        (0x224db8, 0x1d81f0),
        (0x1d8400, 0x1701bd),
        (0x224c60, 0x1d7378),
        (0x1d7398, 0x6aa40),
        (0x1d73a0, 0x67520),
        (0x1d73b0, 0x674c0),
        (0x1d7410, 0x676e0),
    ] {
        assert_eq!(relocations.get(&address), Some(&expected));
    }
    let jump = u16::from_le_bytes(exe[0x1653ee + 256..0x1653f0 + 256].try_into().unwrap());
    assert_eq!(0x20608 + u32::from(jump) * 4, 0x2084c);
    for (address, expected) in [
        (0x205d0, 0x7100245fu32),
        (0x205f0, 0x7868792bu32),
        (0x20604, 0xd61f0140u32),
        (0x2084c, 0x2d4006a0u32),
        (0x208ac, 0x94018e49u32),
        (0x8439c, 0x97ff89e9u32),
        (0x66bcc, 0x94003ca1u32),
        (0x66bfc, 0x940253b1u32),
        (0x66c30, 0xf9004261u32),
        (0x66c44, 0x940000abu32),
        (0x66c5c, 0x1e27d009u32),
        (0x66cbc, 0x1e254000u32),
        (0x66fec, 0x1e2a0800u32),
        (0x66ff0, 0x1e2b0801u32),
        (0x66ff4, 0x1e2c2820u32),
        (0x6703c, 0x1e2d0803u32),
        (0x67040, 0x1e2e2861u32),
        (0x67048, 0x321f03e1u32),
        (0x67050, 0x9401526cu32),
        (0x6742c, 0xb9405ea8u32),
        (0x67430, 0xb9005e68u32),
        (0x67438, 0xf9003268u32),
        (0x67544, 0x940044b7u32),
        (0x67558, 0x94015126u32),
        (0x67684, 0x97fffe1bu32),
        (0x6769c, 0x940150edu32),
        (0x6777c, 0xd2a862c8u32),
        (0x67784, 0xd2a862c9u32),
        (0x67788, 0xb2091beau32),
        (0x67798, 0x32091be8u32),
        (0x6779c, 0xa93523a9u32),
        (0x677a4, 0xf2a8a028u32),
        (0x677b8, 0xf2a8a028u32),
        (0x678cc, 0x1e210800u32),
        (0x678dc, 0x9400e9b5u32),
        (0x678fc, 0x9400e825u32),
        (0x159598, 0x40c90fd0u32),
        (0x1597e0, 0xc0490fd0u32),
        (0x1594c4, 0x30000000u32),
    ] {
        assert_eq!(
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
            expected,
            "Native word{address:x}"
        );
    }
}
