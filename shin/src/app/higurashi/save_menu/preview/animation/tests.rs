use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

mod pixels;

#[derive(Clone)]
struct Case {
    kind: i32,
    seed: u32,
    profile: u32,
    params: [i32; 8],
}
#[derive(Debug)]
struct Snapshot {
    kind: i32,
    header: Vec<u32>,
    rows: Vec<Vec<u32>>,
}
#[derive(Debug)]
struct Draw {
    program: u32,
    blend: u32,
    texture: u32,
    matrix: [u32; 16],
    color: [u32; 4],
    param: [u32; 4],
    vertices: [u32; 16],
}
struct Native {
    present: bool,
    os: u32,
    mt: u32,
    states: Vec<Snapshot>,
    draws: Vec<Draw>,
    scopes: [u32; 3],
}
fn word(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
    w.next().unwrap().parse().unwrap()
}
fn words<const N: usize>(w: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(w))
}
fn state(w: &mut std::str::SplitWhitespace<'_>) -> Snapshot {
    let label = w.next().unwrap();
    let (kind, header_len, stride) = match label {
        "S" => (4, 5, 11),
        "F" => (8, 4, 2),
        "R" => (9, 9, 22),
        _ => panic!("Native animation state:{label}"),
    };
    let header: Vec<_> = (0..header_len).map(|_| word(w)).collect();
    let rows = (0..*header.last().unwrap())
        .map(|_| (0..stride).map(|_| word(w)).collect())
        .collect();
    Snapshot { kind, header, rows }
}
fn parse(line: &str) -> Native {
    let mut w = line.split_whitespace();
    assert_eq!(w.next(), Some("C"));
    let kind = word(&mut w) as i32;
    let present = word(&mut w) != 0;
    let os = word(&mut w);
    let mt = word(&mut w);
    assert_eq!(word(&mut w), 0, "Original factories have no atlas tickets");
    let mut native = Native {
        present,
        os,
        mt,
        states: vec![],
        draws: vec![],
        scopes: [0; 3],
    };
    if !present {
        assert!(w.next().is_none());
        return native;
    }
    assert_eq!(w.next(), Some("V"));
    let refresh = word(&mut w);
    let finish = word(&mut w);
    let render = word(&mut w);
    assert_eq!(refresh, 0x254bc, "Actual virtual40 isRET");
    assert_eq!(
        (finish, render),
        match kind {
            4 => (0x262ec, 0x25ce4),
            8 => (0, 0x5b390),
            9 => (0xa1804, 0xa21a8),
            _ => unreachable!(),
        }
    );
    native.states.push(state(&mut w));
    native.states.push(state(&mut w));
    assert_eq!(w.next(), Some("A"));
    assert_eq!(word(&mut w), u32::from(kind != 8));
    native.states.push(state(&mut w));
    assert_eq!(w.next(), Some("D"));
    loop {
        match w.next().unwrap() {
            "P" => native.draws.push(Draw {
                program: word(&mut w),
                blend: word(&mut w),
                texture: word(&mut w),
                matrix: words(&mut w),
                color: words(&mut w),
                param: words(&mut w),
                vertices: words(&mut w),
            }),
            "E" => {
                native.scopes = words(&mut w);
                break;
            }
            token => panic!("Native animation event:{token}"),
        }
    }
    native.states.push(state(&mut w));
    assert!(w.next().is_none());
    native
}
fn snapshot(view: &View) -> Snapshot {
    match &view.state {
        State::Sakura(model) => {
            let model = model.lock();
            Snapshot {
                kind: 4,
                header: vec![
                    model.elapsed.to_bits(),
                    model.reference_seed(),
                    model.mode as u32,
                    model.reference_owner_emission().to_bits(),
                    model.particles.len() as u32,
                ],
                rows: model
                    .particles
                    .iter()
                    .map(|p| {
                        vec![
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
                        ]
                    })
                    .collect(),
            }
        }
        State::Focus { rays, offset } => Snapshot {
            kind: 8,
            header: vec![
                rays.reference_seed(),
                offset.x.to_bits(),
                offset.y.to_bits(),
                rays.rays.len() as u32,
            ],
            rows: rays
                .rays
                .iter()
                .map(|ray| vec![ray.angle.to_bits(), ray.alpha.value().to_bits()])
                .collect(),
        },
        State::Rain(model) => {
            let model = model.lock();
            let (_, owner) = model.reference_owner_words();
            Snapshot {
                kind: 9,
                header: vec![
                    0,
                    model.reference_seed(),
                    model.mode as u32,
                    model.min_distance as u32,
                    model.max_distance as u32,
                    u32::from_str_radix(&owner[4], 16).unwrap(),
                    model.expired,
                    u32::from_str_radix(&owner[7], 16).unwrap(),
                    model.particles.len() as u32,
                ],
                rows: model
                    .particles
                    .iter()
                    .map(|p| {
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
                            .map(f32::to_bits)
                            .collect()
                    })
                    .collect(),
            }
        }
    }
}
fn float(actual: u32, expected: u32, context: &str) {
    let a = f32::from_bits(actual);
    let e = f32::from_bits(expected);
    assert!(
        a == e || (a - e).abs() <= 0.00002 * e.abs().max(1.),
        "{context}:Rust{a}({actual:x}) native{e}({expected:x})"
    );
}
fn equal_state(actual: &Snapshot, expected: &Snapshot, context: &str) {
    assert_eq!(actual.kind, expected.kind);
    assert_eq!(actual.header.len(), expected.header.len());
    assert_eq!(
        actual.rows.len(),
        expected.rows.len(),
        "{context}:particle/ray count"
    );
    for (i, (&a, &e)) in actual.header.iter().zip(&expected.header).enumerate() {
        let is_float = match actual.kind {
            4 => matches!(i, 0 | 3),
            8 => matches!(i, 1 | 2),
            9 => matches!(i, 0 | 5 | 7),
            _ => false,
        };
        if is_float {
            float(a, e, context);
        } else {
            assert_eq!(a, e, "{context}:nativeheader{i}");
        }
    }
    for (row, (a, e)) in actual.rows.iter().zip(&expected.rows).enumerate() {
        assert_eq!(a.len(), e.len());
        for (index, (&a, &e)) in a.iter().zip(e).enumerate() {
            if actual.kind == 4 && index < 2 {
                assert_eq!(a, e, "{context}:Sakura{row}/{index}");
            } else {
                float(a, e, context);
            }
        }
    }
}
fn native_blend(blend: ColorBlendType) -> u32 {
    match blend {
        ColorBlendType::Opaque => 1,
        ColorBlendType::Layer1 => 2,
        ColorBlendType::Layer2 => 3,
        // The renderer enum is8; the original program's blend value is4.
        ColorBlendType::HigurashiLayer3 => 4,
        value => panic!("Unexpected native saved animation blend {value:?}"),
    }
}
fn equal_draw(packet: &Packet, native: &Draw, context: &str) {
    assert_eq!(
        native.program, 6,
        "These focused profiles use the default texture shader"
    );
    assert_eq!(native.blend, native_blend(packet.blend), "{context}:blend");
    assert_eq!(
        packet.texture,
        [
            "Sakura",
            "focusline",
            "rain",
            "hanabira",
            "snow",
            "goldsnow"
        ][native.texture as usize]
    );
    for (a, e) in packet
        .matrix
        .to_cols_array()
        .map(f32::to_bits)
        .into_iter()
        .zip(native.matrix)
    {
        float(a, e, context);
    }
    for (a, e) in packet
        .color
        .into_vec4()
        .to_array()
        .map(f32::to_bits)
        .into_iter()
        .zip(native.color)
    {
        float(a, e, context);
    }
    for (a, e) in packet
        .shader_param
        .to_array()
        .map(f32::to_bits)
        .into_iter()
        .zip(native.param)
    {
        float(a, e, context);
    }
    let [x0, y0, x1, y1] = packet.rect.to_array();
    let [u0, v0, u1, v1] = packet.uv.to_array();
    let vertices = [
        x0, y0, u0, v0, x1, y0, u1, v0, x0, y1, u0, v1, x1, y1, u1, v1,
    ];
    for (a, e) in vertices.map(f32::to_bits).into_iter().zip(native.vertices) {
        float(a, e, context);
    }
}
fn cases() -> Vec<Case> {
    let mut descriptors = vec![];
    for mode in [-1, 0, 1, 2, 3, 4, 5, 6] {
        descriptors.push((4, [mode, 0, 0, 0, 0, 0, 0, 0]));
    }
    for (x, y) in [(0, 0), (640, 360), (-120, 721)] {
        descriptors.push((8, [x, y, 0, 0, 0, 0, 0, 0]));
    }
    for (mode, min, max) in [
        (0, 100, 1500),
        (1, 900, 100),
        (2, 0, 50000),
        (3, 100, 100),
        (-1, 1500, 100),
    ] {
        descriptors.push((9, [mode, min, max, 0, 0, 0, 0, 0]));
    }
    let mut cases = vec![];
    for seed in [0, 1, u32::MAX] {
        for &(kind, params) in &descriptors {
            for profile in 0..4 {
                cases.push(Case {
                    kind,
                    seed,
                    profile,
                    params,
                });
            }
        }
    }
    cases
}
fn native(cases: &[Case]) -> Vec<Native> {
    let mut input = String::new();
    for c in cases {
        write!(input, "D {} {} {}", c.kind, c.seed, c.profile).unwrap();
        for p in c.params {
            write!(input, " {p}").unwrap();
        }
        input.push('\n');
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_PREVIEW_ANIMATION").unwrap())
            .args([
                std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
                std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let write_input = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(write_input.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_ANIMATION_CAPTURE") {
        std::fs::create_dir_all(&path).unwrap();
        let path = std::path::Path::new(&path);
        std::fs::write(path.join("input.txt"), input).unwrap();
        std::fs::write(path.join("output.txt"), &result.stdout).unwrap();
        std::fs::write(path.join("stderr.txt"), &result.stderr).unwrap();
    }
    let result = String::from_utf8(result.stdout).unwrap();
    assert_eq!(result.lines().count(), cases.len());
    result.lines().map(parse).collect()
}

#[test]
fn higurashi_saved_animation_nullable_clone_and_separate_mt_order() {
    for mode in [-1, 6, i32::MAX] {
        let p = Prepared {
            kind: 4,
            params: [mode, 0, 0, 0, 0, 0, 0, 0],
            atlas: None,
        };
        assert!(!p.clone_present());
        crate::engine_random::with_reference_seed(5489, || {
            let before = crate::engine_random::seed_call_count();
            assert!(p.clone_refresh().unwrap().is_none());
            assert_eq!(crate::engine_random::seed_call_count(), before);
            assert_eq!(crate::engine_random::next_seed(), 3499211612);
        });
    }
    for kind in [4, 8, 9] {
        let p = Prepared {
            kind,
            params: [0, 100, 1500, 0, 0, 0, 0, 0],
            atlas: None,
        };
        assert!(p.clone_present());
        crate::engine_random::with_reference_seed(5489, || {
            let before = crate::engine_random::seed_call_count();
            let view = p.clone_refresh().unwrap().unwrap();
            assert_eq!(crate::engine_random::seed_call_count(), before + 1);
            let expected = View {
                state: State::new(kind, &p.params, 3499211612).unwrap(),
                atlas: None,
            };
            equal_state(&snapshot(&view), &snapshot(&expected), "separateMT");
            assert_eq!(crate::engine_random::next_seed(), 581869302);
        });
    }
}

#[test]
fn higurashi_saved_rain_keeps_identity_shader_operations() {
    // Frozen actual80-profile capture: Rain uses9/7/11 even when the same
    // identities resolve6 in Focus. Sakura's actual25d98 also calls69894.
    for operation in [1, 2, 5] {
        let mut properties = HigurashiProperties::from_seeds([1, 2, 3, 4]);
        properties
            .tweener_mut(21)
            .unwrap()
            .fast_forward_to(operation as f32);
        properties.tweener_mut(26).unwrap().fast_forward_to(0.);
        let expected = match operation {
            1 => LayerFragmentShader::Mono,
            2 => LayerFragmentShader::Fill,
            _ => LayerFragmentShader::Gamma,
        };
        for kind in [4, 8, 9] {
            let params = if kind == 8 {
                [640, 360, 0, 0, 0, 0, 0, 0]
            } else {
                [0, 100, 1500, 0, 0, 0, 0, 0]
            };
            let view = View {
                state: State::new(kind, &params, 1).unwrap(),
                atlas: None,
            };
            let packets = view
                .packets(
                    &properties,
                    &HigurashiTransform::default(),
                    PassKind::Transparent,
                )
                .unwrap();
            assert!(!packets.is_empty());
            assert!(
                packets.iter().all(|packet| packet.shader
                    == if kind == 9 {
                        expected
                    } else {
                        LayerFragmentShader::Default
                    }),
                "Original kind{kind}/operation{operation} shader identity policy"
            );
        }
    }
}

#[test]
#[ignore = "actual updated20df0/6717c/derivedMT/constructor/virtual40/virtual58/render CPU oracle;bounded atlas and clip/stencil/GPU packet transports,no GPU/audio/windows/user saves"]
fn higurashi_saved_animation_warmed_state_and_packets_match_original() {
    let cases = cases();
    let outputs = native(&cases);
    let mut draws = 0usize;
    let mut owners = 0usize;
    for (i, (c, n)) in cases.iter().zip(outputs).enumerate() {
        let context = format!(
            "profile{i}:kind{} seed{} controls{}",
            c.kind, c.seed, c.profile
        );
        let prepared = Prepared {
            kind: c.kind,
            params: c.params,
            atlas: None,
        };
        assert_eq!(prepared.clone_present(), n.present, "{context}");
        assert_eq!(
            (n.os, n.mt),
            if n.present { (4, 1) } else { (0, 0) },
            "{context}"
        );
        if !n.present {
            continue;
        }
        owners += 1;
        let view = View {
            state: State::new(c.kind, &c.params, c.seed).unwrap(),
            atlas: None,
        };
        let initial = snapshot(&view);
        // Actual virtual40 isRET. Sakura58 isRET; Rain58 finds ctor's expiry>0.
        for expected in &n.states[..3] {
            equal_state(&initial, expected, &context);
        }
        let mut properties = HigurashiProperties::from_seeds([1, 2, 3, 4]);
        for (index, value) in match c.profile {
            1 => vec![(6, 300.), (9, 500.)],
            2 => vec![(19, 0.)],
            3 => vec![(9, 0.)],
            _ => vec![],
        } {
            properties
                .tweener_mut(index)
                .unwrap()
                .fast_forward_to(value);
        }
        assert!(
            view.packets(
                &properties,
                &HigurashiTransform::default(),
                PassKind::Opaque
            )
            .unwrap()
            .is_empty()
        );
        let packets = view
            .packets(
                &properties,
                &HigurashiTransform::default(),
                PassKind::Transparent,
            )
            .unwrap();
        assert_eq!(packets.len(), n.draws.len(), "{context}:renderpacketcount");
        assert_eq!(
            n.scopes,
            [
                packets.len() as u32,
                u32::from(c.kind == 9 || matches!(c.profile, 0 | 1)),
                u32::from(c.kind == 9 || matches!(c.profile, 0 | 1))
            ],
            "{context}:originalscope"
        );
        for (p, d) in packets.iter().zip(&n.draws) {
            equal_draw(p, d, &context);
            draws += 1;
        }
        equal_state(&snapshot(&view), &n.states[3], &context);
    }
    eprintln!(
        "Saved animations:{} originalprofiles,{owners} warmedowners,{draws} originalpackets;nullable4 clones/4OS+1MT/refreshedstate/realSakura expiry/Focus1430geometry/Rainwrap+sort;CPU transport scope,no GPU/audio/windows/saves",
        cases.len()
    );
}

#[test]
#[ignore = "actual a1804 warm-up loop with checked a1848 prologue trampoline; original accepted-count retry and300-unit fall clocks; CPU only"]
fn higurashi_saved_rain_first_expiry_clocks_match_original() {
    let executable = std::fs::read(std::env::var("SHIN_HIGURASHI_EXE").unwrap()).unwrap();
    for (address, expected) in [
        (0xa1e14, 0x54fffb2cu32),
        (0xa1d74, 0x51000718),
        (0xa1f14, 0x52a872c8),
    ] {
        assert_eq!(
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
            expected,
            "Original reject branch skips count decrement; mode0 falls300 at{address:x}"
        );
    }
    let descriptors = [0, 1, 3499211612, u32::MAX]
        .into_iter()
        .flat_map(|seed| (0..4).map(move |mode| (seed, mode)))
        .collect::<Vec<_>>();
    let mut input = String::new();
    for (seed, mode) in &descriptors {
        writeln!(input, "W {seed} {mode} 100 1500").unwrap();
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_PREVIEW_RAIN_WARMUP").unwrap())
            .args([
                std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
                std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8(result.stdout).unwrap();
    assert_eq!(output.lines().count(), descriptors.len());
    let mut clocks = 0;
    for ((seed, mode), line) in descriptors.into_iter().zip(output.lines()) {
        let mut tokens = line.split_whitespace();
        assert_eq!(tokens.next(), Some("W"));
        assert_eq!(words::<4>(&mut tokens), [seed, mode as u32, 100, 1500]);
        let mut model = RainParticles::new_higurashi(mode, 100, 1500, seed);
        let owner = |model: &RainParticles| {
            let (_, words) = model.reference_owner_words();
            [
                model.reference_seed(),
                model.particles.len() as u32,
                model.expired,
                u32::from_str_radix(&words[7], 16).unwrap(),
            ]
        };
        let mut clock = 0;
        loop {
            match tokens.next().unwrap() {
                "T" => {
                    clock += 1;
                    clocks += 1;
                    assert_eq!(word(&mut tokens), clock);
                    assert_eq!(
                        owner(&model),
                        words::<4>(&mut tokens),
                        "before native clock{clock},seed{seed},mode{mode}"
                    );
                    model.update(1., 1000., 0., 1000.);
                    assert_eq!(
                        owner(&model),
                        words::<4>(&mut tokens),
                        "after native clock{clock},seed{seed},mode{mode}"
                    );
                }
                "E" => {
                    assert_eq!(words::<4>(&mut tokens), [clock, 4, 1, 0]);
                    break;
                }
                token => panic!("Original Rain clock event{token}"),
            }
        }
        let expected = state(&mut tokens);
        assert!(tokens.next().is_none());
        assert!(model.expired > 0);
        model.warm_up(1000., 0., 1000.); // Actual constructor's stop condition is already reached.
        assert!(!model.needs_warm_up);
        let view = View {
            state: State::Rain(Mutex::new(model)),
            atlas: None,
        };
        equal_state(
            &snapshot(&view),
            &expected,
            &format!("original Rain clocks seed{seed},mode{mode}"),
        );
    }
    eprintln!(
        "Saved Rain:16 actual original warm-up owners,{clocks} before/after update clocks; first expiry/rejected-candidate retries/300-unit rain fall/full final particle vectors; CPU only"
    );
}
