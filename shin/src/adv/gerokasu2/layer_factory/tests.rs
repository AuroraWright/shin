use super::*;
use crate::layer::{DrawableLayer as _, Layer as _};
use shin_core::{
    format::{
        rom::RomReader,
        scenario::gerokasu2::{BustupInfoItem, MovieInfoItem, PictureInfoItem},
        text::utf8::U16Utf8String,
    },
    primitives::stateless_reader::StatelessFile,
};
use std::{
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

fn scenario() -> Gerokasu2Scenario {
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let entry = rom.find_file("/main.snr").unwrap();
    let mut data = Vec::new();
    rom.open_file(entry)
        .unwrap()
        .read_to_end(&mut data)
        .unwrap();
    Gerokasu2Scenario::new(data.into()).unwrap()
}
fn write_string(out: &mut Vec<u8>, s: &str) {
    out.extend((s.len() as u16).to_le_bytes());
    out.extend(s.as_bytes());
}

#[test]
fn ger_effect_publication_constructs_independent_capture_owners_without_artwork() {
    let prepared = PreparedBasicDrawable::Procedural(
        crate::adv::gerokasu2::procedural_animation::PreparedProcedural::Effect,
    );
    crate::engine_random::with_reference_seed(5489, || {
        let calls = crate::engine_random::seed_call_count();
        let first = prepared.try_construct().unwrap();
        let second = prepared.try_construct().unwrap();
        assert_eq!(crate::engine_random::seed_call_count() - calls, 2);
        assert!(first.needs_lower_capture() && second.needs_lower_capture());
        for layer in [&first, &second] {
            assert!(matches!(
                layer.layer,
                GerDrawable::Shared(UserLayer::Effect(_))
            ));
            assert_eq!(
                layer.properties().policy(),
                crate::layer::PropertyPolicy::Gerokasu2
            );
            assert!(layer.properties().initialized_identity().is_none());
        }
        assert_ne!(
            first.properties().identity(),
            second.properties().identity()
        );
        assert_eq!(crate::engine_random::next_seed(), 3890346734);
    });
}

#[test]
fn ger_invalid_animation_retains_absent_value_until_publication_without_null_or_rng() {
    for mode in [i32::MIN, -1, 6, i32::MAX] {
        let prepared = PreparedBasicDrawable::InvalidAnimation(mode);
        let calls = crate::engine_random::seed_call_count();
        assert!(!prepared.has_drawable());
        assert!(
            prepared
                .try_construct()
                .err()
                .unwrap()
                .to_string()
                .contains("returned no drawable at publication")
        );
        assert_eq!(crate::engine_random::seed_call_count(), calls);
    }
}

struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    cases: usize,
    bytes: usize,
}
impl Native {
    fn new(scenario: &Gerokasu2Scenario) -> Self {
        let mut child = Command::new(std::env::var_os("SHIN_GEROKASU2_FACTORY_ORACLE").unwrap())
            .args([
                std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_MEDIUM_FONT").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_SYSTEM_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut data = b"NGF1".to_vec();
        for n in [
            scenario.picture_info.len(),
            scenario.bustup_info.len(),
            scenario.movie_info.len(),
        ] {
            data.extend((n as u32).to_le_bytes());
        }
        for p in &scenario.picture_info {
            write_string(&mut data, p.name.as_str());
            data.extend(p.linked_cg_id.to_le_bytes());
        }
        for p in &scenario.bustup_info {
            write_string(&mut data, p.name.as_str());
            write_string(&mut data, p.emotion.as_str());
            data.extend(p.lipsync_character_id.to_le_bytes());
            for v in p.raw_placement {
                data.extend(v.to_le_bytes());
            }
        }
        for p in &scenario.movie_info {
            write_string(&mut data, p.name.as_str());
            data.extend(p.linked_picture_id.to_le_bytes());
            data.extend([p.volume_source, p.transparency]);
            data.extend(p.linked_bgm_id.to_le_bytes());
        }
        let mut input = child.stdin.take().unwrap();
        input.write_all(&data).unwrap();
        Self {
            input: Some(input),
            output: child.stdout.take().unwrap(),
            child,
            cases: 0,
            bytes: 0,
        }
    }
    fn compare(&mut self, scenario: &Gerokasu2Scenario, kind: i32, params: [i32; 8]) {
        let mut row = kind.to_le_bytes().to_vec();
        for p in params {
            row.extend(p.to_le_bytes());
        }
        self.input.as_mut().unwrap().write_all(&row).unwrap();
        let request = LayerFactoryRequest::resolve(
            scenario,
            kind,
            (
                params[0], params[1], params[2], params[3], params[4], params[5], params[6],
                params[7],
            ),
        );
        let mut words = [0u32; 8];
        let mut strings = [String::new(), String::new(), String::new()];
        let tag = match request {
            None => u32::MAX,
            Some(LayerFactoryRequest::Null) => 0,
            Some(LayerFactoryRequest::Tile { color, rect }) => {
                words[..4].copy_from_slice(&color.into_array().map(f32::to_bits));
                words[4..].copy_from_slice(&rect.to_array().map(f32::to_bits));
                1
            }
            Some(LayerFactoryRequest::Picture { path }) => {
                strings[0] = path;
                2
            }
            Some(LayerFactoryRequest::Bustup {
                path,
                expression,
                character_id,
            }) => {
                strings[0] = path;
                strings[1] = expression;
                words[0] = character_id as u32;
                3
            }
            Some(LayerFactoryRequest::Animation { mode }) => {
                words[0] = mode as u32;
                4
            }
            Some(LayerFactoryRequest::Effect) => 5,
            Some(LayerFactoryRequest::Movie {
                path,
                still,
                volume_source,
                transparency,
                volume,
                repeat,
            }) => {
                strings[0] = path;
                strings[2] = still.unwrap_or_default();
                words[..4].copy_from_slice(&[
                    u32::from(volume_source),
                    u32::from(transparency),
                    volume.to_bits(),
                    u32::from(repeat),
                ]);
                6
            }
            Some(LayerFactoryRequest::FocusLine { x, y }) => {
                words[..2].copy_from_slice(&[x.to_bits(), y.to_bits()]);
                7
            }
            Some(LayerFactoryRequest::Rain { mode, min, max }) => {
                words[..3].copy_from_slice(&[mode as u32, min as u32, max as u32]);
                8
            }
            Some(LayerFactoryRequest::CardSelect {
                first,
                second,
                third,
            }) => {
                words[..3].copy_from_slice(&[first as u32, second as u32, third as u32]);
                9
            }
        };
        let mut expected = b"GFR1".to_vec();
        expected.extend(tag.to_le_bytes());
        for w in words {
            expected.extend(w.to_le_bytes());
        }
        for s in &strings {
            expected.extend((s.len() as u32).to_le_bytes());
        }
        for s in &strings {
            expected.extend(s.as_bytes());
        }
        let mut actual = vec![0; expected.len()];
        self.output.read_exact(&mut actual).unwrap();
        assert_eq!(actual, expected, "factory kind{kind} params{params:?}");
        self.cases += 1;
        self.bytes += actual.len();
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
#[ignore = "requires supplied ROM/executable/fonts and original factory oracle;CPU only"]
fn all_native_gerokasu2_layer_factory_requests_paths_fields_and_bounds() {
    let mut scenario = scenario();
    let original = [
        scenario.picture_info.len(),
        scenario.bustup_info.len(),
        scenario.movie_info.len(),
    ];
    scenario.picture_info.push(PictureInfoItem {
        name: U16Utf8String::new("画像_日本語_長い名前_123456789012345678901234567890"),
        linked_cg_id: -1,
    });
    for lip in [0, 32767, 32768, 65535] {
        scenario.bustup_info.push(BustupInfoItem {
            name: U16Utf8String::new("立ち絵_長い名前_123456789012345678901234567890"),
            emotion: U16Utf8String::new("通常_顔影2_長い表情_123456789012345678901234567890"),
            lipsync_character_id: lip,
            raw_placement: [i16::MIN, i16::MAX, -1],
        });
    }
    for (picture, source, transparency) in [
        (-1, 0, 0),
        (0, 1, 1),
        (i16::MAX, 2, 255),
        (i16::MIN, 255, 2),
    ] {
        scenario.movie_info.push(MovieInfoItem {
            name: U16Utf8String::new("動画_長い名前_123456789012345678901234567890"),
            linked_picture_id: picture,
            volume_source: source,
            transparency,
            linked_bgm_id: -1,
        });
    }
    let mut native = Native::new(&scenario);
    for color in 0..=65535 {
        native.compare(
            &scenario,
            1,
            [color, -960, 540, 1920, 1080, -1, i32::MIN, i32::MAX],
        );
    }
    for (kind, count) in [
        (2, scenario.picture_info.len()),
        (3, scenario.bustup_info.len()),
        (6, scenario.movie_info.len()),
    ] {
        for id in -2..=count as i32 + 1 {
            for quality in [0, 1, -1, i32::MIN, i32::MAX] {
                native.compare(
                    &scenario,
                    kind,
                    [id, quality, 255, 0, i32::MIN, i32::MAX, 1000, -1000],
                );
            }
        }
    }
    for kind in [-1, 0, 2, 3, 4, 5, 6, 7, 8, 9, 10, i32::MIN, i32::MAX] {
        for a in [i32::MIN, -1, 0, 1, 256, 65536, i32::MAX] {
            for b in [i32::MIN, -1, 0, 1, 2, 3, 700, 1001, i32::MAX] {
                native.compare(&scenario, kind, [a, b, b, 4, 5, 6, 7, 8]);
            }
        }
    }
    println!(
        "GEROKASU2_FACTORY_NATIVE cases={} bytes={} supplied_pictures={} supplied_bustups={} supplied_movies={} all_ten_requests_paths_bounds_color_quality_signed_character_raw_movie_flags=verified",
        native.cases, native.bytes, original[0], original[1], original[2]
    );
}

#[cfg(target_os = "macos")]
mod gpu;
mod inventory;
