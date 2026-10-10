use crate::{
    render::normalized_projection_matrix,
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::timed::{TimedWiper, TimedWiperWrapper},
};
use glam::{Mat4, Vec2, Vec3, Vec4};
use shin_core::time::Ticks;
use shin_render::{
    ColorBlendType, CullFace, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        texture::TextureSource,
        vertices::{GlassVertex, PosTexVertex},
    },
};

const INDICES: [u16; 24] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 8, 7, 10, 11, 12, 13, 12, 11, 14, 15, 16, 17, 16, 15,
];
const SIDE_VERTICES: [[usize; 4]; 3] = [[0, 3, 1, 5], [1, 5, 2, 4], [2, 4, 0, 3]];

#[derive(Debug, Clone)]
struct Piece {
    vertices: [GlassVertex; 18],
    position: Vec3,
    velocity: Vec3,
    angles: Vec3,
    angular_velocity: Vec3,
    falling_velocity: f32,
}

#[derive(Debug, Clone)]
pub struct GlassWiperImpl {
    seed: u32,
    pieces: Vec<Piece>,
    rate: f32,
    physics_time: f32,
}

impl GlassWiperImpl {
    fn new(duration: Ticks, seed: u32) -> Self {
        let rate = 60.0 / duration.as_f32();
        let mut state = (seed % 2_147_483_647).max(1);
        let mut random = || {
            let value = i64::from(state % 44_488) * 48_271 - i64::from(state / 44_488) * 3_399;
            state = if value < 0 {
                (value + 2_147_483_647) as u32
            } else {
                value as u32
            };
            (state - 1) as f32 * f32::from_bits(0x30000000)
        };
        // Consume both samples even for boundary points; the native constructor fixes edges later.
        let mut grid = [Vec2::ZERO; 104];
        for row in 0..8 {
            for col in 0..13 {
                grid[row * 13 + col] = Vec2::new(
                    col as f32 * 160.0 + (random() * 128.0 - 64.0),
                    row as f32 * 154.28572 + (random() * 123.42858 - 61.71429),
                );
            }
        }
        for row in 0..8 {
            grid[row * 13].x = 0.0;
            grid[row * 13 + 12].x = 1920.0;
        }
        for col in 0..13 {
            grid[col].y = 0.0;
            grid[91 + col].y = 1080.0;
        }
        let mut pieces = Vec::with_capacity(168);
        for row in 0..7 {
            for triangle in 0..24 {
                let base = row * 13 + triangle / 2;
                let points = if triangle % 2 == 0 {
                    [grid[base], grid[base + 1], grid[base + 13]]
                } else {
                    [grid[base + 14], grid[base + 13], grid[base + 1]]
                };
                let front = points.map(|p| Vec3::new(p.x - 960.0, -(p.y - 540.0), 0.0));
                let position = Vec3::new(
                    (front[0].x + front[1].x + front[2].x) / 3.0 + (random() * 80.0 - 40.0),
                    (front[0].y + front[1].y + front[2].y) / 3.0 + (random() * 77.14286 - 38.57143),
                    -4.0,
                );
                let coordinates = [
                    front[0],
                    front[1],
                    front[2],
                    front[0] - Vec3::Z * 8.0,
                    front[2] - Vec3::Z * 8.0,
                    front[1] - Vec3::Z * 8.0,
                ]
                .map(|p| p - position);
                let uv = points
                    .map(|p| p * Vec2::new(f32::from_bits(0x3a088889), f32::from_bits(0x3a72b9d6)));
                let uvs = [uv[0], uv[1], uv[2], uv[0], uv[2], uv[1]];
                let mut vertices = [GlassVertex {
                    position: Vec3::ZERO,
                    normal: Vec3::ZERO,
                    texture_position: Vec2::ZERO,
                }; 18];
                for i in 0..6 {
                    vertices[i] = GlassVertex {
                        position: coordinates[i],
                        normal: if i < 3 { -Vec3::Z } else { Vec3::Z },
                        texture_position: uvs[i],
                    };
                }
                let normals = [
                    (coordinates[1] - coordinates[0])
                        .cross(coordinates[0] - coordinates[3])
                        .normalize(),
                    (coordinates[2] - coordinates[1])
                        .cross(coordinates[1] - coordinates[5])
                        .normalize(),
                    (coordinates[0] - coordinates[2])
                        .cross(coordinates[2] - coordinates[4])
                        .normalize(),
                ];
                for side in 0..3 {
                    for (i, &source) in SIDE_VERTICES[side].iter().enumerate() {
                        vertices[6 + side * 4 + i] = GlassVertex {
                            position: coordinates[source],
                            normal: normals[side],
                            texture_position: uvs[source],
                        };
                    }
                }
                pieces.push(Piece {
                    vertices,
                    position,
                    velocity: Vec3::new(
                        position.x * 0.4 / 60.0 * rate,
                        position.y * 0.4 / 60.0 * rate,
                        15.0 * rate,
                    ),
                    angles: Vec3::ZERO,
                    angular_velocity: Vec3::new(
                        random() * 0.10471667 - 0.052358337,
                        random() * 0.10471667 - 0.052358337,
                        random() * 0.10471667 - 0.052358337,
                    ) * rate,
                    falling_velocity: (1080.0 - position.y) * 2.0 / 1080.0,
                });
            }
        }
        Self {
            seed,
            pieces,
            rate,
            physics_time: 0.0,
        }
    }

