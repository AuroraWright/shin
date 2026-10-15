use super::*;
use crate::layer::PropertyPolicy;
use shin_core::time::{Easing, Ticks};

#[test]
fn ger_reconstruction_cache_consumes_first_generation_match_once_including_wrap() {
    let mut cache = [Some(0), Some(u32::MAX), None, Some(0), Some(7), Some(7)];
    for (generation, expected) in [
        (0, Some(0)),
        (7, Some(4)),
        (0, Some(3)),
        (u32::MAX, Some(1)),
        (7, Some(5)),
        (0, None),
        (7, None),
        (u32::MAX, None),
    ] {
        assert_eq!(consume_cache_index(&mut cache, generation), expected);
    }
    assert!(cache.into_iter().all(|entry| entry.is_none()));
}

#[test]
fn ger_reconstruction_resets_all_property_queues_but_preserves_owner_and_oscillators() {
    let mut properties = LayerProperties::with_policy(PropertyPolicy::Gerokasu2);
    let wobblers = std::array::from_fn(|i| (77 + i as u32, i as i32 % 4, i as f32 * 0.125));
    properties.reference_install_native_state(&[13.0; 90], &wobblers, None);
    properties.set_layer_id(LayerId::new_unchecked(731));
    properties.set_layerload_counter1(u32::MAX);
    for i in 0..90 {
        let mut tween = properties.property_tweener_mut(LayerProperty::from_usize(i));
        tween.gerokasu2_control(91.0, Ticks::from_f32(20.0), 0, Easing::Linear, false);
        tween.gerokasu2_control(31.0, Ticks::from_f32(11.0), 128, Easing::Linear, true);
        tween.update(Ticks::from_f32(3.0));
    }
    let before = properties.reference_copy_words().unwrap();
    let owner = properties.identity();
    let values = std::array::from_fn(|i| match i % 6 {
        0 => i32::MIN,
        1 => i32::MAX,
        2 => 16_777_217,
        3 => -16_777_217,
        4 => 0,
        _ => i as i32 * 9997 - 88_888,
    });
    apply_properties(&mut properties, values);
    assert_eq!(properties.identity(), owner);
    assert_eq!(properties.get_layer_id(), LayerId::new_unchecked(731));
    assert_eq!(properties.layer_load_counter(), u32::MAX);
    assert_eq!(
        &properties.reference_copy_words().unwrap()[90..],
        &before[90..]
    );
    for (i, value) in values.into_iter().enumerate() {
        let tween = properties.property_tweener(LayerProperty::from_usize(i));
        assert_eq!(tween.value().to_bits(), (value as f32).to_bits());
        assert_eq!(tween.target_value().to_bits(), (value as f32).to_bits());
        assert!(tween.is_idle());
        assert!(!tween.is_waiting());
    }
}

