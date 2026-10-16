//! Updated live Animation4/Focus8/Rain9 owners. Factory replacement starts a
//! fresh private simulation; virtual38 copies retain independent warmed state.
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec2, Vec4, vec2, vec3, vec4};
use parking_lot::Mutex;
use shin_core::{primitives::color::FloatColor4, time::Ticks};
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, PassKind, RenderProgramWithArguments,
    RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosTexVertex},
};

use crate::{
    adv::characters::view::CharacterTextures,
    asset::system::AssetServer,
    layer::{
        higurashi_properties::HigurashiProperties,
        render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
        user::{
            focus_line_model::FocusLineRays, rain_particles::RainParticles,
            sakura_particles::SakuraParticles,
        },
    },
};

/// Asset preparation has no property or process-global random side effects.
#[derive(Debug)]
pub(crate) struct Prepared {
    kind: i32,
    params: [i32; 8],
    atlas: Option<Arc<CharacterTextures>>,
}
impl Prepared {
    pub async fn load(server: &AssetServer, kind: i32, params: &[i32; 8]) -> Result<Self> {
        ensure!(
            matches!(kind, 4 | 8 | 9),
            "Live animation factory is outside4/8/9"
        );
        let mut prepared = Self {
            kind,
            params: *params,
            atlas: None,
        };
        if prepared.present() {
            prepared.atlas = Some(server.load("/effects.txa").await?);
        }
        Ok(prepared)
    }
    pub fn present(&self) -> bool {
        self.kind != 4 || (0..6).contains(&self.params[0])
    }
    /// The caller constructs or deep-copies the base property owner first.
    /// Native6717c uses four OS seeds only without an old owner. A present
    /// derived constructor then consumes exactly one separate global MT seed.
    /// `old_elapsed` is supplied only for Animation RTTI (Sakura/Rain).
    pub fn construct(
        &self,
        properties: &mut HigurashiProperties,
        old_elapsed: Option<f32>,
        mt_seed: u32,
    ) -> Result<Option<Owner>> {
        if !self.present() {
            return Ok(None);
        }
        let state = match self.kind {
            4 => {
                let mut model = SakuraParticles::new(self.params[0], mt_seed)
                    .context("Native Animation factory is NULL")?;
                // The private simulation's constructor is independent of the
                // base properties. Reproduce its separately inherited elapsed
                // clock and all500 inherited-base updates in native order.
                model.elapsed = old_elapsed.unwrap_or(0.);
                for _ in 0..500 {
                    properties.advance(Ticks::from_f32(1.));
                    model.elapsed += 1.;
                }
                State::Sakura(Mutex::new(model))
            }
            8 => State::Focus {
                rays: FocusLineRays::new(mt_seed),
                offset: vec2(self.params[0] as f32, self.params[1] as f32),
            },
            9 => {
                let mut rain = RainParticles::new_higurashi(
                    self.params[0],
                    self.params[1],
                    self.params[2],
                    mt_seed,
                );
                warm_rain(&mut rain, properties)?;
                State::Rain {
                    particles: Mutex::new(rain),
                    elapsed: old_elapsed.unwrap_or(0.),
                }
            }
            _ => unreachable!(),
        };
        Ok(Some(Owner {
            state,
            atlas: self.atlas.clone(),
        }))
    }
}

