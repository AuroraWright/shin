//! Older effect5/6/10 packet/resource policies shared by live and saved owners.
use anyhow::{Result, ensure};
use glam::{Mat4, Vec2, Vec4, vec3, vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, PassKind, RenderProgramWithArguments,
    RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosTexVertex},
};

use crate::layer::{
    higurashi_properties::HigurashiProperties,
    render_params::{higurashi::HigurashiTransform, multiply_transforms},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Prepared {
    kind: i32,
}
impl Prepared {
    pub fn new(kind: i32) -> Result<Self> {
        ensure!(
            matches!(kind, 5 | 6 | 10),
            "Saved effect factory is outside5/6/10"
        );
        Ok(Self { kind })
    }
    /// Original NULL-old clones have no asset tickets, derived MT or phase tick.
    /// Scene supplies the four base OS samples before restoring all65 properties.
    pub fn clone_refresh(&self) -> View {
        View {
            kind: self.kind,
            phases: [0.; 2],
            weights: [0x3e5d4adf, 0x3e434a38, 0x3e06387e, 0x3d8fafb1].map(f32::from_bits),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ResourcePlan {
    pub extent: (u32, u32),
    pub color_count: usize,
    pub depth_count: usize,
    pub shared_depth: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CapturePlan {
    pub resources: ResourcePlan,
    pub viewport: (u32, u32),
}

#[derive(Clone, Debug)]
pub(crate) struct View {
    kind: i32,
    phases: [f32; 2],
    weights: [f32; 4],
}
impl View {
    pub fn replacement(kind: i32, old: Option<&Self>) -> Result<Self> {
        let mut view = Prepared::new(kind)?.clone_refresh();
        // Live loader+28 always allocates fresh buffers and zero Raster phases.
        // Only Blur's successfully cast old owner supplies custom weights.
        if let Some(old) = old.filter(|old| kind == 5 && old.kind == 5) {
            view.weights = old.weights;
        }
        Ok(view)
    }
    pub fn advance(&mut self, delta: shin_core::time::Ticks, properties: &HigurashiProperties) {
        if self.kind == 10 {
            let values = properties.values();
            for (phase, period) in self.phases.iter_mut().zip([values[56], values[59]]) {
                if period != 0. {
                    *phase = wrap_phase(*phase, (1. / period) * delta.as_f32());
                }
            }
        }
    }
    pub fn kind(&self) -> i32 {
        self.kind
    }
    pub fn phases(&self) -> [f32; 2] {
        self.phases
    }
    pub fn resource_plan(&self) -> ResourcePlan {
        match self.kind {
            5 => ResourcePlan {
                extent: (960, 540),
                color_count: 2,
                depth_count: 2,
                shared_depth: false,
            },
            _ => ResourcePlan {
                extent: (1920, 1080),
                color_count: 1,
                depth_count: 0,
                shared_depth: true,
            },
        }
    }
    pub fn capture_plan(&self, properties: &HigurashiProperties) -> CapturePlan {
        let resources = self.resource_plan();
        let viewport = if self.kind == 6 {
            let size = mosaic_size(properties);
            (
                (1280. / size).ceil().min(1920.) as u32,
                (720. / size).ceil().min(1080.) as u32,
            )
        } else {
            resources.extent
        };
        CapturePlan {
            resources,
            viewport,
        }
    }
    pub fn blur_stage0(&self, properties: &HigurashiProperties) -> Option<Packet> {
        (self.kind == 5).then(|| blur_packet(properties, 0, self.weights))
    }
    pub fn packet(
        &self,
        properties: &HigurashiProperties,
        pass: PassKind,
    ) -> Result<Option<Packet>> {
        match self.kind {
            5 => {
                if pass != PassKind::Opaque || !properties.is_visible() {
                    return Ok(None);
                }
                Ok(Some(blur_packet(properties, 1, self.weights)))
            }
            6 => Ok(Some(mosaic_packet(properties))), // Original ignores pass and visibility.
            10 if pass == PassKind::Opaque => Ok(Some(raster_packet(properties, self.phases))),
            _ => Ok(None),
        }
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        source: TextureSource,
        properties: &HigurashiProperties,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        if let Some(packet) = self.packet(properties, kind)? {
            let clipped =
                self.kind == 5 && super::leaf_clip::Clip::from_properties(properties).prepare(pass);
            packet.render_clipped(pass, source, stencil, clipped);
        }
        Ok(())
    }
    pub fn render_blur_stage0(
        &self,
        pass: &mut RenderPass,
        source: TextureSource,
        properties: &HigurashiProperties,
    ) -> Result<()> {
        ensure!(
            self.kind == 5,
            "Saved effect stage0 is only defined for Blur5"
        );
        blur_packet(properties, 0, self.weights).render(pass, source, 0);
        Ok(())
    }
}

/// Original681f8 uses two modff calls, retaining the signed fractional zeros.
/// Its integer component wraps at1000 cycles; it is not the later engine's
/// unbounded EffectPhases or a simple fractional remainder.
pub(crate) fn wrap_phase(current: f32, delta: f32) -> f32 {
    fn split(value: f32) -> (f32, f32) {
        let integer = value.trunc();
        let fractional = if value.is_infinite() || value == integer {
            0.0f32.copysign(value)
        } else {
            value - integer
        };
        (integer, fractional)
    }
    let (integer, fractional) = split(current + delta);
    let fraction = if fractional < 0. {
        fractional + 1.
    } else {
        fractional
    };
    let (_, cycles) = split(integer * f32::from_bits(0x3a83126f));
    let cycles = if cycles < 0. { cycles + 1. } else { cycles };
    fraction + cycles * 1000.
}

pub(crate) struct Owner {
    view: View,
    cache: std::sync::Arc<parking_lot::Mutex<Option<super::effect_capture::Cache>>>,
}
impl std::fmt::Debug for Owner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HigurashiEffect")
            .field("view", &self.view)
            .finish_non_exhaustive()
    }
}
impl Clone for Owner {
    fn clone(&self) -> Self {
        Self {
            view: self.view.clone(),
            // Virtual38 Mosaic deep copy allocates a fresh texture. Blur and
            // Raster retain their existing GPU owner. Allocation is deferred
            // until the first preparation, without sharing Mosaic's storage.
            cache: if self.view.kind == 6 {
                std::sync::Arc::new(parking_lot::Mutex::new(None))
            } else {
                self.cache.clone()
            },
        }
    }
}
impl Owner {
    #[cfg(test)]
    pub(crate) fn reference_native_words(&self) -> Vec<u32> {
        match self.view.kind {
            5 => self.view.weights.map(f32::to_bits).to_vec(),
            10 => self.view.phases.map(f32::to_bits).to_vec(),
            _ => Vec::new(),
        }
    }
    pub fn replacement(kind: i32, old: Option<&Self>) -> Result<Self> {
        Ok(Self {
            view: View::replacement(kind, old.map(|old| &old.view))?,
            cache: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        })
    }
    pub fn kind(&self) -> i32 {
        self.view.kind
    }
    pub fn advance(&mut self, delta: shin_core::time::Ticks, properties: &HigurashiProperties) {
        self.view.advance(delta, properties);
    }
    pub fn prepare(
        &self,
        context: &mut crate::render::PreRenderContext,
        properties: &HigurashiProperties,
        depth: &super::effect_capture::SharedDepth,
        lower: &[crate::adv::higurashi::group::DrawableChild<'_>],
        parent: &HigurashiTransform,
    ) -> Result<()> {
        self.cache
            .lock()
            .get_or_insert_with(|| super::effect_capture::Cache::new(context, &self.view))
            .prepare(context, &self.view, properties, depth, lower, parent)
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        properties: &HigurashiProperties,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        let cache = self.cache.lock();
        let cache = cache.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Higurashi effect needs lower-prefix GPU preparation")
        })?;
        let sampler = if self.view.kind == 6 {
            shin_render::shaders::types::texture::TextureSampler::HigurashiMosaic
        } else {
            shin_render::shaders::types::texture::TextureSampler::Linear
        };
        self.view
            .render(pass, cache.source(sampler), properties, stencil, kind)
    }
}

#[cfg(test)]
mod live_tests;

#[derive(Clone, Debug)]
pub(crate) struct Packet {
    pub program: u32,
    pub stage: u32,
    pub projection: Mat4,
    pub color: Vec4,
    pub param0: Vec4,
    pub param1: Vec4,
    pub distances: [Vec4; 2],
    pub weights: Vec4,
    pub operation: LayerFragmentShader,
    pub native_sampler: Option<[u32; 5]>,
    pub stencil: bool,
    pub vertices: [PosTexVertex; 4],
}
impl Packet {
    pub fn render(&self, pass: &mut RenderPass, source: TextureSource, stencil: u8) {
        self.render_clipped(pass, source, stencil, false);
    }
    pub fn render_clipped(
        &self,
        pass: &mut RenderPass,
        source: TextureSource,
        stencil: u8,
        clipped: bool,
    ) {
        // Root passes the original Mosaic custom sampler through this source.
        // Native mask0 means strict Greater, including both Mosaic passes.
        let mut builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque);
        if self.stencil {
            builder = builder.depth_stencil_shorthand(stencil, false, clipped);
        }
        let vertices = VertexSource::VertexData {
            vertices: &self.vertices,
        };
        let transform = HigurashiTransform::convert_clip_depth(self.projection);
        let arguments = match self.program {
            23 => RenderProgramWithArguments::HigurashiBlur {
                vertices,
                texture: source,
                transform,
                distances: self.distances,
                color: self.color,
                weights: self.weights,
            },
            24 => RenderProgramWithArguments::HigurashiRaster {
                vertices,
                texture: source,
                transform,
                color: self.color,
                amplitude_frequency: self.param0,
                phase: self.param1,
            },
            _ => RenderProgramWithArguments::HigurashiPostprocess {
                vertices,
                texture: source,
                transform,
                color: FloatColor4::from_vec4(self.color),
                operation: self.operation,
                param: self.param0,
            },
        };
        pass.run(builder.build(arguments, DrawPrimitive::TrianglesStrip));
    }
}

/// Actual68278 result, executed independently. Blur uses this raw projection;
/// Mosaic/Raster appendT(-640,-720), instead of an authored effect transform.
fn projection(translated: bool) -> Mat4 {
    let matrix = Mat4::from_cols_array(&[
        f32::from_bits(0x3acccccd),
        0.,
        0.,
        0.,
        0.,
        f32::from_bits(0xbb360b61),
        0.,
        0.,
        0.,
        0.,
        -1.,
        0.,
        -0.,
        -1.,
        -0.,
        1.,
    ]);
    if translated {
        multiply_transforms(matrix, Mat4::from_translation(vec3(-640., -720., 0.)))
    } else {
        matrix
    }
}
fn quad(uv: Vec4) -> [PosTexVertex; 4] {
    [
        (0., 0., uv.x, uv.y),
        (1280., 0., uv.z, uv.y),
        (0., 720., uv.x, uv.w),
        (1280., 720., uv.z, uv.w),
    ]
    .map(|(x, y, u, v)| PosTexVertex {
        position: Vec2::new(x, y),
        texture_position: Vec2::new(u, v),
    })
}
fn blank(program: u32, translated: bool) -> Packet {
    Packet {
        program,
        stage: 0,
        projection: projection(translated),
        color: Vec4::ONE,
        param0: Vec4::ZERO,
        param1: Vec4::ZERO,
        distances: [Vec4::ZERO; 2],
        weights: Vec4::ZERO,
        operation: LayerFragmentShader::Default,
        native_sampler: None,
        stencil: true,
        vertices: quad(vec4(0., 0., 1., 1.)),
    }
}
fn blur_packet(properties: &HigurashiProperties, stage: u32, weights: [f32; 4]) -> Packet {
    let mut packet = blank(23, false);
    packet.stage = stage;
    packet.stencil = stage != 0;
    packet.color = if stage == 0 {
        Vec4::ONE
    } else {
        properties.drawable_params().color_multiplier.into_vec4()
    };
    let radius = properties.values()[49] * f32::from_bits(0x3a83126f);
    packet.distances = if stage == 0 {
        [
            vec4(
                radius * f32::from_bits(0x3a4ccccd),
                0.,
                radius * f32::from_bits(0x3acccccd),
                0.,
            ),
            vec4(radius * f32::from_bits(0x3b19999a), 0., 0., 0.),
        ]
    } else {
        [
            vec4(
                0.,
                radius * f32::from_bits(0x3ab60b61),
                0.,
                radius * f32::from_bits(0x3b360b61),
            ),
            vec4(0., radius * f32::from_bits(0x3b888889), 0., 0.),
        ]
    };
    packet.weights = Vec4::from_array(weights);
    packet
}
fn mosaic_size(properties: &HigurashiProperties) -> f32 {
    (properties.values()[50] as i32).clamp(1, 320) as f32
}
fn mosaic_packet(properties: &HigurashiProperties) -> Packet {
    let drawable = properties.drawable_params();
    let operation = drawable.fragment_shader; // Original8b584 indexes directly.
    let mut packet = blank([6, 9, 7, 8, 10, 11][operation as usize], true);
    packet.operation = operation;
    let color = drawable.color_multiplier.into_vec4();
    packet.color = color * vec4(color.w, color.w, color.w, 1.);
    packet.param0 = drawable.shader_param;
    //8b518 STUR XZR zeros both first words of the initialized descriptor.
    packet.native_sampler = Some([0, 0, 1, 0, 0]);
    let size = mosaic_size(properties);
    let width = (1280. / size).min(1920.);
    let height = (720. / size).min(1080.);
    let ceil_width = (1280. / size).ceil().min(1920.);
    let ceil_height = (720. / size).ceil().min(1080.);
    let u0 = ((ceil_width - width) * 0.5) / 1920.;
    let v0 = ((ceil_height - height) * 0.5) / 1080.;
    packet.vertices = quad(vec4(u0, v0, width / 1920. + u0, height / 1080. + v0));
    packet
}
fn raster_packet(properties: &HigurashiProperties, phases: [f32; 2]) -> Packet {
    let p = properties.values();
    let frequency = |value: f32| {
        let value = value * f32::from_bits(0x3a83126f);
        if value == 0. { value } else { 1. / value }
    };
    let mut packet = blank(24, true);
    packet.color = properties.drawable_params().color_multiplier.into_vec4();
    packet.param0 = vec4(
        p[54] / 1280.,
        p[57] / 720.,
        frequency(p[55]),
        frequency(p[58]),
    );
    packet.param1 = vec4(phases[0], phases[1], 0., 0.);
    packet
}
