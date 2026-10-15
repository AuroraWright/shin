use super::*;

mod pixels;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

#[derive(Clone, Debug)]
struct Case {
    kind: i32,
    seed: u32,
    old: i32,
    profile: u32,
    steps: usize,
    params: [i32; 8],
}
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, PartialEq, Eq)]
struct Base {
    values: [u32; 65],
    oscillator: [u32; 8],
    busy: [u32; 65],
}
#[derive(Debug)]
struct Phase {
    id: u32,
    base: Base,
    private: Snapshot,
}
struct Native {
    present: bool,
    os: u32,
    mt: u32,
    phases: Vec<Phase>,
    draws: Vec<Vec<Draw>>,
    copy_entropy: [u32; 2],
}
fn word(w: &mut std::str::SplitWhitespace<'_>) -> u32 {
    w.next().unwrap().parse().unwrap()
}
fn words<const N: usize>(w: &mut std::str::SplitWhitespace<'_>) -> [u32; N] {
    std::array::from_fn(|_| word(w))
}
fn state(w: &mut std::str::SplitWhitespace<'_>) -> Snapshot {
    let (kind, n, stride) = match w.next().unwrap() {
        "S" => (4, 5, 11),
        "F" => (8, 4, 2),
        "R" => (9, 9, 22),
        v => panic!("Native private state{v}"),
    };
    let header: Vec<_> = (0..n).map(|_| word(w)).collect();
    let rows = (0..*header.last().unwrap())
        .map(|_| (0..stride).map(|_| word(w)).collect())
        .collect();
    Snapshot { kind, header, rows }
}
fn parse(line: &str) -> Native {
    let mut w = line.split_whitespace();
    assert_eq!(w.next(), Some("L"));
    let kind = word(&mut w) as i32;
    let mut n = Native {
        present: word(&mut w) != 0,
        os: word(&mut w),
        mt: word(&mut w),
        phases: vec![],
        draws: vec![],
        copy_entropy: [u32::MAX; 2],
    };
    assert_eq!(
        word(&mut w),
        0,
        "Original effects singleton uses no asset ticket"
    );
    if !n.present {
        assert!(w.next().is_none());
        return n;
    }
    assert_eq!(w.next(), Some("V"));
    assert_eq!(
        words::<2>(&mut w),
        match kind {
            4 => [0x25650, 0x26160],
            8 => [0x5b1d8, 0x5b0fc],
            9 => [0xa2174, 0xa1b14],
            _ => unreachable!(),
        }
    );
    while let Some(t) = w.next() {
        match t {
            "B" => {
                let id = word(&mut w);
                assert_eq!(w.next(), Some("H"));
                let base = Base {
                    values: words(&mut w),
                    oscillator: words(&mut w),
                    busy: words(&mut w),
                };
                n.phases.push(Phase {
                    id,
                    base,
                    private: state(&mut w),
                });
            }
            "C" => n.copy_entropy = words(&mut w),
            "D" => {
                let mut draws = vec![];
                loop {
                    match w.next().unwrap() {
                        "P" => draws.push(Draw {
                            program: word(&mut w),
                            blend: word(&mut w),
                            texture: word(&mut w),
                            matrix: words(&mut w),
                            color: words(&mut w),
                            param: words(&mut w),
                            vertices: words(&mut w),
                        }),
                        "E" => {
                            let scopes = words::<3>(&mut w);
                            assert_eq!(scopes, [draws.len() as u32, 1, 1]);
                            break;
                        }
                        v => panic!("Native render token{v}"),
                    }
                }
                n.draws.push(draws);
            }
            v => panic!("Native live token{v}"),
        }
    }
    n
}
fn snapshot(view: &Owner) -> Snapshot {
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
        State::Rain {
            particles: model,
            elapsed,
        } => {
            let model = model.lock();
            let (_, owner) = model.reference_owner_words();
            Snapshot {
                kind: 9,
                header: vec![
                    elapsed.to_bits(),
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
        native.program,
        [6, 9, 7, 8, 10, 11][packet.shader as usize],
        "{context}:program"
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
fn base(properties: &HigurashiProperties) -> Base {
    Base {
        values: properties.values().map(f32::to_bits),
        oscillator: std::array::from_fn(|i| {
            let o = properties.oscillators[i / 2];
            if i % 2 == 0 {
                o.seed
            } else {
                o.phase.to_bits()
            }
        }),
        busy: std::array::from_fn(|i| u32::from(properties.tweener(i).unwrap().is_waiting())),
    }
}
fn check(owner: &Owner, props: &HigurashiProperties, n: &Phase, context: &str) {
    let a = base(props);
    assert_eq!(
        a.oscillator, n.base.oscillator,
        "{context}:oscillator seeds/phases"
    );
    assert_eq!(a.busy, n.base.busy, "{context}:queued clock lifetime");
    assert_eq!(a.values, n.base.values, "{context}:exact65 property values");
    equal_state(&snapshot(owner), &n.private, context);
}
fn old_base(profile: u32) -> HigurashiProperties {
    let mut p = HigurashiProperties::from_seeds(std::array::from_fn(|i| 0xabc00001 + i as u32));
    p.tweener_mut(0).unwrap().fast_forward_to(20.);
    p.control(0, 120., Ticks::from_f32(10.), 0).unwrap();
    p.control(0, 200., Ticks::from_f32(1000.), 0).unwrap();
    for (i, v) in [(50, 0.), (27, 5.), (28, 80.), (29, 9.)] {
        p.tweener_mut(i).unwrap().fast_forward_to(v);
    }
    for (i, v) in match profile {
        1 => vec![(51, 2400.), (52, -250.), (53, 800.)],
        2 => vec![
            (9, 650.),
            (20, 2.),
            (21, 4.),
            (6, 500.),
            (7, 900.),
            (8, 1200.),
        ],
        3 => vec![(51, 0.), (52, 250.), (53, 800.)],
        _ => vec![],
    } {
        p.tweener_mut(i).unwrap().fast_forward_to(v);
    }
    p.advance(Ticks::from_f32(2.5));
    p
}
fn cases() -> Vec<Case> {
    let mut result = vec![];
    let specs: Vec<_> = (0..6)
        .map(|mode| (4, [mode, 100, 1500, 0, 0, 0, 0, 0]))
        .chain([
            (8, [0, 0, 0, 0, 0, 0, 0, 0]),
            (8, [-200, 100, 0, 0, 0, 0, 0, 0]),
            (8, [640, 360, 0, 0, 0, 0, 0, 0]),
        ])
        .chain((0..4).map(|mode| (9, [mode, 100, 1500, 0, 0, 0, 0, 0])))
        .collect();
    for (kind, params) in specs {
        for old in [99, 0, 4, 8, 9] {
            for profile in 0..3 {
                let i = result.len();
                result.push(Case {
                    kind,
                    params,
                    old,
                    profile,
                    steps: i % 7,
                    seed: [1, 0, 0xffffffff, 0x12345678, 0x7fffffff][i % 5],
                });
            }
        }
    }
    for mode in 0..4 {
        result.push(Case {
            kind: 9,
            seed: 1,
            old: 0,
            profile: 3,
            steps: 6,
            params: [mode, 100, 1500, 0, 0, 0, 0, 0],
        });
    }
    for old in [4, 9] {
        result.push(Case {
            kind: 4,
            seed: 1,
            old,
            profile: 3,
            steps: 0,
            params: [0, 100, 1500, 0, 0, 0, 0, 0],
        });
    }
    for mode in [-1, 6, 9, i32::MAX] {
        for old in [99, 0, 4, 8, 9] {
            result.push(Case {
                kind: 4,
                seed: 1,
                old,
                profile: 0,
                steps: 0,
                params: [mode, 0, 0, 0, 0, 0, 0, 0],
            });
        }
    }
    result
}
fn native(cases: &[Case]) -> Vec<Native> {
    let mut input = String::new();
    for c in cases {
        write!(
            input,
            "L {} {} {} {} {}",
            c.kind, c.seed, c.old, c.profile, c.steps
        )
        .unwrap();
        for p in c.params {
            write!(input, " {p}").unwrap();
        }
        input.push('\n');
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_LIVE_ANIMATION").unwrap())
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
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_LIVE_ANIMATION_CAPTURE") {
        std::fs::write(path, &output.stdout).unwrap();
    }
    let lines = String::from_utf8(output.stdout).unwrap();
    assert_eq!(lines.lines().count(), cases.len());
    lines.lines().map(parse).collect()
}
#[test]
#[ignore = "executes actual updated live4/8/9 old factories, queued clocks, virtual18, deep virtual38 copies and packet transports; no GPU/audio/windows/user saves"]
fn higurashi_live_animation_owners_match_original_update_copy_and_old_base() {
    let cases = cases();
    let originals = native(&cases);
    let mut owners = 0;
    let mut draws = 0;
    for (i, (c, n)) in cases.iter().zip(originals).enumerate() {
        let context = format!("case{i} {c:?}");
        let prepared = Prepared {
            kind: c.kind,
            params: c.params,
            atlas: None,
        };
        assert_eq!(prepared.present(), n.present);
        assert_eq!(
            (n.os, n.mt),
            if !n.present {
                (0, 0)
            } else if c.old == 99 {
                (4, 1)
            } else {
                (0, 1)
            },
            "{context}:replacement entropy"
        );
        if !n.present {
            let mut props = HigurashiProperties::from_seeds([1; 4]);
            let before = props.values();
            assert!(
                prepared
                    .construct(&mut props, Some(123.25), c.seed)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(props.values(), before);
            continue;
        }
        owners += 1;
        let mut props = if c.old == 99 {
            HigurashiProperties::from_seeds(std::array::from_fn(|i| 0xabc00001 + i as u32))
        } else {
            old_base(c.profile)
        };
        let elapsed = if matches!(c.old, 4 | 9) {
            Some(if c.profile == 3 { 16_777_216. } else { 123.25 })
        } else {
            None
        };
        let mut owner = prepared
            .construct(&mut props, elapsed, c.seed)
            .unwrap()
            .unwrap();
        check(&owner, &props, &n.phases[0], &context);
        assert_eq!(owner.elapsed().is_some(), c.kind != 8);
        assert!(
            owner
                .packets(&props, &HigurashiTransform::default(), PassKind::Opaque)
                .unwrap()
                .is_empty()
        );
        let packets = owner
            .packets(
                &props,
                &HigurashiTransform::default(),
                PassKind::Transparent,
            )
            .unwrap();
        assert_eq!(packets.len(), n.draws[0].len(), "{context}:initial packets");
        for (p, d) in packets.iter().zip(&n.draws[0]) {
            equal_draw(p, d, &context);
            draws += 1;
        }
        check(&owner, &props, &n.phases[1], &context);
        for (step, delta) in [0., 0.25, 1., 2.5, 0., -0.25]
            .into_iter()
            .take(c.steps)
            .enumerate()
        {
            owner.advance(Ticks::from_f32(delta), &mut props);
            check(
                &owner,
                &props,
                &n.phases[2 + step],
                &format!("{context}:advance{step}"),
            );
        }
        assert_eq!(n.copy_entropy, [0, 0], "{context}:deep-copy entropy");
        let mut copy = owner.clone();
        let mut copied_props = props.clone();
        let before = snapshot(&owner);
        let base_before = base(&props);
        let copied_phase = 2 + c.steps;
        check(&copy, &copied_props, &n.phases[copied_phase], &context);
        // Exact bitwise private Rust copy and original copy, independently of
        // previously audited bounded particle arithmetic comparisons.
        assert_eq!(snapshot(&copy), before);
        assert_eq!(base(&copied_props), base_before);
        assert_eq!(n.phases[copied_phase].base, n.phases[copied_phase - 1].base);
        assert_eq!(
            n.phases[copied_phase].private,
            n.phases[copied_phase - 1].private
        );
        copy.advance(Ticks::from_f32(1.), &mut copied_props);
        assert_eq!(snapshot(&owner), before);
        assert_eq!(base(&props), base_before);
        check(&owner, &props, &n.phases[copied_phase + 1], &context);
        check(&copy, &copied_props, &n.phases[copied_phase + 2], &context);
        let packets = copy
            .packets(
                &copied_props,
                &HigurashiTransform::default(),
                PassKind::Transparent,
            )
            .unwrap();
        assert_eq!(
            packets.len(),
            n.draws[1].len(),
            "{context}:advanced copy packets"
        );
        for (p, d) in packets.iter().zip(&n.draws[1]) {
            equal_draw(p, d, &context);
            draws += 1;
        }
        check(&copy, &copied_props, &n.phases[copied_phase + 3], &context);
        drop(owner);
        drop(props);
        check(&copy, &copied_props, &n.phases[copied_phase + 4], &context);
        assert_eq!(
            n.phases[copied_phase + 3].base,
            n.phases[copied_phase + 4].base
        );
        assert_eq!(
            n.phases[copied_phase + 3].private,
            n.phases[copied_phase + 4].private,
            "{context}:original copy survives source retirement"
        );
    }
    eprintln!(
        "Live animations:{} native profiles,{owners} live owners,{draws} native packets; replacement/copy entropy,queued65 clocks+oscillators,positive/zero/negative delta,property50zero,deep private buffers and source retirement; prior bounded model arithmetic,CPU only",
        cases.len()
    );
}
#[test]
fn live_animation_clone_has_independent_private_simulation() {
    for (kind, params) in [(4, [0; 8]), (8, [0; 8]), (9, [0, 100, 1500, 0, 0, 0, 0, 0])] {
        let mut props = HigurashiProperties::from_seeds([1; 4]);
        let prepared = Prepared {
            kind,
            params,
            atlas: None,
        };
        let owner = prepared.construct(&mut props, None, 1).unwrap().unwrap();
        let before = snapshot(&owner);
        let mut copy = owner.clone();
        let mut copied_props = props.clone();
        copy.advance(Ticks::from_f32(2.), &mut copied_props);
        assert_eq!(snapshot(&owner), before);
        assert_ne!(snapshot(&copy), before);
    }
}