#[test]
fn ger_reconstruction_card_result_marks_complete_without_reselecting_or_resetting_tweens() {
    use crate::adv::gerokasu2::card_select::CardSelectModel;
    let mut card = CardSelectModel::new(15, 42).unwrap();
    card.fast_forward();
    card.phase = 2;
    card.listener = true;
    card.select(2);
    let before = format!("{card:?}");
    let selected = card.selected;
    card.restore_result(10101);
    assert_eq!(card.phase, -1);
    assert_eq!(card.selected, selected);
    assert_eq!(card.result(), Some(card.cards[selected].face as i32));
    card.phase = 2;
    assert_eq!(format!("{card:?}"), before);
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod native {
    use super::*;
    use std::{
        io::{Read, Write},
        path::{Path, PathBuf},
        process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    };
    struct Oracle {
        child: Child,
        input: ChildStdin,
        output: ChildStdout,
    }
    impl Oracle {
        fn new(name: &str) -> Self {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
            let folder = root.join("gerokasu2/.local/reconstruction");
            std::fs::create_dir_all(&folder).unwrap();
            let binary: PathBuf = folder.join(format!("native-reconstruction-{name}"));
            let compile = Command::new("clang")
                .args(["-std=c11", "-O2"])
                .arg(root.join(format!("gerokasu2/native-reconstruction-{name}.c")))
                .arg("-o")
                .arg(&binary)
                .output()
                .unwrap();
            assert!(
                compile.status.success(),
                "{}",
                String::from_utf8_lossy(&compile.stderr)
            );
            let mut child = Command::new(binary)
                .arg(root.join("gerokasu2/uncompressed_executable"))
                .arg(root.join("gerokasu2/.local/newrodin-medium.fnt"))
                .arg(root.join("gerokasu2/.local/system.fnt"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            let input = child.stdin.take().unwrap();
            let output = child.stdout.take().unwrap();
            Self {
                child,
                input,
                output,
            }
        }
        fn write(&mut self, words: &[u32]) {
            for word in words {
                self.input.write_all(&word.to_le_bytes()).unwrap();
            }
            self.input.flush().unwrap();
        }
        fn magic(&mut self, expected: &[u8; 4]) {
            let mut bytes = [0; 4];
            self.output.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, expected);
        }
        fn words<const N: usize>(&mut self) -> [u32; N] {
            std::array::from_fn(|_| {
                let mut bytes = [0; 4];
                self.output.read_exact(&mut bytes).unwrap();
                u32::from_le_bytes(bytes)
            })
        }
        fn finish(mut self) {
            drop(self.input);
            assert!(self.child.wait().unwrap().success());
        }
    }
    #[test]
    #[ignore = "original1B8D0 cache comparison/consumption; prepared RTTI and nil shared control blocks, CPU only"]
    fn ger_native_reconstruction_cache_generations_match_consumption() {
        let mut oracle = Oracle::new("cache");
        let mut cases = 0;
        let mut queries = 0;
        for seed in [0u32, 42, 0xdead_beef, u32::MAX] {
            for count in [0, 1, 3, 17, 48, 64] {
                let mut input = vec![count, 128];
                let mut cache = Vec::new();
                for index in 0..count {
                    let generation = match index % 4 {
                        0 => 0,
                        1 => u32::MAX,
                        2 => 7,
                        _ => seed.wrapping_add(index),
                    };
                    let eligibility = if index % 11 == 3 {
                        2
                    } else if index % 7 == 2 {
                        0
                    } else {
                        1
                    };
                    input.extend([generation, eligibility]);
                    cache.push((eligibility == 1).then_some(generation));
                }
                let requests: Vec<_> = (0..128)
                    .map(|index| match index % 4 {
                        0 => 0,
                        1 => u32::MAX,
                        2 => 7,
                        _ => seed.wrapping_add(index % count.max(1)),
                    })
                    .collect();
                input.extend(&requests);
                oracle.write(&input);
                oracle.magic(b"GRC1");
                for request in requests {
                    let actual = oracle.words::<1>()[0];
                    assert_eq!(
                        actual,
                        consume_cache_index(&mut cache, request).map_or(u32::MAX, |i| i as u32)
                    );
                    queries += 1;
                }
                cases += 1;
            }
        }
        oracle.finish();
        println!("Ger native reconstruction cache: {cases} cases / {queries} queries");
    }
    fn raw_value(seed: u32, id: u32, property: u32) -> i32 {
        seed.wrapping_mul(1664525)
            .wrapping_add((id + 1).wrapping_mul(1013904223))
            .wrapping_add(property.wrapping_mul(69621)) as i32
    }
    #[test]
    #[ignore = "whole original19CE0 publication with prepared scene/resources/UI/audio; property/ID/Card/Message bodies execute, CPU only"]
    fn ger_native_reconstruction_publication_restores_banks_and_message_then_retires_vm() {
        let mut oracle = Oracle::new("publication");
        let mut cases = 0;
        for seed in [0u32, 42, 0xdead_beef, u32::MAX] {
            for mode in 0..4 {
                for readiness in 0..3 {
                    for profile in 0..3 {
                        oracle.write(&[seed, mode, readiness, profile]);
                        oracle.magic(b"GRP1");
                        let header = oracle.words::<11>();
                        let live = readiness == 1;
                        let present = |bank: u32| {
                            profile == 1 || (profile == 2 && [0, 17, 47].contains(&bank))
                        };
                        let count = (0..48).filter(|bank| present(*bank)).count() as u32;
                        assert_eq!(
                            header,
                            if live {
                                [2, 1, 1, 1, 1, 1, 1, 2, count, u32::from(seed & 1 == 0), 0]
                            } else {
                                [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1]
                            }
                        );
                        for id in 0..56 {
                            let restored = live && (id < 8 || present(id - 8));
                            let h = oracle.words::<5>();
                            assert_eq!(h[0], id);
                            if live && id >= 8 && present(id - 8) {
                                assert_eq!(h[1], (id - 8).wrapping_add(seed.wrapping_mul(17)));
                                assert_eq!(h[2], seed ^ ((id - 7) * 31));
                            } else {
                                assert_eq!(&h[1..3], &[0x55667788, 0xaabbccdd]);
                            }
                            if id >= 8 {
                                let complete = live
                                    && present(id - 8)
                                    && (id - 8) % 3 == 0
                                    && (id - 8).wrapping_add(seed) % 2 != 0;
                                assert_eq!(h[3], if complete { u32::MAX } else { 2 });
                                assert_eq!(h[4], 3);
                            }
                            let mut properties =
                                LayerProperties::with_policy(PropertyPolicy::Gerokasu2);
                            let values = std::array::from_fn(|i| raw_value(seed, id, i as u32));
                            apply_properties(&mut properties, values);
                            for i in 0..90 {
                                let w = oracle.words::<4>();
                                assert_eq!(
                                    w,
                                    if restored {
                                        let tween = properties
                                            .property_tweener(LayerProperty::from_usize(i));
                                        let bits = tween.value().to_bits();
                                        assert!(tween.is_idle());
                                        assert_eq!(tween.target_value().to_bits(), bits);
                                        [bits, bits, 0, bits]
                                    } else {
                                        [0xc1200000, 0x41200000, 1, 0x7fc00000]
                                    },
                                    "seed{seed:x}/mode{mode}/ready{readiness}/profile{profile}/id{id}/property{i}"
                                );
                            }
                            assert_eq!(
                                oracle.words::<24>(),
                                std::array::from_fn(|i| 0xabc00000 + id * 24 + i as u32)
                            );
                        }
                        let message = oracle.words::<19>();
                        let clear = live && mode == 0;
                        let mut expected = if clear {
                            [
                                0,
                                0,
                                0,
                                0,
                                1.0f32.to_bits(),
                                259.0f32.to_bits(),
                                259.0f32.to_bits(),
                                0,
                                0,
                                0,
                                0x76543210,
                                0x12345678,
                                0x87,
                                19,
                                0x41780000,
                                0,
                                0,
                                0,
                                0,
                            ]
                        } else {
                            [
                                1, 1, 0x3f000000, 0x3e800000, 0x3e000000, 0x43fa0000, 0x43fa0000,
                                0x123, 1, 1, 0x76543210, 0x12345678, 0x87, 19, 0x41780000, 7, 3, 2,
                                1,
                            ]
                        };
                        // Assert the host clear against original scalar writes too.
                        if clear {
                            use crate::adv::gerokasu2::message_clock::{
                                MessageClock, MessageParams,
                            };
                            let mut clock = MessageClock::new(
                                &[],
                                &[],
                                glam::Vec2::ZERO,
                                MessageParams {
                                    flags: 0x76543210,
                                    mode: 0x123,
                                    message_id: 0x12345678,
                                    message_arg: 0x87,
                                    animate_open: true,
                                },
                            )
                            .unwrap();
                            clock.block_index = 19;
                            clock.time = 15.5;
                            clock.total_voices = 7;
                            clock.clear_for_reconstruction();
                            expected[4] = clock.modal_slide.to_bits();
                            expected[5] = clock.height.to_bits();
                            expected[6] = clock.height_target.to_bits();
                            expected[7] = clock.params.mode as u32;
                            expected[10] = clock.params.flags;
                            expected[11] = clock.params.message_id;
                            expected[12] = clock.params.message_arg as u32;
                            expected[13] = clock.block_index as u32;
                            expected[14] = clock.time.to_bits();
                            expected[15] = clock.total_voices;
                        }
                        assert_eq!(
                            message, expected,
                            "message seed{seed:x}/mode{mode}/ready{readiness}"
                        );
                        assert_eq!(
                            oracle.words::<4>(),
                            std::array::from_fn(|plane| if !live {
                                0
                            } else if (plane as u32).wrapping_add(seed) % 2 != 0 {
                                0x90u32.wrapping_add(plane as u32).wrapping_add(seed)
                            } else {
                                u32::MAX
                            })
                        );
                        assert_eq!(
                            oracle.words::<48>(),
                            std::array::from_fn(|bank| if live && present(bank as u32) {
                                (bank as u32).wrapping_add(seed) % 4 + 1
                            } else {
                                0
                            })
                        );
                        cases += 1;
                    }
                }
            }
        }
        oracle.finish();
        println!(
            "Ger native reconstruction publication: {cases} cases / {} property snapshots",
            cases * 56 * 90
        );
    }
}