    fn advance(&mut self, delta: f32) {
        let new_time = self.physics_time + self.rate * delta;
        for piece in &mut self.pieces {
            piece.angles += piece.angular_velocity * delta;
            piece.position += piece.velocity * delta;
            let mut time = self.physics_time;
            while time < new_time {
                if (new_time.floor() - time.floor()).abs() < f32::EPSILON {
                    piece.position.y += (new_time - time) * piece.falling_velocity;
                    break;
                }
                let boundary = time.floor() + 1.0;
                piece.position.y += (boundary - time) * piece.falling_velocity;
                piece.falling_velocity -= 0.35;
                time = boundary;
            }
        }
        self.physics_time = new_time;
    }
}

impl AdvUpdatable for GlassWiperImpl {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.advance(context.delta_ticks.as_f32());
    }
}

impl TimedWiper for GlassWiperImpl {
    #[cfg(test)]
    fn reference_set_batch_words(&mut self, words: &[u32]) {
        self.seed = words[0];
        self.rate = f32::from_bits(words[1]);
        self.physics_time = f32::from_bits(words[3]);
    }

    fn batch_parameters_match(&self, other: &Self) -> bool {
        self.seed == other.seed
            && self.rate == other.rate
            && self.physics_time == other.physics_time
    }
    fn batch_uses_timer(&self) -> bool {
        // Native6c930 compares seed/rate/physics time;the timer is excluded.
        false
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        texture_target: TextureSource,
        texture_source: TextureSource,
        progress: f32,
    ) {
        let quad = build_quad_vertices(|t| PosTexVertex {
            position: t,
            texture_position: t,
        });
        // Background is always the new scene. The native fragment discards the old scene alpha.
        pass.run(builder.color_blend_type(ColorBlendType::Opaque).build(
            RenderProgramWithArguments::WiperDefault {
                vertices: VertexSource::VertexData { vertices: &quad },
                texture_source: texture_target,
                texture_target,
                transform: normalized_projection_matrix(),
                alpha: 1.0,
            },
            DrawPrimitive::TrianglesStrip,
        ));
        if progress >= 1.0 {
            return;
        }
        let opacity = if progress >= 0.75 {
            1.0 - (4.0 * (progress - 0.75)).powi(2)
        } else {
            1.0
        };
        let flash = if progress < 0.5 {
            0.5 * (1.0 - 2.0 * progress)
        } else {
            0.0
        };
        // Native OpenGL perspective, converted to wgpu's [0,1] clip depth.
        let projection = Mat4::perspective_rh(0.99026656, 1920.0 / 1080.0, 10.0, 10000.0)
            * Mat4::from_translation(Vec3::new(0.0, 0.0, -1000.0));
        for piece in &self.pieces {
            let model = Mat4::from_translation(piece.position)
                * Mat4::from_rotation_z(piece.angles.z)
                * Mat4::from_rotation_y(piece.angles.y)
                * Mat4::from_rotation_x(piece.angles.x);
            pass.run(
                builder
                    .color_blend_type(ColorBlendType::Layer1)
                    .cull_faces(CullFace::Front)
                    .build(
                        RenderProgramWithArguments::WiperGlass {
                            vertices: VertexSource::VertexAndIndexData {
                                vertices: &piece.vertices,
                                indices: &INDICES,
                            },
                            texture: texture_source,
                            transform: projection * model,
                            model,
                            effect: Vec4::new(opacity, flash, 0.0, 0.0),
                        },
                        DrawPrimitive::Triangles,
                    ),
            );
        }
    }
}

pub type GlassWiper = TimedWiperWrapper<GlassWiperImpl>;
impl GlassWiper {
    pub(crate) fn advance_ticks_with_permission(&mut self, delta: f32, allow_animation: bool) {
        // Native6ca3c gates the progress clock, then6ca40..6cb60 always
        // integrates piece motion with this frame's real delta.
        self.state.update(allow_animation, Ticks::from_f32(delta));
        self.inner.advance(delta);
    }

