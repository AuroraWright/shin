use crate::layer::{
    EffectPhases, LayerProperties, PropertyPolicy,
    user::{SakuraArithmetic, SakuraParticles},
};
use enum_map::Enum;
use shin_core::vm::command::types::LayerProperty;
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
};

#[test]
fn ger_sakura_original_warmup_uses_fma_without_changing_legacy_profile() {
    let first_speed = [
        0x3fee763d, 0x400f3b1f, 0x3fee763d, 0x400f3b1f, 0x3ff18f30, 0x3ff18f30,
    ];
    for mode in 0..6 {
        let ger =
            SakuraParticles::new_with_arithmetic(mode, 1, SakuraArithmetic::Gerokasu2).unwrap();
        assert_eq!(ger.elapsed, 500.0);
        assert_eq!(ger.particles.len(), 4);
        assert_eq!(ger.reference_seed(), 1_479_919_876);
        assert_eq!(
            ger.reference_owner_emission().to_bits(),
            if mode < 4 { 0x42e76460 } else { 0x43030335 }
        );
        assert_eq!(
            ger.particles[0].travel_speed.to_bits(),
            first_speed[mode as usize]
        );
        if mode == 0 || mode == 2 {
            assert_eq!(
                SakuraParticles::new(mode, 1).unwrap().particles[0]
                    .travel_speed
                    .to_bits(),
                0x3fee763e
            );
        }
    }
    for mode in [-1, 6, i32::MIN, i32::MAX] {
        assert!(
            SakuraParticles::new_with_arithmetic(mode, 1, SakuraArithmetic::Gerokasu2).is_none()
        );
    }
}

#[test]
fn ger_effect_phases_match_original_fractional_updates_and_freeze_zero() {
    // Captured from complete original Effect ctor and guarded6C3F0 updates.
    let mut phases = EffectPhases::default();
    for _ in 0..8 {
        phases.advance_gerokasu2(0.1, [3.0, -7.0, 0.0]);
    }
    assert_eq!(phases.0.map(f32::to_bits), [0x3e888889, 0xbdea0ea1, 0]);
    let retained = phases;
    phases.advance_gerokasu2(100.0, [0.0, -0.0, 0.0]);
    assert_eq!(phases.0.map(f32::to_bits), retained.0.map(f32::to_bits));
}

