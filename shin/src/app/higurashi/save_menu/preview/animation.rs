//! Updated saved-preview4/8/9: real warmed constructors, refresh254bc and
//! Animation virtual58 (Sakura no-op; Rain warm-up already complete).
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec2, Vec4, vec2, vec3, vec4};
use parking_lot::Mutex;
use shin_core::{format::scenario::higurashi::HigurashiScenario, primitives::color::FloatColor4};
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

pub(super) struct Prepared {
    kind: i32,
    params: [i32; 8],
    atlas: Option<Arc<CharacterTextures>>,
}
impl Prepared {
    pub async fn load(
        server: &AssetServer,
        _scenario: &HigurashiScenario,
        kind: i32,
        params: &[i32; 8],
    ) -> Result<Self> {
        ensure!(
            matches!(kind, 4 | 8 | 9),
            "Saved animation factory is outside4/8/9"
        );
        let mut prepared = Self {
            kind,
            params: *params,
            atlas: None,
        };
        // Native25180 returns NULL before base construction,globalMT or atlas
        // lookup. All present owners borrow the same native effects singleton.
        if prepared.clone_present() {
            prepared.atlas = Some(server.load("/effects.txa").await?);
        }
        Ok(prepared)
    }
    pub fn clone_present(&self) -> bool {
        self.kind != 4 || (0..6).contains(&self.params[0])
    }
    /// Root supplies four fresh OS property seeds BEFORE this call. The derived
    /// constructors draw exactly once from the separate process-global MT.
    pub fn clone_refresh(&self) -> Result<Option<View>> {
        if !self.clone_present() {
            return Ok(None);
        }
        Ok(Some(View {
            state: State::new(self.kind, &self.params, crate::engine_random::next_seed())?,
            atlas: self.atlas.clone(),
        }))
    }
}

enum State {
    Sakura(Mutex<SakuraParticles>),
    Focus { rays: FocusLineRays, offset: Vec2 },
    Rain(Mutex<RainParticles>),
}
impl State {
    fn new(kind: i32, params: &[i32; 8], seed: u32) -> Result<Self> {
        Ok(match kind {
            4 => Self::Sakura(Mutex::new(
                SakuraParticles::new(params[0], seed).context("Native Animation clone is NULL")?,
            )),
            8 => Self::Focus {
                rays: FocusLineRays::new(seed),
                offset: vec2(params[0] as f32, params[1] as f32),
            },
            9 => {
                let mut rain = RainParticles::new_higurashi(params[0], params[1], params[2], seed);
                // a1490 invokes a1804 while original51/52/53 are still default.
                // d404c calls it again after restored targets;expired>0 makes
                // that second warm-up a no-op. No preview frame advances it.
                rain.warm_up(1000., 0., 1000.);
                Self::Rain(Mutex::new(rain))
            }
            _ => anyhow::bail!("Saved animation state is outside4/8/9"),
        })
    }
}
pub(super) struct View {
    state: State,
    atlas: Option<Arc<CharacterTextures>>,
}

#[derive(Clone, Debug)]
pub(super) struct Packet {
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
impl View {
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
        if !matches!(self.state, State::Rain(_)) && !properties.is_visible() {
            return Ok(Vec::new());
        }
        let p = properties.values();
        ensure!(
            matches!(self.state, State::Rain(_))
                || (p[44].min(p[46]) <= 0.
                    && p[45].min(p[47]) <= 0.
                    && p[44].max(p[46]) >= 1280.
                    && p[45].max(p[47]) >= 720.),
            "Saved animation clipped draw needs the original stencil preparation"
        );
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
            shader: if matches!(self.state, State::Rain(_)) {
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
            State::Rain(rain) => {
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
        if kind == PassKind::Transparent {
            if let State::Rain(rain) = &self.state {
                // a22e8 binds before checking whether the warmed vector is empty.
                let texture = ["rain", "hanabira", "snow", "goldsnow"][rain.lock().mode];
                let atlas = self
                    .atlas
                    .as_ref()
                    .context("Saved animation lost its effects atlas")?;
                ensure!(
                    atlas.contains(texture),
                    "Saved animation requires '{}' in /effects.txa",
                    texture
                );
            }
        }
        for packet in self.packets(properties, parent, kind)? {
            let atlas = self
                .atlas
                .as_ref()
                .context("Saved animation lost its effects atlas")?;
            // Native lookup of absent 'Sakura' dereferences an invalid iterator.
            // Preserve that resource gap as a checked desktop error at draw.
            ensure!(
                atlas.contains(packet.texture),
                "Saved animation requires '{}' in /effects.txa",
                packet.texture
            );
            let vertices = packet.vertices();
            pass.run(
                RenderRequestBuilder::new()
                    // Native25e08/5b4c0/a2270 pass mask1 to697f0, selecting
                    // inclusive compare6 so overlapping particles share one owner.
                    .depth_stencil_shorthand(stencil, true, false)
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
