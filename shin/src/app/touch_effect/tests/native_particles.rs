use super::*;

// Exact native particle records for prepared drag/release callbacks and clocks.
// The native input-area dispatch/platform frame clock is supplied by the host.
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
#[ignore = "requires supplied executable images; no audio/GPU"]
fn reference_native_touch_particle_records() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let images = std::env::var_os("SHIN_REFERENCE_ROM")
        .map(std::path::PathBuf::from)
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| root.join("game-data/umineko"));
    let binary = std::env::temp_dir().join(format!(
        "shin-native-touch-particles-{}",
        std::process::id()
    ));
    assert!(
        Command::new("/usr/bin/clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-touch-particles.c"))
            .arg("-lm")
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap()
            .success()
    );
    let mut compared = 0usize;
    for seed in [5489, 1, u32::MAX] {
        let mut effect = TouchEffect::with_seed(seed);
        let mut input = String::new();
        let mut expected = String::new();
        let mut previous = Vec2::new(90.0, 90.0);
        for step in 0..1800 {
            let (delta, enabled, mode, current) = match step {
                0 => (1.0, true, 1, previous + Vec2::Y * 40.0),
                1..=1440 => (
                    1.0,
                    true,
                    1,
                    Vec2::new(
                        90.0,
                        90.0 + ((step % 180).min(180 - step % 180)) as f32 * 10.0,
                    ),
                ),
                1441..=1620 => (
                    1.0,
                    true,
                    if step % 20 == 0 { 2 } else { 0 },
                    Vec2::new(100.0 + (step % 3) as f32 * 5.0, 300.0),
                ),
                1621..=1640 => (0.5, step % 2 == 0, 1, previous + Vec2::X * 9.0),
                1641..=1660 => (2.0, true, 1, previous + Vec2::X * 10.0),
                1661..=1670 => (0.0, true, 2, Vec2::new(500.0, 300.0)),
                1671..=1680 => (2.0, true, 0, previous),
                1681 => (61.0, false, 0, previous),
                _ => (1.0, false, 0, previous),
            };
            // No held pointer: dispatch these prepared native callbacks directly.
            effect.update(
                Ticks::from_f32(delta),
                enabled,
                false,
                Vec2::new(960.0, 540.0),
            );
            if enabled {
                if mode == 1 {
                    effect.emit_drag(previous, current);
                } else if mode == 2 {
                    effect.release(current);
                }
            }
            input += &format!(
                "{} {delta} {} {mode} {} {} {} {} 0\n",
                effect.clock,
                u8::from(enabled),
                previous.x,
                previous.y,
                current.x,
                current.y
            );
            expected += &format!(
                "F {} {} {} {}\n",
                effect.clock,
                effect.stars.len(),
                effect.ripples.len(),
                effect.last_emission
            );
            for s in &effect.stars {
                expected += &format!(
                    "S {} {} {} {} {} {} {} {} {}\n",
                    s.variant,
                    s.position.x.to_bits(),
                    s.position.y.to_bits(),
                    s.velocity.x.to_bits(),
                    s.velocity.y.to_bits(),
                    s.angle.to_bits(),
                    s.spin.to_bits(),
                    s.scale.to_bits(),
                    s.age.to_bits()
                );
                compared += 1;
            }
            for r in &effect.ripples {
                expected += &format!(
                    "R {} {} {}\n",
                    r.position.x.to_bits(),
                    r.position.y.to_bits(),
                    r.age.to_bits()
                );
            }
            previous = current;
        }
        for version in ["uncompressed_executable", "uncompressed_executable_update"] {
            let mut child = Command::new(&binary)
                .arg(images.join(version))
                .arg(seed.to_string())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdin = child.stdin.take().unwrap();
            let input = input.clone();
            let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
            let output = child.wait_with_output().unwrap();
            writer.join().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let native = String::from_utf8(output.stdout).unwrap();
            assert_eq!(
                native.lines().count(),
                expected.lines().count(),
                "{version} seed{seed}: record count"
            );
            for (line, (actual, native)) in expected.lines().zip(native.lines()).enumerate() {
                assert_eq!(actual, native, "{version} seed{seed} record{line}");
            }
            eprintln!(
                "{version} seed{seed}:1800 steps, {} records exact",
                native.lines().count()
            );
        }
    }
    eprintln!("Touch star records per executable:{compared}");
    std::fs::remove_file(binary).unwrap();
}

