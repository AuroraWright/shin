use super::*;

#[test]
fn six_modes_match_independent_float32_warmup_reference() {
    // Independently calculated with modulo Park-Miller arithmetic and rounded
    // float32 at each native FMUL/FADD/FDIV. No render during500-tick warmup.
    let first = [
        [0x3fee763e, 0x444579c1, 0x3e153f2c, 0xc0a43cf6, 0x43d40000],
        [0x400f3b1f, 0x446d39bf, 0x3e4db85a, 0xc0a43cf6, 0x43d40000],
        [0x3fee763e, 0x444579c1, 0x3e953f2c, 0xc0a43cf6, 0x43d40000],
        [0x400f3b1f, 0x446d39bf, 0x3ecdb85a, 0xc0a43cf6, 0x43d40000],
        [0x3ff18f30, 0x44416fd0, 0x3e4e5562, 0xc09f7e06, 0x43cd0000],
        [0x3ff18f30, 0x44416fd0, 0x3ec12670, 0xc09f7e06, 0x43cd0000],
    ];
    for mode in 0..6 {
        let model = SakuraParticles::new(mode, 1).unwrap();
        assert_eq!(model.elapsed, 500.0);
        assert_eq!(model.particles.len(), 4);
        assert_eq!(model.seed, 1_479_919_876);
        assert_eq!(
            model.emission.to_bits(),
            if mode < 4 { 0x42e76460 } else { 0x43030335 }
        );
        let p = &model.particles[0];
        assert!(p.alive);
        assert_eq!(p.variant, 0);
        assert_eq!(p.origin.x.to_bits(), 0x442c801f);
        assert_eq!(p.origin.y, -10.0);
        assert_eq!(p.travel_angle.to_bits(), 0x3f8e843f);
        assert_eq!(p.spin_speed.to_bits(), 0xbc2d9021);
        assert_eq!(
            [p.travel_speed, p.radius, p.scale, p.spin_angle, p.age].map(f32::to_bits),
            first[mode as usize],
        );
        assert_eq!(
            model.particles.last().unwrap().age,
            if mode < 4 { 15.0 } else { 1.0 }
        );
    }
    for mode in [-1, 6, i32::MIN, i32::MAX] {
        assert!(SakuraParticles::new(mode, 1).is_none());
    }
    assert_eq!(
        SakuraParticles::new(0, 0),
        SakuraParticles::new(0, 2_147_483_647)
    );
    assert_eq!(
        SakuraParticles::new(0, 1),
        SakuraParticles::new(0, 2_147_483_648)
    );
}

#[test]
fn signed_motion_full_delta_births_permission_and_clone() {
    let mut model = SakuraParticles::new(4, 1).unwrap();
    let old = model.clone();
    let mut copy = old.clone();
    model.update(1000.0, false);
    copy.update(1000.0, false);
    assert_eq!(
        model, copy,
        "Clone preserves emission/RNG and each particle"
    );
    assert_eq!(model.elapsed, old.elapsed, "Permission gates only elapsed");
    assert!(model.particles.len() > old.particles.len() + 5);
    for p in &model.particles[old.particles.len()..] {
        assert_eq!(
            p.age, 1000.0,
            "Births get full delta, not residual emission time"
        );
        assert_eq!(p.radius, p.travel_speed * 1000.0);
        assert_eq!(p.origin.y, -10.0);
    }
    let count = model.particles.len();
    let before = model.particles[0].clone();
    model.update(-2.0, true);
    assert_eq!(model.elapsed, 498.0);
    assert_eq!(model.particles.len(), count);
    assert_eq!(
        model.particles[0].radius,
        before.radius + before.travel_speed * -2.0
    );
    assert_eq!(model.particles[0].age, before.age - 2.0);
    assert_eq!(model.particles[0].origin, before.origin);
}

