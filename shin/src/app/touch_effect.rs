//! Native TouchEffect1154b0/1157b0/115d40: four drifting `systex:drag` stars.
use crate::{
    adv::{
        characters::view::CharacterTextures,
        input_area::{InputArea, InputAreaTarget, PointerFrame, PointerSampler},
    },
    engine_random::EngineRandom,
};
use glam::{Mat4, Vec2, Vec3, Vec4};
use shin_core::{primitives::color::FloatColor4, time::Ticks};
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind,
    RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosTexVertex},
};

struct Star {
    variant: u32,
    position: Vec2,
    velocity: Vec2,
    angle: f32,
    spin: f32,
    scale: f32,
    age: f32,
}

struct TapRipple {
    position: Vec2,
    age: f32,
}

pub(super) struct TouchEffect {
    random: EngineRandom,
    stars: Vec<Star>,
    ripples: Vec<TapRipple>,
    input: Option<InputArea>,
    sampler: PointerSampler,
    extent: (u16, u16),
    last_emission: u32,
    clock: u32,
}

impl Default for TouchEffect {
    fn default() -> Self {
        // Native owns a separately seeded MT stream; visual input must never
        // consume script or layer-constructor randomness.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        Self::with_seed(seed)
    }
}

impl TouchEffect {
    pub(super) fn with_seed(seed: u32) -> Self {
        Self {
            random: EngineRandom::from_seed(seed),
            stars: Vec::new(),
            ripples: Vec::new(),
            input: None,
            sampler: PointerSampler::default(),
            extent: (1280, 720),
            last_emission: 0,
            clock: 0,
        }
    }

    pub fn cancel_contact(&mut self) {
        self.input = None;
        self.sampler = PointerSampler::default();
    }

    #[cfg(test)]
    fn update(&mut self, delta: Ticks, enabled: bool, held: bool, pointer: Vec2) {
        self.update_in_viewport(delta, enabled, held, pointer, (1280, 720));
    }
    pub fn update_in_viewport(
        &mut self,
        delta: Ticks,
        enabled: bool,
        held: bool,
        pointer: Vec2,
        extent: (u16, u16),
    ) {
        self.advance_particles(delta);
        let ticks = delta.as_f32();
        if !enabled
            || !pointer.is_finite()
            || !(0.0..1920.0).contains(&pointer.x)
            || !(0.0..1080.0).contains(&pointer.y)
        {
            self.cancel_contact();
            return;
        }
        if extent != self.extent {
            self.cancel_contact();
            self.extent = extent;
        }
        let mut input = self
            .input
            .take()
            .unwrap_or_else(|| InputArea::new(extent.0.max(1), extent.1.max(1)));
        let mut sampler = std::mem::take(&mut self.sampler);
        sampler.sample(
            pointer * Vec2::new(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0),
            held,
            ticks as f64 / 60.0,
        );
        input.update(self, &sampler.frame());
        self.input = Some(input);
        self.sampler = sampler;
    }
    /// Use the application's admitted contact stream. A held mouse must not
    /// become a new visual Down after a modal, focus change or letterbox entry.
    pub(super) fn update_with_pointer_frame(
        &mut self,
        delta: Ticks,
        enabled: bool,
        frame: &PointerFrame<'_>,
        extent: (u16, u16),
    ) {
        self.advance_particles(delta);
        if !enabled {
            self.cancel_contact();
            return;
        }
        if extent != self.extent {
            self.cancel_contact();
            self.extent = extent;
        }
        let mut input = self
            .input
            .take()
            .unwrap_or_else(|| InputArea::new(extent.0.max(1), extent.1.max(1)));
        input.update(self, frame);
        self.input = Some(input);
    }
    fn advance_particles(&mut self, delta: Ticks) {
        let ticks = delta.as_f32();
        self.clock = self.clock.wrapping_add(1);
        for ripple in &mut self.ripples {
            ripple.age += ticks * (1.0 / 30.0);
        }
        self.ripples.retain(|ripple| ripple.age <= 1.0);
        let damping = 0.8f32.powf(ticks);
        for star in &mut self.stars {
            star.age += ticks * (1.0 / 60.0);
            // Native1155e8 advances position and spin once per update, while
            // damping and lifetime use the supplied tick delta.
            star.position += star.velocity;
            star.velocity *= damping;
            star.angle += star.spin;
        }
        self.stars.retain(|star| star.age <= 1.0);
    }
    fn emit_drag(&mut self, previous: Vec2, pointer: Vec2) {
        //115d40 is called only by InputArea's drag callback. A held tap must
        // never emit stars, even after the ten-frame emission interval.
        if pointer.distance(previous) < 10.0 && self.clock.wrapping_sub(self.last_emission) < 10 {
            return;
        }
        for _ in 0..4 {
            let variant = self.random.next() & 3;
            let velocity = Vec2::new(self.signed_random(), self.signed_random()) * 16.0;
            let angle = self.signed_random();
            let spin = self.signed_random() * 0.05;
            let scale = (1.0 + self.unit_random() * 0.5) * 0.5;
            self.stars.push(Star {
                variant,
                // Native115d74/11615c keeps the previous callback coordinate.
                // The current sample is used only for the emission threshold.
                position: previous,
                velocity,
                angle,
                spin,
                scale,
                age: 0.0,
            });
        }
        self.last_emission = self.clock;
    }

