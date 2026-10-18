use super::*;

#[test]
fn ger_adv_bank_conversion_retains_all_header_parameter_property_and_interaction_words() {
    for seed in [0u32, 42, 0xdead_beef, u32::MAX] {
        let full = std::array::from_fn(|index| seed.wrapping_add(index as u32 * 641) as i32);
        let record = LayerRecord::from_adv_words(full);
        assert_eq!(record.adv_words(), full);
        assert_eq!(record.initialization_id(), full[3] as u32);
        assert_eq!(record.parameters(), &full[8..16]);
        for index in 0..90 {
            assert_eq!(
                record.property(LayerProperty::from_usize(index)),
                full[16 + index]
            );
        }
        assert_eq!(record.interaction_completed(), full[106] & 0xff != 0);
        assert_eq!(record.interaction_result(), full[107]);
    }
}

#[test]
fn ger_adv_bank_resource_generations_are_separate_from_property_reset_loads() {
    let mut layers = Gerokasu2Layers::default();
    let bank = layers.load(7, 2, 0, [0; 8]).unwrap()[0].layerbank;
    assert_eq!(
        (
            layers.record(bank).initialization_id(),
            layers.next_initialization_id(),
            layers.loads_with_init()
        ),
        (0, 1, 1)
    );
    layers.control(7, 0, 731, 0).unwrap();
    layers.load(7, 2, 2, [0; 8]).unwrap();
    assert_eq!(
        (layers.next_initialization_id(), layers.loads_with_init()),
        (1, 1)
    );
    assert_eq!(layers.record(bank).property(LayerProperty::TranslateX), 731);
    layers.load(7, 2, 0, [0; 8]).unwrap();
    assert_eq!(
        (layers.next_initialization_id(), layers.loads_with_init()),
        (1, 2)
    );
    layers.load(7, 2, 2, [11; 8]).unwrap();
    assert_eq!(
        (
            layers.record(bank).initialization_id(),
            layers.next_initialization_id(),
            layers.loads_with_init()
        ),
        (1, 2, 2)
    );
    layers.load(7, 9, 0, [0; 8]).unwrap();
    layers.complete_interaction(bank, 27);
    let saved = layers.clone();
    layers.load(7, 9, 2, [91; 8]).unwrap();
    assert_eq!(
        (
            layers.record(bank).initialization_id(),
            layers.next_initialization_id(),
            layers.loads_with_init()
        ),
        (2, 3, 3)
    );
    assert!(!layers.record(bank).interaction_completed());
    assert_eq!(layers.record(bank).interaction_result(), 27);
    assert!(saved.record(bank).interaction_completed());
    layers.unload(7).unwrap();
    let bank = layers.load(8, 2, 0, [3; 8]).unwrap()[0].layerbank;
    assert_eq!(
        (
            layers.record(bank).initialization_id(),
            layers.next_initialization_id(),
            layers.loads_with_init()
        ),
        (3, 4, 4)
    );
    layers.next_initialization_id = u32::MAX;
    layers.loads_with_init = u32::MAX;
    layers.load(8, 2, 0, [4; 8]).unwrap();
    assert_eq!(
        (
            layers.record(bank).initialization_id(),
            layers.next_initialization_id(),
            layers.loads_with_init()
        ),
        (u32::MAX, 0, 0)
    );
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original full ADV bank constructors and108-word/counter mapping; CPU only"]
fn ger_native_adv_banks_complete_headers_generations_properties_and_interactions() {
    use std::{
        fs,
        io::{Read, Write},
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/adv-banks");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-layer-adv-banks");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("reverse-engineering/gerokasu2/native-layer-adv-banks.c"))
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
        .arg(root.join("game-data/gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = child.stdout.take().unwrap();
    let mut state = Gerokasu2Layers::default();
    let mut snapshots = 0;
    let mut step = |op: u32, args: &[i32], state: &mut Gerokasu2Layers| {
        let mut row = [0u32; 12];
        row[0] = op;
        for (word, &value) in row[1..].iter_mut().zip(args) {
            *word = value as u32;
        }
        for word in row {
            input.write_all(&word.to_le_bytes()).unwrap();
        }
        input.flush().unwrap();
        match op {
            0 => *state = Default::default(),
            1 => {
                let params = args[3..11].try_into().unwrap();
                state.load(args[0], args[1], args[2], params).unwrap();
            }
            2 => {
                state.loads_with_init = args[0] as u32;
                state.next_initialization_id = args[1] as u32;
            }
            3 => state.select(args[0], args[1]),
            4 => state.select_plane(args[0]).unwrap(),
            5 => {
                let bank = args[0] as usize;
                let mut words = state.records[bank].adv_words();
                words[args[1] as usize] = args[2];
                state.records[bank] = LayerRecord::from_adv_words(words);
            }
            6 => {
                state.unload(args[0]).unwrap();
            }
            7 => state.swap(args[0], args[1]),
            8 => {
                state.clear_plane();
            }
            9 => {
                state.initialize(args[0]).unwrap();
            }
            _ => unreachable!(),
        }
        let mut header = [0; 4];
        output.read_exact(&mut header).unwrap();
        assert_eq!(&header, b"GBK1");
        let mut native = vec![0; 8 + 48 * 0x1b0];
        output.read_exact(&mut native).unwrap();
        assert_eq!(
            native,
            state.reference_adv_bank_bytes(),
            "ADV snapshot{snapshots},op{op},args{args:?}"
        );
        snapshots += 1;
    };
    for plane in 0..4 {
        for seed in [0i32, 1, -1] {
            for flags in [0, 2, 3, 0x102] {
                step(0, &[], &mut state);
                step(4, &[plane], &mut state);
                step(2, &[-1, seed], &mut state);
                step(1, &[7, 2, flags, 0, 1, 2, 3, 4, 5, 6, 7], &mut state);
                let bank = state.target(LayerId::new(7)).unwrap().layerbank.raw() as i32;
                for (word, value) in [
                    (5, 0x12345678),
                    (6, i32::MIN),
                    (7, i32::MAX),
                    (16, 731),
                    (105, -900),
                    (106, 0x7fffff01),
                    (107, -31),
                ] {
                    step(5, &[bank, word, value], &mut state);
                }
                step(1, &[7, 2, flags, 0, 1, 2, 3, 4, 5, 6, 7], &mut state);
                step(1, &[7, 2, flags, 0, 1, 2, 3, 4, 5, 6, 8], &mut state);
                step(
                    1,
                    &[7, 9, flags, 11, 12, 13, 14, 15, 16, 17, 18],
                    &mut state,
                );
                step(5, &[bank, 106, 0x12345601], &mut state);
                step(5, &[bank, 107, 31], &mut state);
                step(
                    1,
                    &[7, 9, flags, 21, 22, 23, 24, 25, 26, 27, 28],
                    &mut state,
                );
                step(9, &[7], &mut state);
                step(7, &[7, 9], &mut state);
                step(6, &[9, 0], &mut state);
                step(3, &[4, 2], &mut state);
                step(1, &[-6, 1, flags, 0, 0, 0, 0, 0, 0, 0, 0], &mut state);
                step(8, &[], &mut state);
                step(1, &[255, 6, flags, 1, 2, 3, 4, 5, 6, 7, 8], &mut state);
            }
        }
    }
    drop(step);
    drop(input);
    assert!(child.wait().unwrap().success());
    println!(
        "GEROKASU2_ADV_BANKS_NATIVE snapshots={snapshots} bytes={} banks=48 words=108 next_generation_and_reset_count=verified helper107_word_layout=preserved",
        snapshots * (8 + 48 * 0x1b0)
    );
}