#[derive(Debug)]
enum State {
    Sakura(Mutex<SakuraParticles>),
    Focus {
        rays: FocusLineRays,
        offset: Vec2,
    },
    Rain {
        particles: Mutex<RainParticles>,
        elapsed: f32,
    },
}
impl Clone for State {
    fn clone(&self) -> Self {
        match self {
            Self::Sakura(p) => Self::Sakura(Mutex::new(p.lock().clone())),
            Self::Focus { rays, offset } => Self::Focus {
                rays: rays.clone(),
                offset: *offset,
            },
            Self::Rain { particles, elapsed } => Self::Rain {
                particles: Mutex::new(particles.lock().clone()),
                elapsed: *elapsed,
            },
        }
    }
}
fn warm_rain(rain: &mut RainParticles, properties: &HigurashiProperties) -> Result<()> {
    if rain.expired != 0 {
        return Ok(());
    }
    let p = properties.values();
    // Actual a1804 waits without a bound for the first expiry. Zero intensity
    // can still finish: initial emission1 may create its first moving particle.
    // No later-version permission/needs_warm_up shortcut is applied here.
    while rain.expired == 0 {
        rain.update(1., p[51], p[52], p[53]);
    }
    rain.needs_warm_up = false;
    Ok(())
}

/// Clone follows actual26160/5b0fc/a1b14: no fresh entropy or warm-up, deep
/// private vectors/tween queues, and shared immutable atlas resources only.
#[derive(Debug, Clone)]
pub(crate) struct Owner {
    state: State,
    atlas: Option<Arc<CharacterTextures>>,
}
impl Owner {
    #[cfg(test)]
    pub(crate) fn reference_clear_sakura(&mut self) {
        match &mut self.state {
            State::Sakura(particles) => particles.get_mut().particles.clear(),
            _ => panic!("Controlled empty Sakura fixture requires native type4"),
        }
    }
    pub fn native_id(&self) -> i32 {
        match self.state {
            State::Sakura(_) => 4,
            State::Focus { .. } => 8,
            State::Rain { .. } => 9,
        }
    }
    pub fn elapsed(&self) -> Option<f32> {
        match &self.state {
            State::Sakura(p) => Some(p.lock().elapsed),
            State::Rain { elapsed, .. } => Some(*elapsed),
            State::Focus { .. } => None,
        }
    }
    /// Original integrated D payload: scalar words, vector count, then native
    /// particle records; Focus ray current alpha/busy replace native pointers.
    #[cfg(test)]
    pub(crate) fn reference_native_words(&self) -> Vec<u32> {
        match &self.state {
            State::Sakura(model) => {
                let model = model.lock();
                let mut words = vec![
                    model.elapsed.to_bits(),
                    model.reference_seed(),
                    model.mode as u32,
                    model.reference_owner_emission().to_bits(),
                    model.particles.len() as u32,
                ];
                for p in &model.particles {
                    words.extend([
                        u32::from(p.alive),
                        p.variant as u32,
                        p.origin.x.to_bits(),
                        p.origin.y.to_bits(),
                        p.travel_speed.to_bits(),
                        p.travel_angle.to_bits(),
                        p.radius.to_bits(),
                        p.scale.to_bits(),
                        p.spin_angle.to_bits(),
                        p.spin_speed.to_bits(),
                        p.age.to_bits(),
                    ]);
                }
                words
            }
            State::Focus { rays, offset } => {
                let mut words = vec![
                    rays.reference_seed(),
                    offset.x.to_bits(),
                    offset.y.to_bits(),
                    rays.rays.len() as u32,
                ];
                for ray in &rays.rays {
                    words.extend([
                        ray.angle.to_bits(),
                        ray.alpha.value().to_bits(),
                        u32::from(!ray.alpha.is_idle()),
                    ]);
                }
                words
            }
            State::Rain { particles, elapsed } => {
                let model = particles.lock();
                let (_, owner) = model.reference_owner_words();
                let mut words = vec![
                    elapsed.to_bits(),
                    model.reference_seed(),
                    model.mode as u32,
                    model.min_distance as u32,
                    model.max_distance as u32,
                    u32::from_str_radix(&owner[4], 16).unwrap(),
                    model.expired,
                    u32::from_str_radix(&owner[7], 16).unwrap(),
                    model.particles.len() as u32,
                ];
                for p in &model.particles {
                    words.extend(
                        p.shape
                            .to_cols_array()
                            .into_iter()
                            .chain([
                                p.position.x,
                                p.position.y,
                                p.depth,
                                p.velocity.x,
                                p.velocity.y,
                                p.radius,
                            ])
                            .map(f32::to_bits),
                    );
                }
                words
            }
        }
    }
    /// Original virtual18 accepts only delta. Suspension belongs to the caller
    /// deciding whether to invoke it; these bodies have no extra permission gate.
    pub fn advance(&mut self, delta: Ticks, properties: &mut HigurashiProperties) {
        properties.advance(delta);
        match &mut self.state {
            State::Sakura(p) => p.get_mut().update(delta.as_f32(), true),
            State::Focus { rays, .. } => rays.update(delta),
            State::Rain { particles, elapsed } => {
                *elapsed += delta.as_f32();
                let p = properties.values();
                particles
                    .get_mut()
                    .update(delta.as_f32(), p[51], p[52], p[53]);
            }
        }
    }
    /// Native Animation virtual58: SakuraRET; Rain awaits its first expiry.
    /// The normal Rain constructor has already completed that warm-up.
    pub fn warm_up(&mut self, properties: &HigurashiProperties) -> Result<()> {
        if let State::Rain { particles, .. } = &mut self.state {
            warm_rain(particles.get_mut(), properties)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Packet {
    pub texture: &'static str,
    pub matrix: Mat4,
    pub rect: Vec4,
    pub uv: Vec4,
    pub color: FloatColor4,
    pub shader: LayerFragmentShader,
    pub shader_param: Vec4,
    pub blend: ColorBlendType,
}
impl Packet {
    fn vertices(&self) -> [PosTexVertex; 4] {
        [
            (self.rect.x, self.rect.y, self.uv.x, self.uv.y),
            (self.rect.z, self.rect.y, self.uv.z, self.uv.y),
            (self.rect.x, self.rect.w, self.uv.x, self.uv.w),
            (self.rect.z, self.rect.w, self.uv.z, self.uv.w),
        ]
        .map(|(x, y, u, v)| PosTexVertex {
            position: vec2(x, y),
            texture_position: vec2(u, v),
        })
    }
}
impl Owner {
    pub fn packets(
        &self,
        properties: &HigurashiProperties,
        parent: &HigurashiTransform,
        kind: PassKind,
    ) -> Result<Vec<Packet>> {
        if kind == PassKind::Opaque {
            return Ok(Vec::new());
        }
        // a21a8 Rain omits the external visibility test used by25d50/5b400.
        if !matches!(self.state, State::Rain { .. }) && !properties.is_visible() {
            return Ok(Vec::new());
        }
        let drawable = properties.drawable_params();
        let native = properties
            .composed_transform(parent)
            .compute_native_final_transform();
        let base = |texture, matrix, rect, uv, color| Packet {
            texture,
            matrix,
            rect,
            uv,
            color,
            // Rain a2314 indexes the original operation directly. Sakura
            //25d98 and Focus5b450 instead call69894 to simplify identities.
            shader: if matches!(self.state, State::Rain { .. }) {
                drawable.fragment_shader
            } else {
                drawable.fragment_shader.simplify(drawable.shader_param)
            },
            shader_param: drawable.shader_param,
            blend: ColorBlendType::from_higurashi_layer(drawable.blend_type),
        };
        let mut packets = Vec::new();
        match &self.state {
            State::Sakura(particles) => {
                // Native rendering owns expiry,so this mutex is scene-local.
                particles.lock().render_particles(native, |matrix, sprite| {
                    packets.push(base(
                        "Sakura",
                        matrix,
                        Vec4::from_array(sprite.bounds.map(|v| v as f32)),
                        Vec4::from_array(sprite.uv),
                        drawable.color_multiplier,
                    ));
                });
            }
            State::Focus { rays, offset } => {
                let matrix = multiply(
                    native,
                    Mat4::from_translation(vec3(offset.x, offset.y - 360., 0.)),
                );
                for ray in &rays.rays {
                    // Native draws all40 rays,including completed alpha0 ones.
                    packets.push(base(
                        "focusline",
                        multiply(matrix, Mat4::from_rotation_z(ray.angle)),
                        vec4(150., -15., 1430., 15.),
                        vec4(1., 0., 0., 1.),
                        drawable
                            .color_multiplier
                            .with_alpha(drawable.color_multiplier.a * ray.alpha.value()),
                    ));
                }
            }
            State::Rain {
                particles: rain, ..
            } => {
                let mut rain = rain.lock();
                let matrix = multiply(
                    multiply(native, Mat4::from_translation(vec3(-640., -720., 0.))),
                    Mat4::from_scale(vec3(2. / 3., 2. / 3., 0.)),
                );
                let (texture, width, height) = match rain.mode {
                    0 => ("rain", 4., 150.),
                    1 => ("hanabira", 30., 30.),
                    2 => ("snow", 15., 15.),
                    _ => ("goldsnow", 15., 15.),
                };
                rain.particles.sort_by(|a, b| b.depth.total_cmp(&a.depth));
                for particle in &rain.particles {
                    let mut xs = vec![particle.position.x];
                    if particle.position.x - particle.radius < 0. {
                        xs.push(particle.position.x + 1920.);
                    }
                    if particle.position.x + particle.radius > 1920. {
                        xs.push(particle.position.x - 1920.);
                    }
                    for x in xs {
                        let local = multiply(
                            Mat4::from_translation(vec3(x, particle.position.y, 0.)),
                            particle.shape,
                        );
                        packets.push(base(
                            texture,
                            multiply(matrix, local),
                            vec4(-width, -height, width, height),
                            vec4(0., 0., 1., 1.),
                            drawable.color_multiplier,
                        ));
                    }
                }
            }
        }
        Ok(packets)
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        properties: &HigurashiProperties,
        parent: &HigurashiTransform,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        // Original Sakura/Focus prepare the logical mask after the pass and
        // visibility gates, even when their particle vector is empty. Rain's
        // own path does not consult44..47 or this helper.
        let clipped = kind == PassKind::Transparent
            && !matches!(self.state, State::Rain { .. })
            && properties.is_visible()
            && super::leaf_clip::Clip::from_properties(properties).prepare(pass);
        if kind == PassKind::Transparent {
            if let State::Rain {
                particles: rain, ..
            } = &self.state
            {
                // a22e8 binds before checking whether the warmed vector is empty.
                let texture = ["rain", "hanabira", "snow", "goldsnow"][rain.lock().mode];
                let atlas = self
                    .atlas
                    .as_ref()
                    .context("Live animation lost its effects atlas")?;
                ensure!(
                    atlas.contains(texture),
                    "Live animation requires '{}' in /effects.txa",
                    texture
                );
            }
        }
        for packet in self.packets(properties, parent, kind)? {
            let atlas = self
                .atlas
                .as_ref()
                .context("Live animation lost its effects atlas")?;
            // Native lookup of absent 'Sakura' dereferences an invalid iterator.
            // Preserve that resource gap as a checked desktop error at draw.
            ensure!(
                atlas.contains(packet.texture),
                "Live animation requires '{}' in /effects.txa",
                packet.texture
            );
            let vertices = packet.vertices();
            pass.run(
                RenderRequestBuilder::new()
                    // Native25e08/5b4c0/a2270 pass mask1 to697f0, selecting
                    // inclusive compare6 so overlapping particles share one owner.
                    .depth_stencil_shorthand(stencil, true, clipped)
                    .color_blend_type(packet.blend)
                    .build(
                        RenderProgramWithArguments::HigurashiPostprocess {
                            operation: packet.shader,
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
                            texture: atlas.get(packet.texture).as_source(),
                            transform: HigurashiTransform::convert_clip_depth(packet.matrix),
                            color: packet.color,
                            param: packet.shader_param,
                        },
                        DrawPrimitive::TrianglesStrip,
                    ),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
