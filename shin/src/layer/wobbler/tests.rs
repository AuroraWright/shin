use super::*;

#[test]
fn period_changes_freeze_reverse_and_mode_changes_restart_native_phase() {
    let mut w = Wobbler::with_seed(123);
    let step = |w: &mut Wobbler, delta, mode, period| {
        w.update(Ticks::from_f32(delta), mode, Ticks::from_f32(period));
    };
    step(&mut w, 2.0, 7.0, 10.0);
    assert_eq!(w.value(), 0.2);
    step(&mut w, 2.0, 7.0, 20.0);
    assert_eq!(w.value(), 0.3); // preserve phase rather than resetting to .1
    step(&mut w, 50.0, 7.0, 0.0);
    assert!(w.active());
    assert_eq!(w.value_opt(), Some(0.3));
    step(&mut w, 2.0, 7.0, -20.0);
    assert!((w.value() - 0.2).abs() < 1e-7);
    let mut copy = w.clone();
    step(&mut copy, 1.0, 8.0, 10.0);
    assert_eq!(copy.value(), 0.9);
    assert!((w.value() - 0.2).abs() < 1e-7);
    step(&mut w, 1.0, -1.0, 10.0);
    assert_eq!(w.value_opt(), None);
    step(&mut w, 1.0, 7.0, 10.0);
    assert_eq!(w.value(), 0.1);
    step(&mut w, -2.0, 7.0, 10.0);
    assert_eq!(w.time, 0.9);
    step(&mut w, -10.0, 7.0, 10.0);
    assert!((w.time - 0.9).abs() < 1e-7);
    // Reciprocal/multiply and modff wrap, not floating remainder by1000.
    step(&mut w, 10001.0, 7.0, 10.0);
    assert!((w.time - 1.0).abs() < 0.0002);
}

#[test]
fn seeded_random_motion_matches_full_mt_initialization_with_wrapping_seeds() {
    // Independent full624-word MT initialization/twist rather than the optimized
    // production397-word prefix. Native uses the first output of each fresh seed.
    let sample = |seed: u32| {
        let mut mt = [0u32; 624];
        mt[0] = if seed == 0 { 0x12345678 } else { seed };
        for i in 1..624 {
            mt[i] = 1812433253u32
                .wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        let joined = (mt[0] & 0x80000000) | (mt[1] & 0x7fffffff);
        let mut n = mt[397] ^ (joined >> 1);
        if joined & 1 != 0 {
            n ^= 0x9908b0df;
        }
        n ^= n >> 11;
        n ^= (n << 7) & 0x9d2c5680;
        n ^= (n << 15) & 0xefc60000;
        n ^= n >> 18;
        let x = n as f32 * 2.0f32.powi(-32);
        x + x - 1.0
    };
    for seed in [0, 1, 3499211612, i32::MAX as u32, u32::MAX] {
        let mut w = Wobbler::with_seed(seed);
        w.update(Ticks::from_f32(1.25), 1.0, Ticks::from_f32(1.0));
        let from = sample(seed.wrapping_add(1));
        let to = sample(seed.wrapping_add(2));
        assert_eq!(w.value(), from + 0.25 * (to - from));
        let copy = w.clone();
        assert_eq!(copy.value(), w.value());
        assert_eq!(copy.seed, seed as i32);
    }
    let mut a = Wobbler::with_seed(1);
    let mut b = Wobbler::with_seed(2);
    a.update(Ticks::from_f32(2.0), 1.0, Ticks::from_f32(1.0));
    b.update(Ticks::from_f32(2.0), 1.0, Ticks::from_f32(1.0));
    assert_ne!(a.value(), b.value());
}

#[test]
#[ignore = "requires supplied decompressed executable"]
fn reference_native_wobbler_seeding_phase_and_float_arithmetic() {
    let root = std::env::var("SHIN_REFERENCE_ROM").expect("SHIN_REFERENCE_ROM");
    let executable =
        std::fs::read(std::path::Path::new(&root).with_file_name("uncompressed_executable"))
            .unwrap();
    let word = |address: usize| {
        u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap())
    };
    for (address, expected) in [
        (0xc72d0, 0x9400d1fc),
        (0xc72d8, 0x29007e60),
        (0xc7324, 0x7100203f),
        (0xc7328, 0x1a8183e8),
        (0xc7338, 0x2900fe68),
        (0xc733c, 0x1e202008),
        (0xc7340, 0x54000380),
        (0xc734c, 0x1e201900),
        (0xc7358, 0x1e210800),
        (0xc7360, 0x9401573c),
        (0xc737c, 0x1e210800),
        (0xc7384, 0x94015733),
        (0xc73a4, 0x1e210803),
        (0xc73ac, 0xbd000a60),
        (0xc7308, 0x1a9f07e0),
        (0xc7464, 0x2b13010b),
        (0xc7530, 0x1e2301e1),
        (0xc754c, 0x1e220821),
        (0xc7550, 0x1e212821),
        (0xc7554, 0x1e232821),
        (0x73858, 0x94014e9a),
        (0x73868, 0x94014e96),
        (0x73878, 0x94014e92),
        (0x73888, 0x94014e8e),
        (0x73898, 0x94014e8a),
        (0x738a8, 0x94014e86),
        (0x738b0, 0x94022084),
        (0x73ec4, 0x94014cff),
        (0x73ed8, 0x94014cfa),
        (0x73eec, 0x94014cf5),
        (0x73f00, 0x94014cf0),
        (0x73f14, 0x94014ceb),
        (0x73f24, 0x94014ce7),
        (0x74008, 0xa9002548),
        (0x74018, 0xa9002548),
        (0x74028, 0xa9002548),
        (0x74038, 0xa9002548),
        (0x74044, 0xa9002728),
        (0x74050, 0xa9002748),
    ] {
        assert_eq!(
            word(address),
            expected,
            "Native wobble instruction {address:x}"
        );
    }
    for (address, bits) in [
        (0x159a58, 0x3a83126f),
        (0x159578, 0x447a0000),
        (0x159a00, 0x2f800000),
        (0x1593bc, 0x40490fdb),
    ] {
        assert_eq!(word(address), bits);
    }
    assert_eq!(
        &executable[0x17266c + 256..0x172674 + 256],
        &[0, 110, 123, 102, 129, 135, 150, 142]
    );
}
