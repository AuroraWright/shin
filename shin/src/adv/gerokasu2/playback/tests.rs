use super::*;
#[test]
fn ger_playback_current_read_is_snapshot_modes_independent_and_ctrl_temporary() {
    let mut p = Playback::default();
    p.enter_message(31);
    assert!(!p.was_read);
    p.complete_message(31);
    assert!(
        !p.was_read,
        "Current message retains its original read classification"
    );
    p.enter_message(31);
    assert!(p.was_read);
    p.input(true, true, false);
    assert!(p.modes.autoplay && !p.modes.skip);
    p.input(false, true, false);
    assert!(p.modes.autoplay && p.modes.skip);
    assert!(p.autoplay(-1, false) && p.skip(-1, false));
    for event in [0, 1, i32::MAX] {
        assert!(!p.autoplay(event, false) && !p.skip(event, false));
    }
    assert!(!p.autoplay(-1, true) && !p.skip(-1, true));
    p.system_call(1, 0);
    assert!(p.system_controls && p.modes.skip);
    p.system_call(0, 0);
    assert!(!p.system_controls && !p.modes.skip && !p.modes.autoplay);
    p.system_call(0, -1);
    assert!(p.system_controls);
    p.held_skip = true;
    assert!(p.skip(-1, false));
    assert!(!p.modes.skip);
    p.enter_message(32);
    assert!(!p.skip(-1, false));
    p.disallow_skip_unread = false;
    assert!(p.skip(-1, false));
    p.hidden_ui = true;
    assert!(!p.skip(-1, false));
    let size = p.seen_messages.size_in_words();
    for id in [u32::MAX, i32::MIN as u32] {
        p.enter_message(id);
        assert!(p.was_read);
        p.complete_message(id);
    }
    assert_eq!(p.seen_messages.size_in_words(), size);
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "executes original Ger playback/SYS/read bodies;CPU only"]
fn ger_native_playback_predicates_sys_permission_and_message_read_growth() {
    use std::{
        fs,
        io::{Read, Write},
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/playback");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-playback");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-playback.c"))
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
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"),
            root.join("test-fixtures/gerokasu2/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut p = Playback::default();
    let mut sounds = 0;
    let mut snapshots = 0;
    let mut bytes = 0;
    let mut compare = |row: [u32; 12], p: &Playback, result: u32, read: u32, sounds: u32| {
        input
            .write_all(
                &row.into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let mut expected = b"GPB1".to_vec();
        for w in [
            result,
            read,
            u32::from(p.system_controls),
            u32::from(p.modes.autoplay) | (u32::from(p.modes.skip) << 1),
            sounds,
            p.seen_messages.size_in_words() as u32,
        ] {
            expected.extend(w.to_le_bytes());
        }
        let mut words = vec![0u32; p.seen_messages.size_in_words()];
        for id in p.seen_messages.iter() {
            words[id / 32] |= 1 << (id & 31);
        }
        expected.extend(words.into_iter().flat_map(u32::to_le_bytes));
        let mut actual = vec![0; expected.len()];
        output.read_exact(&mut actual).unwrap();
        assert_eq!(actual, expected, "native snapshot{snapshots},row{row:?}");
        snapshots += 1;
        bytes += actual.len();
    };
    compare([0; 12], &p, 0, 0, 0);
    for vm_present in [false, true] {
        for hidden in [false, true] {
            for system in [false, true] {
                for event in [-1, i32::MIN, 0, 17] {
                    for flags in 0..4 {
                        for disallow in [false, true] {
                            for was_read in [false, true] {
                                for overlay in 0..5 {
                                    p.system_controls = system;
                                    p.hidden_ui = hidden;
                                    p.modes.autoplay = flags & 1 != 0;
                                    p.modes.skip = flags & 2 != 0;
                                    p.disallow_skip_unread = disallow;
                                    p.was_read = was_read;
                                    let present = overlay != 0;
                                    let active = overlay & 1 != 0;
                                    let waiting = overlay & 2 != 0;
                                    let gate = present && (!active || waiting);
                                    let auto = vm_present && p.autoplay(event, gate);
                                    let skip = vm_present && p.skip(event, gate);
                                    compare(
                                        [
                                            1,
                                            hidden as u32,
                                            system as u32,
                                            event as u32,
                                            flags,
                                            disallow as u32,
                                            was_read as u32,
                                            present as u32,
                                            active as u32,
                                            waiting as u32,
                                            vm_present as u32,
                                            0,
                                        ],
                                        &p,
                                        auto as u32,
                                        skip as u32,
                                        sounds,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    for id in [0, 1, -1, i32::MIN, i32::MAX] {
        for value in [0, 1, -1, i32::MIN, i32::MAX] {
            p.system_call(id, value);
            compare(
                [2, id as u32, value as u32, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                &p,
                1,
                0,
                sounds,
            );
        }
    }
    for vm_present in [false, true] {
        for id in [
            0,
            31,
            32,
            8191,
            8192,
            16384,
            65535,
            u32::MAX,
            i32::MIN as u32,
        ] {
            for _ in 0..2 {
                if vm_present {
                    p.complete_message(id);
                    sounds += 1;
                }
                compare(
                    [3, id, vm_present as u32, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                    &p,
                    0,
                    p.seen_messages.contains(id as usize) as u32,
                    sounds,
                );
            }
        }
    }
    for id in [
        0,
        31,
        32,
        8191,
        8192,
        16384,
        65535,
        65534,
        u32::MAX,
        i32::MIN as u32,
    ] {
        for event in [-1, 0, 17] {
            p.enter_message(id);
            compare(
                [6, id, event as u32, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                &p,
                u32::from(p.was_read),
                0,
                sounds,
            );
        }
    }
    for flags in 0..4 {
        for system in [false, true] {
            for hidden in [false, true] {
                for bits in 0..8 {
                    p.modes.autoplay = flags & 1 != 0;
                    p.modes.skip = flags & 2 != 0;
                    p.system_controls = system;
                    p.hidden_ui = hidden;
                    p.was_read = true;
                    p.disallow_skip_unread = true;
                    compare(
                        [
                            1,
                            hidden as u32,
                            system as u32,
                            0,
                            flags,
                            1,
                            1,
                            0,
                            0,
                            0,
                            1,
                            0,
                        ],
                        &p,
                        0,
                        0,
                        sounds,
                    );
                    p.input(bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
                    compare(
                        [
                            4,
                            (bits & 1 != 0) as u32,
                            (bits & 2 != 0) as u32,
                            (bits & 4 != 0) as u32,
                            system as u32,
                            hidden as u32,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                        ],
                        &p,
                        0,
                        0,
                        sounds,
                    );
                }
            }
        }
    }
    p = Playback::default();
    compare([0; 12], &p, 0, 0, 0);
    // Complete original SaveGame initializer: skip unread defaults to true;
    // its adjacent message-speed byte is50. Other save fields are not claimed.
    compare(
        [5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        &p,
        p.disallow_skip_unread as u32,
        50,
        0,
    );
    drop(compare);
    drop(input);
    assert!(child.wait().unwrap().success());
    println!(
        "GEROKASU2_PLAYBACK_NATIVE_MATCH snapshots={snapshots} bytes={bytes} original_predicates_SYS_permission_modes_input_completion_read_growth_MSGSET_defaults=verified prepared_graph_cast_sound=bounded"
    );
}
