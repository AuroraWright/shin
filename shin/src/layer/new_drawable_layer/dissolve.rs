//! Native dissolve mesh, 0x776e0..0x77ddc. Shader47 samples source RGBA
//! and breakup alpha, multiplying all four channels (0x1e0e58..0x1e0e98).
use glam::{Vec2, vec2};
use shin_render::shaders::types::vertices::{MaskVertex, PosTexVertex};

pub struct DissolveMesh {
    pub intact: Vec<PosTexVertex>,
    pub pieces: Vec<MaskVertex>,
}

fn random(seed: &mut u32) -> f32 {
    let value = i64::from(*seed % 44_488) * 48_271 - i64::from(*seed / 44_488) * 3_399;
    *seed = if value < 0 {
        (value + 2_147_483_647) as u32
    } else {
        value as u32
    };
    (*seed - 1) as f32 * f32::from_bits(0x30000000)
}

impl DissolveMesh {
    pub fn new(intensity: f32, mode: i32, seed: u32) -> Self {
        let progress = 1.0 - intensity.min(1.0) * 3.0;
        let canvas = vec2(1920.0, 1080.0);
        let mut intact = Vec::new();
        if progress > -1.0 && progress < 1.0 {
            let span = (progress + 1.0) * 0.5 * 2971.72;
            let left = if mode & 1 != 0 { 1920.0 - span } else { 0.0 };
            let top = if mode & 2 != 0 { 0.0 } else { 1080.0 - span };
            let corners = [
                vec2(left, top),
                vec2(left + span, top),
                vec2(left, top + span),
                vec2(left + span, top + span),
            ];
            let indices = [[0, 3, 2], [2, 1, 3], [0, 1, 2], [0, 1, 3]][(mode & 3) as usize];
            for index in indices {
                let position = corners[index];
                intact.push(PosTexVertex {
                    position,
                    texture_position: position / canvas,
                });
            }
        }
        let phase = match mode & 3 {
            0 => 0.875,
            1 => 0.625,
            2 => 0.125,
            _ => 0.375,
        };
        let mut seed = (seed % 2_147_483_647).max(1);
        let mut pieces = Vec::new();
        for row in 0..54 {
            let rank_y = if mode & 2 != 0 { 54 - row } else { row };
            for col in 0..96 {
                let rank_x = if mode & 1 != 0 { col } else { 96 - col };
                // Consume both values even for culled cells: native layout remains
                // stable as intensity changes, with the same initial per-layer seed.
                let angle_random = random(&mut seed);
                let distance_random = random(&mut seed);
                let age = progress + ((rank_x + rank_y) as f32 / 148.0) * 2.0;
                if age <= 0.0 || age > 1.1 {
                    continue;
                }
                let displacement = if age < 1.0 {
                    let distance =
                        (distance_random * 800.0 + 200.0) * (1.0 - ((1.0 - age) * 1.570795).cos());
                    let angle = (angle_random * 0.48 - 0.24 + phase) * 6.28318;
                    vec2(angle.cos(), angle.sin()) * distance
                } else {
                    Vec2::ZERO
                };
                let center = vec2(col as f32 * 20.0 + 10.0, row as f32 * 20.0 + 10.0);
                let half_size = age * 32.0 * 0.5;
                let corners = [Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE];
                for index in [0, 1, 2, 2, 1, 3] {
                    let mask_position = corners[index];
                    let unshifted = center + (mask_position * 2.0 - 1.0) * half_size;
                    pieces.push(MaskVertex {
                        position: unshifted + displacement,
                        texture_position: unshifted / canvas,
                        mask_position,
                    });
                }
            }
        }
        Self { intact, pieces }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_rng_and_intact_triangle_directions() {
        let mut seed = 1;
        let _ = random(&mut seed);
        assert_eq!(seed, 48271);
        let _ = random(&mut seed);
        assert_eq!(seed, 182605794);
        for mode in 0..4 {
            let mesh = DissolveMesh::new(0.5, mode, 1);
            assert_eq!(mesh.intact.len(), 3);
            assert!(!mesh.pieces.is_empty());
            assert_eq!(mesh.pieces.len() % 6, 0);
            let corner = vec2(
                if mode & 1 != 0 { 1920.0 } else { 0.0 },
                if mode & 2 != 0 { 0.0 } else { 1080.0 },
            );
            assert!(mesh.intact.iter().any(|v| {
                let position = v.position;
                position == corner
            }));
            for piece in &mesh.pieces {
                assert!(piece.position.is_finite());
                assert!(piece.texture_position.is_finite());
            }
        }
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_dissolve_shader_samples_breakup_alpha_and_multiplies_rgba() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let file = std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable");
        let executable = std::fs::read(file).unwrap();
        // Disassembled Maxwell fragment program47: TEX alpha, TEX RGBA,
        // then four FMUL operations with the alpha sample in register6.
        for (offset, expected) in [
            (0x68, 0xd82e00aff0770606u64),
            (0x78, 0xd832008020570400),
            (0x88, 0x5c68100000670000),
            (0x90, 0x5c68100000670101),
            (0x98, 0x5c68100000670202),
            (0xa8, 0x5c68100000670303),
        ] {
            let start = 0x1e0df0 + offset + 256;
            assert_eq!(
                u64::from_le_bytes(executable[start..start + 8].try_into().unwrap()),
                expected
            );
        }
    }

    #[test]
    fn saturation_and_repeat_frames_preserve_native_layout() {
        let a = DissolveMesh::new(1.0, 0, 123);
        let b = DissolveMesh::new(10.0, 0, 123);
        assert!(a.intact.is_empty());
        for (mode, count) in [6, 0, 18, 6].into_iter().enumerate() {
            assert_eq!(
                DissolveMesh::new(1.0, mode as i32, 123).pieces.len(),
                count,
                "Native rank endpoint keeps subpixel corner pieces"
            );
        }
        assert_eq!(a.pieces.len(), b.pieces.len());
        assert!(a.pieces.iter().zip(&b.pieces).all(|(a, b)| {
            let pa = a.position;
            let pb = b.position;
            pa == pb
        }));
        let a = DissolveMesh::new(0.5, 0, 123);
        let b = DissolveMesh::new(0.5, 0, 123);
        assert!(a.pieces.iter().zip(&b.pieces).all(|(a, b)| {
            let pa = a.position;
            let pb = b.position;
            pa == pb
        }));
    }
}