pub(super) fn check_native_touch_star_packets(
    ctx: &mut crate::render::PreRenderContext,
    target: &wgpu::Texture,
    view: &wgpu::TextureView,
    atlas: &CharacterTextures,
    rom: &std::path::Path,
) {
    use shin_core::primitives::color::UnormColor;
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let binary = std::env::temp_dir().join(format!(
        "shin-native-touch-draw-session-{}",
        std::process::id()
    ));
    assert!(
        Command::new("/usr/bin/clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-touch-particles.c"))
            .arg("-lm")
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap()
            .success()
    );
    let deltas = [1.0, 0.25, 0.5, 1.0, 2.0, 10.0, 20.0, 30.25];
    let input = deltas
        .iter()
        .enumerate()
        .map(|(i, delta)| {
            format!(
                "{} {delta} 1 {} 450 400 470 400 1\n",
                i + 1,
                usize::from(i == 0)
            )
        })
        .collect::<String>();
    for version in ["uncompressed_executable", "uncompressed_executable_update"] {
        let mut child = Command::new(&binary)
            .arg(rom.parent().unwrap().join(version))
            .arg("5489")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = input.clone();
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let trace = String::from_utf8(output.stdout).unwrap();
        let mut frames: Vec<Vec<String>> = Vec::new();
        for line in trace.lines() {
            if line.starts_with("F ") {
                frames.push(Vec::new());
            } else if line.starts_with("Q ") {
                frames.last_mut().unwrap().push(line.to_owned());
            }
        }
        assert_eq!(frames.len(), deltas.len());
        let mut effect = TouchEffect::with_seed(5489);
        let mut compared = 0usize;
        let mut worst = 0;
        for (step, (delta, packets)) in deltas.iter().zip(frames).enumerate() {
            effect.update(
                Ticks::from_f32(*delta),
                true,
                false,
                Vec2::new(960.0, 540.0),
            );
            if step == 0 {
                effect.emit_drag(Vec2::new(450.0, 400.0), Vec2::new(470.0, 400.0));
            }
            assert_eq!(packets.len(), effect.stars.len());
            for background in [UnormColor::BLACK, UnormColor::from_rgba(40, 25, 20, 255)] {
                {
                    let mut pass = ctx.begin_pass(
                        TextureTarget {
                            kind: TextureTargetKind::Screen,
                            view,
                        },
                        Some(ctx.depth_stencil),
                        "native star production",
                    );
                    pass.clear(Some(background), Some(255), Some(0.0));
                    effect.render(&mut pass, atlas);
                }
                let actual = crate::layer::user::readback_test_texture(ctx, target);
                {
                    let mut pass = ctx.begin_pass(
                        TextureTarget {
                            kind: TextureTargetKind::Screen,
                            view,
                        },
                        Some(ctx.depth_stencil),
                        "native star packet replay",
                    );
                    pass.clear(Some(background), Some(255), Some(0.0));
                    for packet in &packets {
                        let w: Vec<_> = packet.split_whitespace().collect();
                        assert_eq!(&w[..3], ["Q", "8", "3"]);
                        assert_eq!(w.len(), 39);
                        let f = |i: usize| w[i].parse::<f32>().unwrap();
                        let matrix = Mat4::from_cols_array(&std::array::from_fn(|i| f(3 + i)));
                        let color = FloatColor4::from_rgba(f(19), f(20), f(21), f(22));
                        let vertices = std::array::from_fn::<_, 4, _>(|i| PosTexVertex {
                            position: Vec2::new(f(23 + 4 * i), f(24 + 4 * i)),
                            texture_position: Vec2::new(f(25 + 4 * i), f(26 + 4 * i)),
                        });
                        pass.run(
                            RenderRequestBuilder::new()
                                .color_blend_type(ColorBlendType::Layer2)
                                .build(
                                    RenderProgramWithArguments::Layer {
                                        output_kind: LayerShaderOutputKind::Layer,
                                        fragment_shader: LayerFragmentShader::Default,
                                        vertices: VertexSource::VertexData {
                                            vertices: &vertices,
                                        },
                                        texture: atlas.get("drag").as_source(),
                                        transform: matrix,
                                        color_multiplier: color,
                                        fragment_shader_param: Vec4::ZERO,
                                    },
                                    DrawPrimitive::TrianglesStrip,
                                ),
                        );
                    }
                }
                let native = crate::layer::user::readback_test_texture(ctx, target);
                let max = actual
                    .iter()
                    .zip(&native)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                worst = worst.max(max);
                assert!(
                    max <= 1,
                    "{version} step{step} native star pixel channel difference {max}"
                );
                if step == 0 {
                    assert!(
                        actual
                            .chunks_exact(4)
                            .filter(|p| p[..3]
                                != [
                                    (background.0 & 255) as u8,
                                    ((background.0 >> 8) & 255) as u8,
                                    ((background.0 >> 16) & 255) as u8
                                ])
                            .count()
                            > 100
                    );
                }
                compared += target.width() as usize * target.height() as usize;
            }
        }
        eprintln!("{version}:star packet replay {compared} pixels, max channel difference {worst}");
    }
    std::fs::remove_file(binary).unwrap();
}
