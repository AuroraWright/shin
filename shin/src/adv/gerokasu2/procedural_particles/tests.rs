use super::*;

fn prepared_particles() -> Vec<RainParticle> {
    let xs = [-200003.25, 200003.25, -0.0, 1920.0, 1918.0];
    let ys = [1.0, 1.0, 1.0, 1080.0, 200.0];
    let vx = [-7.25, 13.5, -0.0, 7.0, 20.0];
    let vy = [1.0, 1.0, 1.0, 40.0, 2.0];
    (0..5)
        .map(|i| RainParticle {
            shape: Mat4::IDENTITY,
            position: Vec2::new(xs[i], ys[i]),
            depth: 0.25,
            velocity: Vec2::new(vx[i], vy[i]),
            radius: 5.0,
        })
        .collect()
}

#[test]
fn ger_rain_rejected_samples_retry_without_consuming_single_emission() {
    let mut m = GerRainParticles::new(0, 100, 1500, 1);
    m.update(0.0, 0.0, 100.0, 1000.0);
    assert_eq!(m.particles.len(), 1);
    // Native rejects the first two two-draw samples, then accepts the third
    // and consumes its direction jitter. The single emission remains pending
    // throughout both rejected samples.
    assert_eq!(m.seed, 1_105_902_161);
    assert_eq!(m.emission, 0.0);
    assert_eq!(m.particles[0].depth.to_bits(), 0x3f77cbf2);
    assert_eq!(m.particles[0].position.x.to_bits(), 0x43b73fd2);
}

#[test]
fn ger_rain_zero_delta_moves_once_wraps_repeatedly_preserves_negative_zero_and_expires() {
    let mut m = GerRainParticles::new(-99, 20_000, -100, 0);
    assert_eq!(
        (m.mode, m.min_distance, m.max_distance, m.seed),
        (0, 100, 10000, 1)
    );
    m.emission = 0.0;
    m.expired = 7;
    m.particles = prepared_particles();
    m.update(0.0, 0.0, 0.0, 0.0);
    assert_eq!(m.expired, 8);
    assert_eq!(m.seed, 1);
    assert_eq!(m.particles.len(), 4);
    assert_eq!(m.particles[0].position, Vec2::new(1589.5, 2.0));
    assert_eq!(m.particles[1].position, Vec2::new(336.75, 2.0));
    assert_eq!(m.particles[2].position.x.to_bits(), (-0.0f32).to_bits());
    assert_eq!(m.particles[3].position, Vec2::new(18.0, 202.0));
}

#[test]
fn ger_rain_warmup_stays_pending_for_nonterminating_controls_then_completes_once() {
    let mut m = GerRainParticles::new(0, 100, 1500, 1);
    for (intensity, speed) in [(0.0, 0.0), (3000.0, 0.0), (-1.0, 1000.0)] {
        m.warm_up(intensity, 100.0, speed);
        assert!(m.needs_warm_up);
        assert!(m.particles.is_empty());
        assert_eq!((m.seed, m.emission), (1, 1.0));
    }
    m.warm_up(3000.0, 100.0, 1000.0);
    assert!(!m.needs_warm_up);
    assert!(m.expired > 0 && !m.particles.is_empty());
    let before = rain_snapshot(&m);
    m.warm_up(3000.0, 100.0, 1000.0);
    assert_eq!(before, rain_snapshot(&m));
    let mut copy = m.clone();
    m.update(0.125, 1234.125, -321.25, 999.875);
    copy.update(0.125, 1234.125, -321.25, 999.875);
    assert_eq!(rain_snapshot(&m), rain_snapshot(&copy));
    copy.particles[0].position.x = -10.0;
    assert_ne!(rain_snapshot(&m), rain_snapshot(&copy));
}

#[test]
fn ger_rain_zero_intensity_warmup_consumes_pending_emission_or_existing_velocity() {
    for mode in 0..4 {
        let mut m = GerRainParticles::new(mode, 100, 1500, 42);
        m.warm_up(0.0, 100.0, 1000.0);
        assert!(!m.needs_warm_up);
        assert_eq!(m.expired, 1);
        assert_eq!(m.emission, 0.0);
        assert!(m.particles.is_empty());
        assert_eq!(m.seed, if mode == 1 { 3_864_260 } else { 974_199_846 });

        let mut m = GerRainParticles::new(mode, 100, 1500, 42);
        m.update(0.0, 0.0, 100.0, 1000.0);
        assert_eq!(m.particles.len(), 1);
        // Current speed only affects future particles. The retained particle
        // can finish native warm-up using its original positive velocity.
        m.warm_up(0.0, 100.0, 0.0);
        assert!(!m.needs_warm_up);
        assert_eq!(m.expired, 1);

        let mut m = GerRainParticles::new(mode, 100, 1500, 42);
        m.emission = 0.5;
        m.warm_up(0.0, 100.0, 1000.0);
        assert!(m.needs_warm_up);
        assert!(m.particles.is_empty());
    }
}

