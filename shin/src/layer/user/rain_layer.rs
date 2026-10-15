#[cfg(test)]
use std::sync::Arc;
use super::rain_particles::RainParticles;
#[cfg(test)]
use crate::asset::system::AssetServer;
use crate::{
    asset::effects::EffectsAtlasBorrow,
    layer::{
        LayerProperties, NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
    },
    update::{AdvUpdatable, AdvUpdateContext},
};
use glam::{Mat4, Vec3};
use shin_core::vm::command::types::LayerProperty;
use shin_render::{
    ColorBlendType, PassKind, RenderRequestBuilder, quad_vertices::QuadVertices,
    render_pass::RenderPass, shaders::types::RenderClone,
};

#[derive(Clone, RenderClone)]
pub struct RainLayerImpl {
    textures: EffectsAtlasBorrow,
    particles: RainModel,
}
#[derive(Clone, Debug)]
enum RainModel {
    Legacy(RainParticles),
    Ger(crate::adv::gerokasu2::procedural_particles::GerRainParticles),
}
impl RainModel {
    fn mode(&self) -> usize {
        match self {
            Self::Legacy(m) => m.mode,
            Self::Ger(m) => m.mode,
        }
    }
    fn particles(&self) -> &[super::RainParticle] {
        match self {
            Self::Legacy(m) => &m.particles,
            Self::Ger(m) => &m.particles,
        }
    }
    fn warm_up(&mut self, intensity: f32, direction: f32, speed: f32) {
        match self {
            Self::Legacy(m) => m.warm_up(intensity, direction, speed),
            Self::Ger(m) => m.warm_up(intensity, direction, speed),
        }
    }
    fn update(&mut self, delta: f32, intensity: f32, direction: f32, speed: f32) {
        match self {
            Self::Legacy(m) => m.update(delta, intensity, direction, speed),
            Self::Ger(m) => m.update(delta, intensity, direction, speed),
        }
    }
    #[cfg(test)]
    fn reference_seed(&self) -> u32 {
        match self {
            Self::Legacy(m) => m.reference_seed(),
            Self::Ger(m) => m.reference_seed(),
        }
    }
    #[cfg(test)]
    fn legacy(&self) -> &RainParticles {
        let Self::Legacy(m) = self else {
            panic!("Legacy rain fixture")
        };
        m
    }
    #[cfg(test)]
    fn legacy_mut(&mut self) -> &mut RainParticles {
        let Self::Legacy(m) = self else {
            panic!("Legacy rain fixture")
        };
        m
    }
}
impl std::fmt::Debug for RainLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RainLayer")
            .field("particles", &self.particles)
            .finish()
    }
}
pub type RainLayer = NewDrawableLayerWrapper<RainLayerImpl>;
impl RainLayer {
    #[cfg(test)]
    pub(crate) fn reference_constructor_state(&self) -> (usize, u32, u32) {
        (
            self.inner_ref().textures.reference_identity(),
            self.reference_dissolve_seed(),
            self.inner_ref().particles.reference_seed(),
        )
    }
    pub(crate) fn texture_name(mode: i32) -> &'static str {
        match mode.clamp(0, 3) {
            0 => "rain",
            1 => "hanabira",
            2 => "snow",
            _ => "goldsnow",
        }
    }
    #[cfg(test)]
    pub async fn load(server: &AssetServer, mode: i32, min: i32, max: i32) -> Self {
        let textures = server
            .effects_atlas_for(Self::texture_name(mode))
            .await
            .expect("Failed to load rain textures");
        Self::new(textures, mode, min, max)
    }

    pub(crate) fn new(textures: EffectsAtlasBorrow, mode: i32, min: i32, max: i32) -> Self {
        Self::from_inner_constructor(|| RainLayerImpl {
            textures,
            particles: RainModel::Legacy(RainParticles::new(
                mode,
                min,
                max,
                crate::engine_random::next_seed(),
            )),
        })
    }
    pub(crate) fn new_gerokasu2(
        textures: EffectsAtlasBorrow,
        mode: i32,
        min: i32,
        max: i32,
    ) -> Self {
        Self::from_inner_constructor(|| RainLayerImpl {
            textures,
            particles: RainModel::Ger(
                crate::adv::gerokasu2::procedural_particles::GerRainParticles::new(
                    mode,
                    min,
                    max,
                    crate::engine_random::next_seed(),
                ),
            ),
        })
    }
}