struct Expected {
    kind: u32,
    valid: bool,
    seed: u32,
    seed_calls: u32,
    frame: u32,
    property_frame: u32,
    props: Option<LayerProperties>,
    phases: EffectPhases,
    particles: Option<SakuraParticles>,
}
impl Expected {
    fn new(kind: u32, mode: i32, seed: u32) -> Self {
        let particles = (kind == 4)
            .then(|| SakuraParticles::new_with_arithmetic(mode, seed, SakuraArithmetic::Gerokasu2))
            .flatten();
        let valid = kind == 5 || particles.is_some();
        let mut result = Self {
            kind,
            valid,
            seed,
            seed_calls: u32::from(valid),
            frame: 0,
            property_frame: 0,
            props: None,
            phases: Default::default(),
            particles,
        };
        if result.particles.is_some() {
            result.seed_calls += 1;
            result.materialize();
        }
        result
    }
    fn materialize(&mut self) {
        if self.props.is_none() {
            let props = LayerProperties::with_policy(PropertyPolicy::Gerokasu2);
            props.get_value(LayerProperty::RasterHorizontalTPeriod);
            self.props = Some(props);
            self.seed_calls += 7;
        }
    }
    fn periods(&mut self, values: [f32; 3]) {
        self.materialize();
        for (p, value) in [
            LayerProperty::RasterHorizontalTPeriod,
            LayerProperty::RasterVerticalTPeriod,
            LayerProperty::RippleTPeriod,
        ]
        .into_iter()
        .zip(values)
        {
            self.props
                .as_mut()
                .unwrap()
                .property_tweener_mut(p)
                .fast_forward_to(value);
        }
    }
    fn update(&mut self, frame: u32, delta: f32, permission: bool) {
        if self.frame == frame {
            return;
        }
        self.frame = frame;
        if self.props.is_some() {
            self.property_frame = frame;
        }
        self.materialize();
        let periods = [
            LayerProperty::RasterHorizontalTPeriod,
            LayerProperty::RasterVerticalTPeriod,
            LayerProperty::RippleTPeriod,
        ]
        .map(|p| self.props.as_ref().unwrap().get_value(p));
        self.phases.advance_gerokasu2(delta, periods);
        if let Some(particles) = &mut self.particles {
            particles.update(delta, permission);
        }
    }
    fn copied(&mut self) {
        self.frame = 0;
        self.particles = self.particles.clone();
        if self.props.is_some() {
            self.seed_calls += 6;
            self.props = self.props.clone();
        }
    }
    fn words(&self) -> Vec<u32> {
        let mut result = vec![
            self.kind,
            u32::from(self.valid),
            self.seed_calls,
            self.frame,
            u32::from(self.props.is_some()),
            self.property_frame,
            if self.valid { self.seed } else { 0 },
        ];
        result.extend(self.phases.0.map(f32::to_bits));
        if let Some(particles) = &self.particles {
            result.extend([
                particles.elapsed.to_bits(),
                particles.reference_seed(),
                particles.mode as u32,
                particles.reference_owner_emission().to_bits(),
                particles.particles.len() as u32,
            ]);
        } else {
            result.extend([0; 5]);
        }
        for i in 0..90 {
            result.push(
                self.props
                    .as_ref()
                    .map_or(0, |p| p.get_value(LayerProperty::from_usize(i)).to_bits()),
            );
        }
        if let Some(particles) = &self.particles {
            for p in &particles.particles {
                result.extend([u32::from(p.alive), p.variant as u32]);
                result.extend(
                    [
                        p.origin.x,
                        p.origin.y,
                        p.travel_speed,
                        p.travel_angle,
                        p.radius,
                        p.scale,
                        p.spin_angle,
                        p.spin_speed,
                        p.age,
                    ]
                    .map(f32::to_bits),
                );
            }
        }
        result
    }
}
struct Native {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::process::ChildStdout,
    operations: usize,
    words: usize,
}
impl Native {
    fn new(root: &std::path::Path) -> Self {
        let folder = root.join("reverse-engineering/gerokasu2/procedural-animation");
        std::fs::create_dir_all(&folder).unwrap();
        let binary = folder.join("native-procedural-animation");
        let compiled = Command::new("clang")
            .args(["-std=c11", "-O2"])
            .arg(root.join("reverse-engineering/gerokasu2/native-procedural-animation.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let mut child = Command::new(binary)
            .arg(root.join("game-data/gerokasu2/uncompressed_executable"))
            .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
            .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
            operations: 0,
            words: 0,
        }
    }
    fn compare(&mut self, row: [u32; 10], expected: &Expected) {
        self.input
            .as_mut()
            .unwrap()
            .write_all(
                &row.into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let mut tag = [0; 4];
        self.output.read_exact(&mut tag).unwrap();
        assert_eq!(&tag, b"GPA1");
        let mut header = [0u8; 60];
        self.output.read_exact(&mut header).unwrap();
        let header = header
            .chunks_exact(4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
            .collect::<Vec<_>>();
        let count = 90
            + if header[0] == 4 && header[1] != 0 {
                header[14] as usize * 11
            } else {
                0
            };
        let mut bytes = vec![0; count * 4];
        self.output.read_exact(&mut bytes).unwrap();
        let mut actual = header;
        actual.extend(
            bytes
                .chunks_exact(4)
                .map(|w| u32::from_le_bytes(w.try_into().unwrap())),
        );
        let expected = expected.words();
        assert_eq!(actual.len(), expected.len(), "operation{}", self.operations);
        let mismatches = actual
            .iter()
            .zip(&expected)
            .enumerate()
            .filter_map(|(i, (a, b))| (a != b).then_some((i, *a, *b)))
            .collect::<Vec<_>>();
        assert!(
            mismatches.is_empty(),
            "original procedural operation{} row{row:?} differing words{mismatches:?}",
            self.operations
        );
        self.operations += 1;
        self.words += actual.len();
    }
    fn create(&mut self, kind: u32, mode: i32, seed: u32) -> Expected {
        let expected = Expected::new(kind, mode, seed);
        self.compare([0, kind, mode as u32, seed, 0, 0, 0, 0, 0, 0], &expected);
        expected
    }
    fn update(&mut self, expected: &mut Expected, frame: u32, delta: f32, permission: bool) {
        expected.update(frame, delta, permission);
        self.compare(
            [
                1,
                0,
                0,
                0,
                frame,
                delta.to_bits(),
                u32::from(permission),
                0,
                0,
                0,
            ],
            expected,
        );
    }
    fn periods(&mut self, expected: &mut Expected, values: [f32; 3]) {
        expected.periods(values);
        self.compare(
            [
                2,
                0,
                0,
                0,
                0,
                0,
                0,
                values[0].to_bits(),
                values[1].to_bits(),
                values[2].to_bits(),
            ],
            expected,
        );
    }
    fn copy(&mut self, expected: &mut Expected) {
        expected.copied();
        self.compare([3, 0, 0, 0, 0, 0, 0, 0, 0, 0], expected);
    }
    fn fast_forward(&mut self, expected: &mut Expected) {
        if let Some(props) = &mut expected.props {
            props.fast_forward();
        }
        self.compare([5, 0, 0, 0, 0, 0, 0, 0, 0, 0], expected);
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        self.input.take();
        let status = self.child.wait().unwrap();
        if !std::thread::panicking() {
            assert!(status.success());
        }
    }
}

#[test]
#[ignore = "executes original Ger procedural constructors/update/copy/property bodies; CPU only"]
fn ger_native_procedural_sakura_effect_warmup_motion_phase_lazy_props_and_copy() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let mut native = Native::new(root);
    let seeds = [
        0,
        1,
        2,
        42,
        2_147_483_646,
        2_147_483_647,
        2_147_483_648,
        u32::MAX,
    ];
    for kind in [4, 5] {
        for mode in 0..if kind == 4 { 6 } else { 1 } {
            for seed in seeds {
                let mut state = native.create(kind, mode, seed);
                if kind == 5 {
                    native.copy(&mut state);
                }
                native.fast_forward(&mut state);
                native.update(&mut state, 0, 17.3, true);
                native.periods(&mut state, [3.0, -7.0, 0.0]);
                native.update(&mut state, 1, 0.1, false);
                native.update(&mut state, 1, 1000.0, true);
                native.update(&mut state, 2, 0.125, true);
                native.copy(&mut state);
                native.update(&mut state, 2, -2.0, false);
                if let Some(particles) = &mut state.particles {
                    particles.particles[0].alive = false;
                    native.compare([4, 0, 0, 0, 0, 0, 0, 0, 0, 0], &state);
                    native.copy(&mut state);
                }
                native.update(&mut state, 3, 0.0, false);
                native.periods(&mut state, [0.0, 17.1, -3.0]);
                native.update(&mut state, 4, 1000.0, false);
                native.update(&mut state, 5, -17.3, true);
                native.update(&mut state, 6, 33.3, true);
                native.periods(&mut state, [101.0, 3.0, -0.0]);
                native.update(&mut state, 7, 7.3, true);
                native.fast_forward(&mut state);
            }
        }
    }
    for mode in [i32::MIN, -1, 6, 7, i32::MAX] {
        native.create(4, mode, 42);
    }
    println!(
        "GEROKASU2_PROCEDURAL_NATIVE modes=6 seeds=8 effect_cases=8 invalid_modes=5 operations={} compared_words={} original_ctor_guarded_update_copy_default90properties_RNG_counts_FMA_motion_phases=verified SDK_draw=excluded",
        native.operations, native.words
    );
}

#[test]
#[ignore = "checks original Ger Sakura sprite tables and supplied effects archive; CPU only"]
fn ger_native_sakura_sprite_descriptors_and_actual_atlas_requirement() {
    use shin_core::{
        format::{
            rom::RomReader, text::TextEncoding, texture_archive::read_texture_archive_with_encoding,
        },
        primitives::stateless_reader::StatelessFile,
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let image = std::fs::read(root.join("game-data/gerokasu2/uncompressed_executable")).unwrap();
    let q = |a: usize| u64::from_le_bytes(image[a + 256..a + 264].try_into().unwrap()) as usize;
    let w = |a: usize| u32::from_le_bytes(image[a + 256..a + 260].try_into().unwrap());
    let table = q(0x158940);
    let particle = SakuraParticles::new_with_arithmetic(0, 1, SakuraArithmetic::Gerokasu2)
        .unwrap()
        .particles
        .remove(0);
    let mut frames = 0;
    for variant in 0..2 {
        assert_eq!(q(table + variant * 16), 12);
        let list = q(table + variant * 16 + 8);
        let mut age = 0;
        for frame in 0..12 {
            let duration = w(list + frame * 16) as i32;
            let descriptor = q(list + frame * 16 + 8);
            assert_eq!(w(descriptor + 16), 1);
            let sprite = q(descriptor + 24);
            let texture = q(sprite);
            let name = q(texture);
            assert_eq!(&image[name + 256..name + 263], b"Sakura\0");
            let mut p = particle.clone();
            p.variant = variant;
            p.age = age as f32;
            assert_eq!(
                p.sprite().uv.map(f32::to_bits),
                std::array::from_fn(|i| w(sprite + 8 + i * 4))
            );
            assert_eq!(
                p.sprite().bounds,
                std::array::from_fn(|i| w(sprite + 24 + i * 4) as i32)
            );
            p.age = (age + duration) as f32 - 0.01;
            assert_eq!(
                p.sprite().uv.map(f32::to_bits),
                std::array::from_fn(|i| w(sprite + 8 + i * 4))
            );
            age += duration;
            frames += 1;
        }
    }
    let path = std::env::var_os("SHIN_GEROKASU2_ROM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("game-data/gerokasu2/data.rom"));
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap()).unwrap();
    let file = rom.find_file("/effects.txa").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let atlas = read_texture_archive_with_encoding(&bytes, TextEncoding::Utf8).unwrap();
    let sakura = atlas.get_texture("Sakura");
    println!(
        "GEROKASU2_SAKURA_TABLES frames={frames} duration_boundaries_literal_UV_bounds_texture_name=verified supplied_effects_names={:?} Sakura_present={} dimensions={:?}",
        atlas.name_to_index.keys().collect::<Vec<_>>(),
        sakura.is_some(),
        sakura.map(|s| s.dimensions())
    );
}
