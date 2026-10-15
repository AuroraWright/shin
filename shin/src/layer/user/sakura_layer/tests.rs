use super::*;
use crate::asset::{
    effects::{EffectTextures, EffectsAtlas},
    texture_archive::TextureArchiveBuilder,
};
use crate::{
    layer::{DrawableLayer, Layer},
    render::PreRenderContext,
};
use glam::{Mat4, Vec2, Vec3};
use shin_core::{
    primitives::{color::UnormColor, update::FrameId},
    time::Ticks,
    vm::command::types::LayerProperty,
};
use shin_render::gpu_texture::GpuTexture;
use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
use std::sync::Arc;

#[test]
fn clone_owns_render_expiry_and_missing_texture_has_explicit_error() {
    let atlas = EffectsAtlas::default();
    atlas.publish(Arc::new(EffectTextures::default()));
    let layer = SakuraLayerImpl {
        textures: atlas.borrower(),
        particles: Mutex::new(SakuraParticles::new(0, 1).unwrap()),
    };
    let copy = layer.clone();
    assert_eq!(
        copy.textures.reference_identity(),
        layer.textures.reference_identity()
    );
    layer.particles.lock().particles[0].alive = false;
    assert!(copy.particles.lock().particles[0].alive);
    assert!(
        std::panic::catch_unwind(|| {
            EffectTextures::default().required("Sakura");
        })
        .is_err()
    );
}