    pub fn has_ripples(&self) -> bool {
        self.ripples
            .iter()
            .any(|ripple| ripple.age > 0.0 && ripple.age < 1.0)
    }
    pub fn has_stars(&self) -> bool {
        !self.stars.is_empty()
    }
    pub fn render_ripples(&self, pass: &mut RenderPass, background: TextureSource) {
        let builder =
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::LayerPremultiplied1);
        let canvas = Vec2::new(1920.0, 1080.0);
        for ripple in &self.ripples {
            let radius = 80.0 * (1.0 - (1.0 - ripple.age).powf(3.0));
            if radius <= 0.0 {
                continue;
            }
            let corners = [
                Vec2::new(-1.0, -1.0),
                Vec2::new(1.0, -1.0),
                Vec2::new(-1.0, 1.0),
                Vec2::new(1.0, 1.0),
            ];
            let vertices = corners.map(|corner| {
                let position = ripple.position + corner * radius;
                PosTexVertex {
                    position,
                    texture_position: position / canvas,
                }
            });
            pass.run(builder.build(
                RenderProgramWithArguments::TapRipple {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: background,
                    transform: crate::render::top_left_projection_matrix(),
                    center_scale: Vec4::new(
                        ripple.position.x / canvas.x,
                        ripple.position.y / canvas.y,
                        canvas.x / radius,
                        canvas.y / radius,
                    ),
                    wave_params: Vec4::new(ripple.age, 0.0, 0.004, (1920.0 / 1080.0) * 0.004),
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
    fn unit_random(&mut self) -> f32 {
        self.random.next() as f32 * (1.0 / 4_294_967_296.0)
    }
    fn signed_random(&mut self) -> f32 {
        self.unit_random() * 2.0 - 1.0
    }

    pub fn render(&self, pass: &mut RenderPass, textures: &CharacterTextures) {
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer2);
        // Global input feedback is drawn over every scene. Keep the default
        // Always/Keep stencil state: shorthand(0, false, false) tests 0 > the
        // stored stencil, rejecting every star on a real window attachment.
        for star in &self.stars {
            let fade = (1.0 - star.age).max(0.0).powf(1.6);
            let left = (star.variant & 1) as f32 * 0.5;
            let top = ((star.variant >> 1) & 1) as f32 * 0.5;
            let transform = crate::render::top_left_projection_matrix()
                * Mat4::from_translation(star.position.extend(0.0))
                * Mat4::from_rotation_z(star.angle * f32::from_bits(0x40490fd0))
                * Mat4::from_scale(Vec3::new(fade * star.scale, fade * star.scale, 1.0));
            let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosTexVertex {
                position: Vec2::splat(-40.0) + corner * 80.0,
                texture_position: Vec2::new(left, top) + corner * 0.5,
            });
            // Native program8 supplies float alpha, not an eight-bit vertex
            // color. The default textured-layer program preserves that fade.
            pass.run(builder.build(
                RenderProgramWithArguments::Layer {
                    output_kind: LayerShaderOutputKind::Layer,
                    fragment_shader: LayerFragmentShader::Default,
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: textures.get("drag").as_source(),
                    transform,
                    color_multiplier: FloatColor4::from_rgba(1.0, 1.0, 1.0, fade),
                    fragment_shader_param: Vec4::ZERO,
                },
                DrawPrimitive::TrianglesStrip,
            ));
        }
    }
}

impl InputAreaTarget for TouchEffect {
    fn probe(&mut self, _: Vec2) -> bool {
        true
    }
    fn reset(&mut self) {}
    fn press(&mut self, _: Vec2) {}
    fn release(&mut self, position: Vec2) {
        self.ripples.push(TapRipple { position, age: 0.0 });
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        self.emit_drag(previous, current);
    }
    fn gesture(&mut self, _: Vec2) {}
    fn cancel(&mut self) {}
}

#[cfg(test)]
mod tests;