    #[cfg(test)]
    pub(crate) fn reference_motion(&self) -> (f32, Vec<(Vec3, Vec3, f32)>) {
        (
            self.inner.physics_time,
            self.inner
                .pieces
                .iter()
                .map(|piece| (piece.position, piece.angles, piece.falling_velocity))
                .collect(),
        )
    }

    pub fn new(duration: Ticks) -> Self {
        Self::with_seed(duration, crate::engine_random::next_seed())
    }
    pub(crate) fn with_seed(duration: Ticks, seed: u32) -> Self {
        Self::from_inner(GlassWiperImpl::new(duration, seed), duration)
    }
}

#[cfg(test)]
impl GlassWiper {
    /// Scalar rasterizer independent of the WGSL and matrix projection paths.
    pub(crate) fn reference_pixel(ticks: f32, progress: f32, x: usize, y: usize) -> [u8; 4] {
        let mut model = GlassWiperImpl::new(Ticks::from_f32(100.0), 1);
        model.advance(ticks);
        let mut color = [
            255.0 - y as f32 * 255.0 / 1079.0,
            x as f32 * 255.0 / 1919.0,
            230.0,
            255.0,
        ];
        let opacity = if progress >= 0.75 {
            1.0 - (4.0 * (progress - 0.75)).powi(2)
        } else {
            1.0
        };
        if progress < 1.0 {
            for piece in &model.pieces {
                let rotate = |mut v: Vec3| {
                    let (s, c) = piece.angles.x.sin_cos();
                    v = Vec3::new(v.x, c * v.y - s * v.z, s * v.y + c * v.z);
                    let (s, c) = piece.angles.y.sin_cos();
                    v = Vec3::new(c * v.x + s * v.z, v.y, -s * v.x + c * v.z);
                    let (s, c) = piece.angles.z.sin_cos();
                    Vec3::new(c * v.x - s * v.y, s * v.x + c * v.y, v.z)
                };
                for indices in INDICES.chunks_exact(3) {
                    let v =
                        [indices[0], indices[1], indices[2]].map(|i| piece.vertices[i as usize]);
                    let world = v.map(|v| rotate(v.position) + piece.position);
                    // All tested pieces are between the native near and far planes.
                    let reciprocal_w = world.map(|v| 1.0 / (1000.0 - v.z));
                    let projected = std::array::from_fn::<_, 3, _>(|i| {
                        Vec2::new(
                            960.0 + world[i].x * 1000.0 * reciprocal_w[i],
                            540.0 - world[i].y * 1000.0 * reciprocal_w[i],
                        )
                    });
                    let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
                    let area = cross(projected[1] - projected[0], projected[2] - projected[0]);
                    if area <= 0.0 {
                        continue;
                    }
                    let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let weights = [
                        cross(projected[1] - point, projected[2] - point) / area,
                        cross(projected[2] - point, projected[0] - point) / area,
                        cross(projected[0] - point, projected[1] - point) / area,
                    ];
                    if weights.iter().any(|&w| w < 0.0) {
                        continue;
                    }
                    let denominator = (0..3).map(|i| weights[i] * reciprocal_w[i]).sum::<f32>();
                    let uv = (0..3)
                        .map(|i| v[i].texture_position * weights[i] * reciprocal_w[i] / denominator)
                        .sum::<Vec2>();
                    let normal = rotate(v[0].normal);
                    let flash = if progress < 0.5 {
                        0.5 * (1.0 - 2.0 * progress)
                    } else {
                        0.0
                    };
                    let lighting =
                        (-(normal.x + normal.y) * f32::from_bits(0x3e3504f3) + flash) * 255.0;
                    // Texture fixture is a UNORM gradient with integer texels and linear sampling.
                    let sample = |u: f32, n: f32| {
                        let t = (u * n - 0.5).clamp(0.0, n - 1.0);
                        let lo = t.floor();
                        let hi = (lo + 1.0).min(n - 1.0);
                        (lo * 255.0 / (n - 1.0)).floor() * (1.0 - (t - lo))
                            + (hi * 255.0 / (n - 1.0)).floor() * (t - lo)
                    };
                    let src = [
                        sample(uv.x, 1920.0) + lighting,
                        sample(uv.y, 1080.0) + lighting,
                        20.0 + lighting,
                    ];
                    for i in 0..3 {
                        color[i] = (src[i].clamp(0.0, 255.0) * opacity
                            + color[i] * (1.0 - opacity))
                            .clamp(0.0, 255.0)
                            .round();
                    }
                    color[3] = (255.0 * opacity * (1.0 - color[3] / 255.0) + color[3]).round();
                }
            }
        }
        color.map(|c| c.round() as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prism_mesh_covers_canvas_and_preserves_source_uvs() {
        let mesh = GlassWiperImpl::new(Ticks::from_f32(100.0), 1);
        assert_eq!(mesh.pieces.len(), 168);
        assert_eq!(std::mem::size_of::<GlassVertex>(), 32);
        let mut area = 0.0;
        for piece in &mesh.pieces {
            let front = [0, 1, 2].map(|i| piece.vertices[i].position + piece.position);
            area += (front[1] - front[0]).cross(front[2] - front[0]).z.abs() * 0.5;
            for vertex in piece.vertices {
                let world = vertex.position + piece.position;
                let uv = vertex.texture_position;
                assert!((world.x - (uv.x * 1920.0 - 960.0)).abs() < 0.001);
                assert!((world.y - (540.0 - uv.y * 1080.0)).abs() < 0.001);
                assert!(world.z.abs() < 0.001 || (world.z + 8.0).abs() < 0.001);
                assert!((vertex.normal.length() - 1.0).abs() < 0.00001);
            }
        }
        assert!((area - 1920.0 * 1080.0).abs() < 1.0);
        let first = mesh.pieces[0].vertices[0].position + mesh.pieces[0].position;
        assert_eq!(first, Vec3::new(-960.0, 540.0, 0.0));
        let last = mesh.pieces[167].vertices[0].position + mesh.pieces[167].position;
        assert_eq!(last, Vec3::new(960.0, -540.0, 0.0));
    }

    #[test]
    fn gravity_integrates_fractional_ticks_and_clone_preserves_motion() {
        let mut one = GlassWiperImpl::new(Ticks::from_f32(60.0), 1);
        let before = one.pieces[0].position;
        let velocity = one.pieces[0].velocity;
        let fall = one.pieces[0].falling_velocity;
        let mut split = one.clone();
        one.advance(2.25);
        for delta in [0.25, 0.75, 0.5, 0.75] {
            split.advance(delta);
        }
        let expected_y = before.y + velocity.y * 2.25 + fall + (fall - 0.35) + 0.25 * (fall - 0.7);
        assert!((one.pieces[0].position.y - expected_y).abs() < 0.0001);
        assert!((one.pieces[0].position.z - (before.z + 15.0 * 2.25)).abs() < 0.0001);
        for (a, b) in one.pieces.iter().zip(&split.pieces) {
            assert!((a.position - b.position).length() < 0.0003);
            assert!((a.angles - b.angles).length() < 0.00001);
            assert_eq!(a.falling_velocity, b.falling_velocity);
        }
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_glass_layout_and_shader_match_native() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        assert_eq!(0x21960 + usize::from(exe[0x1653f4 + 256 + 9]) * 4, 0x21a34);
        for (i, index) in INDICES.iter().enumerate() {
            let a = 0x16fb20 + 256 + i * 2;
            assert_eq!(
                u16::from_le_bytes(exe[a..a + 2].try_into().unwrap()),
                *index
            );
        }
        for (a, expected) in [
            (0x159a68, 80.0f32),
            (0x159364, 77.14286),
            (0x159514, 0.10471667),
            (0x1595fc, -0.052358337),
            (0x159a88, 0.99026656),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
                expected.to_bits()
            );
        }
        // Native pauses only progress; real-delta piece integration follows unconditionally.
        for (address, expected) in [
            (0x6ca30, 0x12000021u32),
            (0x6ca34, 0x4ea01c08u32),
            (0x6ca3c, 0x9402b9e9u32),
            (0x6ca40, 0xbd402a61u32),
            (0x6ca48, 0x1e280821u32),
            (0x6ca54, 0x1e212800u32),
            (0x6ca70, 0x1e280884u32),
            (0x6ca78, 0x1e2428a4u32),
            (0x6cab0, 0x1e280887u32),
            (0x6cab4, 0x1e2728a4u32),
            (0x6cb18, 0xbd402e65u32),
            (0x6cb1c, 0x1e2538e5u32),
            (0x6cb60, 0xbd003260u32),
            (0x11b1e0, 0x360000e1u32),
            (0x11b1fc, 0xd65f03c0u32),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "Native Glass update {address:x}"
            );
        }
        // Inline shader lighting coefficient, old scene RGB sampling, and RGB brightness addition.
        for (a, opcode) in [
            (0x214f70 + 0xb40 + 0x130, 0x1e2be3504f370404u64),
            (0x2142b0 + 0xb40 + 0x48, 0xd822008020570400),
            (0x2142b0 + 0xb40 + 0x78, 0x5c58100000670000),
        ] {
            assert_eq!(
                u64::from_le_bytes(exe[a + 256..a + 264].try_into().unwrap()),
                opcode
            );
        }
    }
}