impl SakuraLayer {
    pub(crate) fn assert_reference_render(
        &mut self,
        context: &mut PreRenderContext,
        server: &Arc<AssetServer>,
        mode: i32,
    ) {
        let model = self.inner_ref().particles.lock();
        assert_eq!(model.mode, mode as usize);
        assert_eq!(model.elapsed, 500.0);
        assert!(!model.particles.is_empty());
        assert!(model.particles.iter().all(|p| p.alive));
        drop(model);
        assert!(
            self.inner_ref().textures.read().get("Sakura").is_none(),
            "Actual supplied atlas lacks Sakura"
        );
        for invalid in [-1, 6, i32::MIN, i32::MAX] {
            assert!(shin_tasks::block_on(Self::load(server, invalid)).is_none());
        }
        // Make all modes reproducible after checking their real loader state.
        *self.inner_mut().particles.get_mut() = SakuraParticles::new(mode, 1).unwrap();
        self.properties_mut()
            .property_tweener_mut(LayerProperty::Prop27)
            .fast_forward_to(0.0);
        let before = self.inner_ref().particles.lock().clone();
        let mut frame = FrameId::default();
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
        assert_eq!(
            *self.inner_ref().particles.lock(),
            before,
            "Initial framezero guards whole body"
        );
        frame.advance();
        update.frame_id = frame;
        self.update(&update);
        let advanced = self.inner_ref().particles.lock().clone();
        assert_eq!(advanced.elapsed, 500.0);
        assert_eq!(advanced.particles[0].age, before.particles[0].age + 50.0);
        assert_eq!(advanced.particles[0].origin, before.particles[0].origin);
        self.update(&update);
        assert_eq!(
            *self.inner_ref().particles.lock(),
            advanced,
            "Duplicate frame guards particles/RNG"
        );
        let mut copy = self.render_clone(&mut context.render_clone_ctx());
        copy.update(&update);
        assert_eq!(
            copy.inner_ref().particles.lock().particles[0].age,
            advanced.particles[0].age + 50.0,
            "Clone owns fresh frame guard"
        );
        assert_eq!(*self.inner_ref().particles.lock(), advanced);
        self.fast_forward();
        assert_eq!(
            *self.inner_ref().particles.lock(),
            advanced,
            "Fast-forward affects properties only"
        );
        frame.advance();
        update.frame_id = frame;
        update.delta_ticks = Ticks::from_f32(-2.0);
        update.are_animations_allowed = true;
        self.update(&update);
        assert_eq!(self.inner_ref().particles.lock().elapsed, 498.0);
        assert_eq!(
            self.inner_ref().particles.lock().particles[0].age,
            advanced.particles[0].age - 2.0
        );
        if mode != 0 {
            return;
        }

        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Synthetic Sakura geometry probe"),
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
        let draw = |layer: &Self,
                    context: &mut PreRenderContext,
                    kind: PassKind,
                    transform: TransformParams| {
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Synthetic Sakura geometry probe",
                );
                pass.clear(Some(UnormColor::BLACK), None, None);
                layer.inner_ref().render_drawable_direct(
                    &mut pass,
                    &transform,
                    &layer.properties().get_drawable_params(),
                    &layer.properties().get_clip_params(),
                    0,
                    kind,
                );
            }
            super::super::movie_layer::readback_test_texture(context, &target)
        };
        let visible = |pixels: &[u8]| {
            pixels
                .chunks_exact(4)
                .filter(|p| p[0].max(p[1]).max(p[2]) > 5)
                .count()
        };
        let p = self.inner_mut().particles.get_mut();
        p.particles.truncate(1);
        let particle = &mut p.particles[0];
        particle.alive = true;
        particle.variant = 0;
        particle.origin = Vec2::ZERO;
        particle.radius = 0.0;
        particle.travel_angle = 0.0;
        particle.spin_angle = 0.0;
        particle.scale = 1.0;
        particle.age = 0.0;
        let original = self.inner_ref().particles.lock().clone();
        assert_eq!(
            visible(&draw(
                self,
                context,
                PassKind::Opaque,
                TransformParams::default()
            )),
            0
        );
        assert_eq!(
            *self.inner_ref().particles.lock(),
            original,
            "Opaque pass neither expires nor looks up missing assets"
        );
        let missing = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            draw(
                self,
                context,
                PassKind::Transparent,
                TransformParams::default(),
            )
        }));
        assert!(
            missing.is_err(),
            "Missing Sakura must fail, never silently use hanabira"
        );
        assert_eq!(*self.inner_ref().particles.lock(), original);

        // Synthetic pixels prove geometry/UV plumbing only, not native art.
        let pixels: Vec<u8> = (0..288 * 408)
            .flat_map(|index| {
                if index / 288 < 204 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
            .collect();
        let synthetic_atlas = EffectsAtlas::default();
        let mut synthetic_textures = EffectTextures::default();
        synthetic_textures.add_texture(
            "Sakura",
            GpuTexture::new_static_with_data(
                context.device,
                context.queue,
                Some("Synthetic Sakura atlas"),
                winit::dpi::PhysicalSize::new(288, 408),
                wgpu::TextureFormat::Rgba8Unorm,
                &pixels,
            ),
        );
        synthetic_atlas.publish(Arc::new(synthetic_textures));
        let original_borrower = self.inner_ref().textures.clone();
        self.inner_mut().textures = synthetic_atlas.borrower();
        let initial = draw(
            self,
            context,
            PassKind::Transparent,
            TransformParams::default(),
        );
        assert_eq!(visible(&initial), 40 * 88);
        for (index, pixel) in initial.chunks_exact(4).enumerate() {
            if pixel[0] > 5 {
                let (x, y) = (index % 1920, index / 1920);
                assert!((939..979).contains(&x) && (494..582).contains(&y));
                assert_eq!(pixel[2], 0);
            }
        }
        let copy = self.render_clone(&mut context.render_clone_ctx());
        assert_eq!(
            initial,
            draw(
                &copy,
                context,
                PassKind::Transparent,
                TransformParams::default()
            )
        );
        self.inner_mut().particles.get_mut().particles[0].variant = 1;
        let variant = draw(
            self,
            context,
            PassKind::Transparent,
            TransformParams::default(),
        );
        assert_eq!(visible(&variant), 39 * 90);
        assert!(variant.chunks_exact(4).filter(|p| p[2] > 5).count() > 3000);
        self.inner_mut().particles.get_mut().particles[0].variant = 0;
        self.inner_mut().particles.get_mut().particles[0].radius = 100.0;
        self.inner_mut().particles.get_mut().particles[0].travel_angle =
            std::f32::consts::FRAC_PI_2;
        let radial = draw(
            self,
            context,
            PassKind::Transparent,
            TransformParams::default(),
        );
        // Travel rotation also rotates sprite geometry; spin stayszero.
        assert_eq!(visible(&radial), 40 * 88);
        for (index, pixel) in radial.chunks_exact(4).enumerate() {
            if pixel[0] > 5 {
                let (x, y) = (index % 1920, index / 1920);
                assert!((918..1006).contains(&x) && (619..659).contains(&y));
            }
        }
        let transform = TransformParams {
            transform: Mat4::from_translation(Vec3::new(0.0, 1000.0, 0.0)),
            ..Default::default()
        };
        assert_eq!(
            visible(&draw(self, context, PassKind::Transparent, transform)),
            0
        );
        assert!(!self.inner_ref().particles.lock().particles[0].alive);
        assert!(copy.inner_ref().particles.lock().particles[0].alive);
        assert_eq!(
            initial,
            draw(
                &copy,
                context,
                PassKind::Transparent,
                TransformParams::default()
            ),
            "Clone is unaffected by source render expiry"
        );
        frame.advance();
        update.frame_id = frame;
        update.delta_ticks = Ticks::ZERO;
        self.update(&update);
        assert!(self.inner_ref().particles.lock().particles.is_empty());
        self.inner_mut().textures = original_borrower;
        println!(
            "Animation0..5 loader/update/clone/guard/fast-forward, native missing Sakura error and synthetic sprite UV/bounds/radial transform/render expiry pass"
        );
    }
}
