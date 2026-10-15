use super::*;
use crate::layer::user::BustupBlink;
use shin_core::{
    format::{rom::RomReader, scenario::gerokasu2::Gerokasu2Scenario},
    primitives::stateless_reader::StatelessFile,
    vm::{command::types::LayerProperty, gerokasu2::message_face::MessageFaceState},
};
use std::{
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

struct Oracle {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
}
impl Oracle {
    fn new() -> Self {
        let mut child = Command::new(std::env::var_os("SHIN_GEROKASU2_PORTRAIT_ORACLE").unwrap())
            .arg(std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").unwrap())
            .arg(std::env::var_os("SHIN_GEROKASU2_MEDIUM_FONT").unwrap())
            .arg(std::env::var_os("SHIN_GEROKASU2_SYSTEM_FONT").unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
        }
    }
    fn row(&mut self, words: &[u32]) {
        self.input
            .as_mut()
            .unwrap()
            .write_all(
                &words
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    fn result(&mut self, tag: &[u8; 4], count: usize) -> Vec<u32> {
        let mut actual = [0; 4];
        self.output.read_exact(&mut actual).unwrap();
        assert_eq!(&actual, tag);
        let mut bytes = vec![0; count * 4];
        self.output.read_exact(&mut bytes).unwrap();
        bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect()
    }
}
impl Drop for Oracle {
    fn drop(&mut self) {
        self.input.take();
        let success = self.child.wait().unwrap().success();
        if !std::thread::panicking() {
            assert!(success);
        }
    }
}

fn scenario() -> Arc<Gerokasu2Scenario> {
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
    Arc::new(Gerokasu2Scenario::new(data.into()).unwrap())
}

#[test]
#[ignore = "requires supplied executable/fonts and guarded native portrait oracle; CPU only"]
fn native_gerokasu2_portrait_blink_mouth_placement_and_origin() {
    let mut oracle = Oracle::new();
    let mut animations = 0;
    let mut poses = 0;
    for seed in [
        0,
        1,
        42,
        44_488,
        2_147_483_646,
        2_147_483_647,
        2_147_483_648,
        u32::MAX,
    ] {
        for eyes in [0, 1, 2, 3, 9] {
            for mouths in [0, 1, 2, 3, 5] {
                let mut blink = BustupBlink::new_gerokasu2(eyes, seed);
                oracle.row(&[0, seed, mouths, eyes as u32, 0, 0]);
                let (state, elapsed, wait, _) = blink.reference_state();
                assert_eq!(oracle.result(b"GMP1", 6), [0, 0, state, elapsed, wait, 0]);
                animations += 1;
                let mut random = seed;
                for index in 0..128 {
                    random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let ticks = match index % 8 {
                        0 => 319.5,
                        1 => 0.5,
                        2 => 4.0,
                        3 => 20.0,
                        4 => 10000.0,
                        5 => -3.5,
                        _ => (random % 200) as f32 * 0.25,
                    };
                    let enabled = index % 11 != 0;
                    let listening = index % 7 != 0;
                    let level = match index % 6 {
                        0 => u32::MAX,
                        x => x as u32 - 1,
                    };
                    let mouth = if !enabled || !listening || level == u32::MAX || mouths == 0 {
                        0
                    } else {
                        (level * mouths / 3).min(mouths - 1)
                    };
                    let eye = blink.update(ticks, enabled);
                    let (state, elapsed, wait, _) = blink.reference_state();
                    oracle.row(&[
                        1,
                        ticks.to_bits(),
                        level,
                        enabled as u32,
                        listening as u32,
                        0,
                    ]);
                    assert_eq!(
                        oracle.result(b"GMP1", 6),
                        [mouth, eye, state, elapsed, wait, 0],
                        "seed{seed} eyes{eyes} mouths{mouths} index{index}"
                    );
                    animations += 1;
                }
            }
        }
    }
    let scenario = scenario();
    for index in 0..scenario.bustup_info.len() {
        let PortraitChange::Load(request) =
            MessageFaceState::new(index as i32).on_message_installed(&scenario.bustup_info)
        else {
            panic!()
        };
        let mut values = enum_map::EnumMap::<LayerProperty, i32>::from_fn(|p| p.initial_value());
        for (property, value) in request.properties() {
            values[property] = value;
        }
        for parent in [
            Mat4::IDENTITY,
            Mat4::from_cols_array(&[
                0.000981, 0.00007, 0.0, 0.0, -0.00005, -0.001713, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                0.012, -0.021, 0.0, 1.0,
            ]),
        ] {
            let origin = [
                (index as i16).wrapping_mul(-7),
                (index as i16).wrapping_mul(11),
            ];
            oracle.row(&[2, origin[0] as u16 as u32, origin[1] as u16 as u32, 0, 0, 0]);
            oracle.row(
                &values
                    .values()
                    .map(|&v| (v as f32).to_bits())
                    .collect::<Vec<_>>(),
            );
            oracle.row(&parent.to_cols_array().map(f32::to_bits));
            let native = oracle.result(b"GMP2", 33);
            let mut local = placement_transform(&request);
            local.w_axis.z = 1000.0;
            assert_eq!(
                native[..16],
                local.to_cols_array().map(f32::to_bits),
                "local index{index}"
            );
            local.w_axis.z = 0.0;
            let expected = multiply_transforms(
                multiply_transforms(parent, local),
                Mat4::from_translation(vec3(-f32::from(origin[0]), -f32::from(origin[1]), 0.0)),
            );
            assert_eq!(
                native[16..32],
                expected.to_cols_array().map(f32::to_bits),
                "origin index{index}"
            );
            assert_eq!(
                native[32],
                (local.w_axis.x * (1.0 / 960.0)).clamp(-1.0, 1.0).to_bits(),
                "voice pan index{index}"
            );
            poses += 1;
        }
    }
    println!(
        "GEROKASU2_PORTRAIT_NATIVE animations={animations} poses={poses} original_blink_mouth_properties_origin=verified"
    );
}

#[cfg(target_os = "macos")]
mod gpu;