#[cfg(test)]
impl RainLayerImpl {
    pub(super) fn reference_owner_prepare(&mut self, count: usize, identity: usize) {
        self.particles
            .legacy_mut()
            .reference_owner_prepare(count, identity);
    }
    pub(super) fn reference_owner_words(&self) -> (Option<usize>, Vec<String>) {
        self.particles.legacy().reference_owner_words()
    }
    pub(super) fn reference_owner_mutate(&mut self) {
        self.particles.legacy_mut().reference_owner_mutate();
    }
    pub(super) fn reference_owner_advance(&mut self, intensity: f32, direction: f32, speed: f32) {
        self.particles.update(0.0, intensity, direction, speed);
    }
}
impl AdvUpdatable for RainLayerImpl {
    fn update(&mut self, _: &AdvUpdateContext) {}
}
impl NewDrawableLayerFastForward for RainLayerImpl {
    fn fast_forward(&mut self) {}
}
impl NewDrawableLayerNeedsSeparatePass for RainLayerImpl {}
impl NewDrawableLayer for RainLayerImpl {
    fn update_with_properties(&mut self, context: &AdvUpdateContext, props: &LayerProperties) {
        let intensity = props.get_value(LayerProperty::RainIntensity);
        let direction = props.get_value(LayerProperty::RainDirection);
        let speed = props.get_value(LayerProperty::RainSpeed);
        self.particles.warm_up(intensity, direction, speed);
        if props.get_value(LayerProperty::Prop27) as i32 != 0 {
            self.particles
                .update(context.delta_ticks.as_f32(), intensity, direction, speed);
        }
    }
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        drawable: &DrawableParams,
        clip: &DrawableClipParams,
        stencil_ref: u8,
        kind: PassKind,
    ) {
        if kind != PassKind::Transparent || drawable.color_multiplier.a <= 0.0 {
            return;
        }
        assert_eq!(
            clip.mode,
            DrawableClipMode::None,
            "Rain clipping is not implemented"
        );
        let textures = self.textures.read();
        let (texture, width, height) = match self.particles.mode() {
            0 => (textures.required("rain"), 4.0, 150.0),
            1 => (textures.required("hanabira"), 30.0, 30.0),
            2 => (textures.required("snow"), 15.0, 15.0),
            _ => (textures.required("goldsnow"), 15.0, 15.0),
        };
        let color = drawable
            .fragment_shader
            .simplify(drawable.shader_param)
            .evaluate(drawable.color_multiplier, drawable.shader_param)
            .into_unorm();
        let quad = QuadVertices::new()
            .with_box(-width, -height, width, height)
            .with_color(color);
        let builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(stencil_ref, false, false)
            .color_blend_type(ColorBlendType::from_regular_layer(drawable.blend_type));
        let matrix = transform.compute_final_transform();
        let mut particles: Vec<_> = self.particles.particles().iter().collect();
        particles.sort_by(|a, b| b.depth.total_cmp(&a.depth));
        for particle in particles {
            let mut positions = vec![particle.position.x];
            if particle.position.x - particle.radius < 0.0 {
                positions.push(particle.position.x + 1920.0);
            }
            if particle.position.x + particle.radius > 1920.0 {
                positions.push(particle.position.x - 1920.0);
            }
            for x in positions {
                let translation =
                    Mat4::from_translation(Vec3::new(x - 960.0, particle.position.y - 540.0, 0.0));
                let matrix = if matches!(self.particles, RainModel::Ger(_)) {
                    crate::layer::render_params::multiply_transforms(
                        crate::layer::render_params::multiply_transforms(matrix, translation),
                        particle.shape,
                    )
                } else {
                    matrix * translation * particle.shape
                };
                quad.render_sprite(pass, builder, texture.as_source(), matrix);
            }
        }
    }
}

#[cfg(test)]
impl RainLayer {
    pub(crate) fn assert_reference_render(
        &mut self,
        context: &mut crate::render::PreRenderContext,
        server: &Arc<AssetServer>,
    ) {
        use crate::layer::DrawableLayer;
        use shin_core::{
            primitives::{color::UnormColor, update::FrameId},
            time::Ticks,
        };
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let mode = self.inner_ref().particles.mode();
        *self = Self::from_inner(RainLayerImpl {
            textures: self.inner_ref().textures.clone(),
            particles: RainModel::Legacy(RainParticles::new(mode as i32, 100, 1500, 1)),
        });
        self.properties_mut()
            .property_tweener_mut(LayerProperty::RainIntensity)
            .fast_forward_to(3000.0);
        let mut frame = FrameId::default();
        frame.advance();
        let update = AdvUpdateContext {
            frame_id: frame,
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        };
        self.update(&update);
        assert!(!self.inner_ref().particles.particles().is_empty());
        assert!(!self.inner_ref().particles.legacy().needs_warm_up);
        let previous: Vec<_> = self
            .inner_ref()
            .particles
            .particles()
            .iter()
            .map(|p| p.position)
            .collect();
        self.properties_mut()
            .property_tweener_mut(LayerProperty::Prop27)
            .fast_forward_to(0.0);
        self.update(&update);
        assert_eq!(
            previous,
            self.inner_ref()
                .particles
                .particles()
                .iter()
                .map(|p| p.position)
                .collect::<Vec<_>>()
        );
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Reference rain"),
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
        for alpha in [1000.0, 0.0] {
            self.properties_mut()
                .property_tweener_mut(LayerProperty::MulColorAlpha)
                .fast_forward_to(alpha);
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Reference rain",
                );
                pass.clear(Some(UnormColor::BLACK), None, None);
                self.inner_ref().render_drawable_direct(
                    &mut pass,
                    &TransformParams::default(),
                    &self.properties().get_drawable_params(),
                    &self.properties().get_clip_params(),
                    0,
                    PassKind::Transparent,
                );
            }
            let pixels = super::movie_layer::readback_test_texture(context, &target);
            let visible = pixels
                .chunks_exact(4)
                .filter(|p| p[0].max(p[1]).max(p[2]) > 5)
                .count();
            if alpha > 0.0 {
                assert!(
                    visible > 100,
                    "Particle mode{mode} must render visible texture, got{visible}"
                );
                let mut ppm = format!("P6\n1920 1080\n255\n").into_bytes();
                for pixel in pixels.chunks_exact(4) {
                    ppm.extend_from_slice(&pixel[..3]);
                }
                std::fs::write(format!("/tmp/shin-rain-mode-{mode}.ppm"), ppm).unwrap();
            } else {
                assert_eq!(visible, 0, "Opacity must hide rain");
            }
        }
    }
}