#[test]
fn render_projection_expires_before_next_update_and_moves_radially() {
    let mut model = SakuraParticles::new(0, 1).unwrap();
    model.particles.truncate(1);
    let p = &mut model.particles[0];
    p.origin = Vec2::new(10.0, 20.0);
    p.radius = 3.0;
    p.travel_angle = std::f32::consts::FRAC_PI_2;
    p.spin_angle = 0.0;
    p.scale = 2.0;
    let origin = p.transform(Mat4::IDENTITY).transform_point3(Vec3::ZERO);
    assert!((origin - Vec3::new(10.0, 23.0, 0.0)).length() < 0.00001);
    let mut draws = 0;
    model.render_particles(Mat4::IDENTITY, |_, _| draws += 1);
    assert_eq!(draws, 1);
    let unchanged = model.clone();
    // Parent transform participates in expiry, including its translation.
    model.render_particles(
        Mat4::from_translation(Vec3::new(0.0, -25.0, 0.0)),
        |_, _| panic!("Expired particle must not draw"),
    );
    assert_eq!(model.particles.len(), 1);
    assert!(!model.particles[0].alive);
    assert!(
        unchanged.particles[0].alive,
        "Render-clone must not share flags"
    );
    model.render_particles(Mat4::IDENTITY, |_, _| panic!("Dead particle must not draw"));
    model.update(0.0, false);
    assert!(
        model.particles.is_empty(),
        "Removal belongs to subsequent update"
    );

    let mut model = unchanged;
    model.particles[0].origin = Vec2::new(0.0, -1.1);
    model.particles[0].radius = 0.0;
    model.render_particles(Mat4::IDENTITY, |_, _| {});
    assert!(
        model.particles[0].alive,
        "Expiry is strictly less than -1.1"
    );
}