#[test]
fn ger_focus_warmed_expired_rays_are_replaced_before_advance_even_at_zero_delta() {
    let mut m = GerFocusRays::new(1);
    assert_eq!(m.rays.len(), 40);
    assert_eq!(m.seed, 1427401579);
    assert_eq!(m.rays[0].angle.to_bits(), 0xc0490d80);
    assert!(m.rays.iter().any(|r| r.alpha.is_idle()));
    m.update(Ticks::from_f32(50.0));
    assert!(
        m.rays
            .iter()
            .all(|r| r.alpha.is_idle() && r.alpha.value() == 0.0)
    );
    let previous: Vec<_> = m.rays.iter().map(|r| r.angle.to_bits()).collect();
    m.update(Ticks::ZERO);
    assert!(
        m.rays
            .iter()
            .all(|r| !r.alpha.is_idle() && r.alpha.value() == 1.0)
    );
    assert_ne!(
        previous,
        m.rays.iter().map(|r| r.angle.to_bits()).collect::<Vec<_>>()
    );
    let seed = m.seed;
    m.update(Ticks::from_f32(-1.0));
    assert_eq!(seed, m.seed);
    assert!(m.rays.iter().all(|r| r.alpha.value() == 1.0));
}

fn header(words: [u32; 11]) -> Vec<u8> {
    let mut out = b"GPR1".to_vec();
    out.extend(words.into_iter().flat_map(u32::to_le_bytes));
    out
}
fn rain_snapshot(m: &GerRainParticles) -> Vec<u8> {
    let mut out = header([
        0,
        m.seed,
        0xdeadbeef,
        m.mode as u32,
        m.min_distance as u32,
        m.max_distance as u32,
        match m.mode {
            0 => 0x43160da7,
            1 => 0x4229b4a4,
            _ => 0x41a9b4a4,
        },
        m.expired,
        m.needs_warm_up as u32,
        m.emission.to_bits(),
        m.particles.len() as u32,
    ]);
    for p in &m.particles {
        out.extend(
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
                .flat_map(|v| v.to_bits().to_le_bytes()),
        );
    }
    out
}
fn focus_snapshot(m: &GerFocusRays, offset: Vec2) -> Vec<u8> {
    let mut out = header([
        1,
        m.seed,
        0xdeadbeef,
        offset.x.to_bits(),
        offset.y.to_bits(),
        0,
        0,
        0,
        0,
        0,
        m.rays.len() as u32,
    ]);
    for ray in &m.rays {
        out.extend(
            [
                ray.angle.to_bits(),
                ray.alpha.value().to_bits(),
                ray.alpha.target_value().to_bits(),
                (!ray.alpha.is_idle()) as u32,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes),
        );
    }
    out
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes original Ger Rain/Focus constructors and simulation; prepared SDK rotation; no GPU/audio"]
fn ger_native_procedural_constructors_fused_particles_warmup_and_focus_tweens() {
    use std::{
        fs,
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/procedural");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-procedural");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("reverse-engineering/gerokasu2/native-procedural.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut input = Vec::new();
    let mut expected = Vec::new();
    let mut rows = 0;
    let mut constructors = 0;
    let mut record_offsets = Vec::new();
    let mut record = |row: [u32; 8], snapshot: Vec<u8>| {
        input.extend(row.into_iter().flat_map(u32::to_le_bytes));
        record_offsets.push(expected.len());
        expected.extend(snapshot);
        rows += 1;
    };
    for mode in [-99, 0, 1, 2, 3, 99] {
        for (min, max) in [(100, 1500), (1000, 1000), (10_001, 1), (i32::MAX, i32::MIN)] {
            for seed in [0, 1, 42, 2_147_483_646, 2_147_483_647, u32::MAX] {
                let mut m = GerRainParticles::new(mode, min, max, seed);
                record(
                    [0, mode as u32, min as u32, max as u32, seed, 0, 0, 0],
                    rain_snapshot(&m),
                );
                constructors += 1;
                for (delta, intensity, direction, speed) in [
                    (0.0f32, 0.0f32, -1000.0f32, 1000.0f32),
                    (1.0, 3000.0, 100.0, 1000.0),
                    (0.125, 1234.125, -321.25, 999.875),
                    (0.1, 3999.75, 321.25, 1000.125),
                    (0.0, 999.875, 2000.0, 1000.0),
                    (-0.125, 1234.125, -2000.0, 1000.0),
                    (2.5, 1500.125, 0.0, 333.375),
                ] {
                    m.update(delta, intensity, direction, speed);
                    record(
                        [
                            1,
                            delta.to_bits(),
                            intensity.to_bits(),
                            direction.to_bits(),
                            speed.to_bits(),
                            0,
                            0,
                            0,
                        ],
                        rain_snapshot(&m),
                    );
                }
                m.emission = 0.0;
                m.expired = 7;
                m.needs_warm_up = false;
                m.particles = prepared_particles();
                record([5, 0, 0, 0, 0, 0, 0, 0], rain_snapshot(&m));
                m.update(0.0, 0.0, 0.0, 0.0);
                record([1, 0, 0, 0, 0, 0, 0, 0], rain_snapshot(&m));
            }
        }
    }
    for mode in 0..4 {
        let mut m = GerRainParticles::new(mode, 100, 1500, 42);
        record([0, mode as u32, 100, 1500, 42, 0, 0, 0], rain_snapshot(&m));
        constructors += 1;
        for (delta, animate, allowed) in [
            (0.0f32, 0.0f32, 0),
            (0.0, 0.999, 0),
            (0.0, 1.0, 0),
            (0.125, -1.0, 1),
            (2.5, 0.0, 1),
        ] {
            m.warm_up(3000.0, 100.0, 1000.0);
            if animate as i32 != 0 {
                m.update(delta, 3000.0, 100.0, 1000.0);
            }
            record(
                [
                    4,
                    delta.to_bits(),
                    3000.0f32.to_bits(),
                    100.0f32.to_bits(),
                    1000.0f32.to_bits(),
                    animate.to_bits(),
                    allowed,
                    0,
                ],
                rain_snapshot(&m),
            );
        }
    }
    // Initial emission is one, so intensity zero still warms up to one expiry.
    // This finite native case is distinct from empty zero-emission warm-up.
    for mode in 0..4 {
        let mut m = GerRainParticles::new(mode, 100, 1500, 42);
        record([0, mode as u32, 100, 1500, 42, 0, 0, 0], rain_snapshot(&m));
        constructors += 1;
        m.warm_up(0.0, 100.0, 1000.0);
        record(
            [4, 0, 0, 100.0f32.to_bits(), 1000.0f32.to_bits(), 0, 0, 0],
            rain_snapshot(&m),
        );
    }
    for seed in [0, 1, 42, 2_147_483_646, 2_147_483_647, u32::MAX] {
        let offset = Vec2::new(i32::MIN as f32, i32::MAX as f32);
        let mut m = GerFocusRays::new(seed);
        record(
            [2, seed, offset.x.to_bits(), offset.y.to_bits(), 0, 0, 0, 0],
            focus_snapshot(&m, offset),
        );
        constructors += 1;
        for (delta, allowed) in [
            (0.0f32, 0),
            (-1.0, 1),
            (0.125, 0),
            (5.1, 0),
            (30.0, 1),
            (0.0, 0),
            (50.0, 1),
            (0.0, 0),
        ] {
            m.update(Ticks::from_f32(delta));
            record(
                [3, delta.to_bits(), allowed, 0, 0, 0, 0, 0],
                focus_snapshot(&m, offset),
            );
        }
    }
    drop(record);
    // clang can merge sinf/cosf into Darwin's paired entrypoint, while an
    // unoptimized Rust host uses scalar calls. They differ for this angle.
    // Select the actual host geometry contract; SDK trig remains excluded.
    let (sine, cosine) = std::hint::black_box(f32::from_bits(0x3de9a0cc)).sin_cos();
    assert_eq!(cosine.to_bits(), 0x3f7e560a, "unreviewed host cosine");
    let rotation_profile = match sine.to_bits() {
        0x3de91f2a => "scalar",
        0x3de91f29 => "paired",
        value => panic!("unreviewed host sine {value:08x}"),
    };
    let run = |mutant: Option<&str>| {
        let mut command = Command::new(&binary);
        command
            .env("SHIN_GER_PROCEDURAL_ROTATION", rotation_profile)
            .args([
                root.join("game-data/gerokasu2/uncompressed_executable"),
                root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
                root.join("test-fixtures/gerokasu2/system.fnt"),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(mutant) = mutant {
            command.env("SHIN_GER_PROCEDURAL_MUTANT", mutant);
        }
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let bytes = input.clone();
        let writer = std::thread::spawn(move || stdin.write_all(&bytes));
        let output = child.wait_with_output().unwrap();
        let written = writer.join().unwrap();
        if mutant.is_none() {
            written.unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        output
    };
    let output = run(None);
    fs::write(folder.join("native.log"), &output.stderr).unwrap();
    fs::write(folder.join("comparison-input.bin"), &input).unwrap();
    fs::write(folder.join("comparison-native.bin"), &output.stdout).unwrap();
    fs::write(folder.join("comparison-rust.bin"), &expected).unwrap();
    if output.stdout != expected {
        let at = output
            .stdout
            .iter()
            .zip(&expected)
            .position(|(a, b)| a != b)
            .unwrap_or(output.stdout.len().min(expected.len()));
        let row = record_offsets.partition_point(|offset| *offset <= at) - 1;
        let offset = at - record_offsets[row];
        let request = |row: usize| {
            input[row * 32..(row + 1) * 32]
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                .collect::<Vec<_>>()
        };
        let constructor = (0..=row)
            .rev()
            .find(|row| matches!(request(*row)[0], 0 | 2))
            .unwrap();
        let kind = request(constructor)[0];
        let stride = if kind == 2 { 16 } else { 88 };
        let field = if offset < 48 {
            format!("header word {}", offset / 4)
        } else {
            format!(
                "record {} word {}",
                (offset - 48) / stride,
                ((offset - 48) % stride) / 4
            )
        };
        panic!(
            "Ger procedural native mismatch byte {at}, row {row}, {field}, request {:08x?}, constructor row {constructor} {:08x?}, rotation {rotation_profile}, native bytes{}, port bytes{}; native{:?}, port{:?}",
            request(row),
            request(constructor),
            output.stdout.len(),
            expected.len(),
            output
                .stdout
                .get(at.saturating_sub(4)..(at + 8).min(output.stdout.len())),
            expected.get(at.saturating_sub(4)..(at + 8).min(expected.len()))
        );
    }
    let mut mutants = Vec::new();
    for mutant in [
        "rain_emission_fma",
        "rain_distance_fma",
        "rain_jitter_fma",
        "rain_position_fma",
        "focus_angle_fma",
        "focus_duration_fma",
        "focus_warmup",
    ] {
        let mutated = run(Some(mutant));
        assert!(
            mutated.status.success(),
            "procedural mutant {mutant} failed instead of completing: {}",
            String::from_utf8_lossy(&mutated.stderr)
        );
        assert!(
            mutated.stdout != expected,
            "undetected procedural mutant {mutant}"
        );
        mutants.push(serde_json::json!({
            "name": mutant,
            "exit_code": mutated.status.code(),
            "changed": true,
            "bytes": mutated.stdout.len(),
        }));
    }
    fs::write(
        folder.join("comparison-summary.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "rows": rows,
            "constructors": constructors,
            "bytes": expected.len(),
            "equal": true,
            "host_rotation": rotation_profile,
            "mutants": mutants,
            "sdk_trigonometry_verified": false,
            "gpu_audio_executed": false,
        }))
        .unwrap(),
    )
    .unwrap();
    eprintln!(
        "GEROKASU2_PROCEDURAL rows={rows} constructors={constructors} bytes={} mutants=7 original_native_simulation_and_tweens=verified sdk_rotation_properties_seed_draws=prepared host_rotation={rotation_profile}",
        expected.len()
    );
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes original CE6E0 with guarded SDK mutex; no GPU/audio"]
fn ger_native_engine_mt_matches_shared_stream_across_four_wraps() {
    use std::{
        fs,
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/procedural");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-procedural-prng");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("reverse-engineering/gerokasu2/native-procedural-prng.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut input = Vec::new();
    let mut expected = Vec::new();
    for seed in [0, 1, 42, 5489, 2_147_483_647, u32::MAX] {
        input.extend([seed, 2496].into_iter().flat_map(u32::to_le_bytes));
        let mut stream = crate::engine_random::EngineRandom::from_seed(seed);
        for _ in 0..2496 {
            expected.extend(stream.next().to_le_bytes());
        }
    }
    let mut child = Command::new(&binary)
        .args([
            root.join("game-data/gerokasu2/uncompressed_executable"),
            root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
            root.join("test-fixtures/gerokasu2/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(folder.join("prng-native.log"), &output.stderr).unwrap();
    assert_eq!(output.stdout, expected);
    eprintln!(
        "GEROKASU2_PROCEDURAL_PRNG streams=6 values=14976 bytes=59904 wraps_per_stream=4 original_ce6e0_shared_engine_random=verified sdk_mutex=guarded"
    );
}