#[test]
fn frame_durations_boundaries_and_signed_age() {
    let mut p = SakuraParticles::new(0, 1).unwrap().particles.remove(0);
    for variant in 0..2 {
        p.variant = variant;
        let total = DURATIONS[variant].iter().sum::<i32>();
        let mut start = 0;
        for (i, duration) in DURATIONS[variant].into_iter().enumerate() {
            for age in [
                start as f32,
                (start + duration) as f32 - 0.01,
                (start + total) as f32,
            ] {
                p.age = age;
                assert_eq!(p.sprite(), &SPRITES[variant * 12 + i]);
            }
            start += duration;
        }
        for age in [-0.9, -1.0, -(total as f32) - 1.0, total as f32] {
            p.age = age;
            assert_eq!(p.sprite(), &SPRITES[variant * 12]);
        }
    }
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU/audio"]
fn reference_native_sakura_parameters_sprite_tables_and_lifecycle() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    use std::io::Read;
    let mut reader = shin_core::format::rom::RomReader::new(
        shin_core::primitives::stateless_reader::StatelessFile::new(
            std::fs::File::open(&rom).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let archive = reader.find_file("/effects.txa").unwrap();
    let mut bytes = Vec::new();
    reader
        .open_file(archive)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let archive = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
    assert_eq!(archive.name_to_index.len(), 6);
    assert!(archive.get_texture("Sakura").is_none());
    assert!(archive.get_texture("hanabira").is_some());
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
        .step_by(24)
        .map(|offset| {
            (
                u64::from_le_bytes(exe[offset + 256..offset + 264].try_into().unwrap()),
                u64::from_le_bytes(exe[offset + 272..offset + 280].try_into().unwrap()),
            )
        })
        .collect();
    for (address, expected) in [
        (0x2247c0, 0x1d3740),
        (0x2247c8, 0x1d3830),
        (0x2247d0, 0x1d3860),
        (0x1d3760, 0x6aa40),
        (0x1d3768, 0x25760),
        (0x1d3778, 0x25e30),
        (0x1d3780, 0x75d90),
        (0x1d37d8, 0x25f10),
        (0x2247e8, 0x1d3708),
        (0x1d82f0, 0x84820),
    ] {
        assert_eq!(relocations.get(&address), Some(&expected));
    }
    for mode in 0..6 {
        for (i, value) in PERIODS[mode].into_iter().enumerate() {
            assert_eq!(word(0x1d3830 + mode * 8 + i * 4), value.to_bits());
        }
        for (i, value) in PARAMETERS[mode].into_iter().enumerate() {
            assert_eq!(word(0x1d3860 + mode * 32 + i * 4), value.to_bits());
        }
    }
    assert_eq!(word(0x159880), PI.to_bits());
    assert_eq!(word(0x159598), (PI * 2.0).to_bits());
    let texture_pointer = relocations[&0x1d2ec0] as usize;
    assert_eq!(
        &exe[texture_pointer + 256..texture_pointer + 263],
        b"Sakura\0"
    );
    assert_eq!(relocations[&0x2247f0], 0x227208);
    assert_eq!(&exe[0x161346 + 256..0x161346 + 269], b"/effects.txa\0");
    for (i, sprite) in SPRITES.iter().enumerate() {
        let tile = 0x1d2ec8 + i * 40;
        assert_eq!(relocations[&(tile as u64)], 0x1d2ec0);
        for j in 0..4 {
            assert_eq!(word(tile + 8 + j * 4), sprite.uv[j].to_bits());
            assert_eq!(word(tile + 24 + j * 4) as i32, sprite.bounds[j]);
        }
        let variant = i / 12;
        let frame = i % 12;
        let frame_address = [0x1d3588, 0x1d3648][variant] + frame * 16;
        let descriptor = [0x1d3288, 0x1d3408][variant] + frame * 32;
        assert_eq!(word(frame_address) as i32, DURATIONS[variant][frame]);
        assert_eq!(
            relocations[&((frame_address + 8) as u64)],
            descriptor as u64
        );
        assert_eq!(word(descriptor + 16), 1);
        assert_eq!(relocations[&((descriptor + 24) as u64)], tile as u64);
    }
    for (a, expected) in [
        (0x25358, 0x7100183f),
        (0x25360, 0x540006e2),
        (0x2556c, 0x94014239),
        (0x2559c, 0x94035949),
        (0x25664, 0xd63f0100),
        (0x2566c, 0x7107d2df),
        (0x257a0, 0x94014c20),
        (0x257a4, 0x36000094),
        (0x257c0, 0x1e283800),
        (0x25870, 0xbd401921),
        (0x2587c, 0xbd001920),
        (0x2588c, 0xbd002120),
        (0x25898, 0xbd002920),
        (0x25a64, 0x1200010c),
        (0x25ad4, 0x1e210803),
        (0x25c30, 0xb900191f),
        (0x25de8, 0xbd006a60),
        (0x25dec, 0x54ffe1a9),
        (0x25e58, 0x94014066),
        (0x25e8c, 0xb94062a8),
        (0x25ea0, 0xb9006a68),
        (0x26074, 0xbd401ac0),
        (0x260b0, 0xbd4016c0),
        (0x26184, 0x540000c5),
        (0x26188, 0x390002df),
        (0x261bc, 0x1e38000a),
        (0x261f0, 0x1ac90d4b),
        (0x261f4, 0x1b09a96b),
        (0x65f40, 0x900007e8),
        (0x65f44, 0x910d1908),
        (0x65f74, 0x9402c483),
        (0x65f80, 0xf943f908),
        (0x65f84, 0xc85ffd13),
        (0x65f88, 0xc809fd00),
        // Animation loader+40 returns the raw possibly-null factory result.
        // LAYERLOAD's no-wipe path eventually dereferences its drawable cast.
        (0x8489c, 0xf9000268),
        (0x9338, 0xf9402109),
        (0x9340, 0xd63f0120),
        (0x938c, 0xb4000940),
        (0x94b4, 0xf9002fff),
        (0x9740, 0xf9400008),
        (0x9744, 0xf9403108),
        (0x974c, 0xd63f0100),
        // Tile drawing uses unchecked native lookup; absence is not fallback.
        (0x2524c, 0x9403cbcd),
        (0x1181f4, 0xf9401a75),
        (0x1181f8, 0xf94002a8),
    ] {
        assert_eq!(word(a), expected, "Native instruction at{a:x}");
    }
}
