//! Differential proof against supplied updated native callback, plus input gates.
use crate::adv::GlobalData;
use shin_core::format::save::{PersistData, SaveData};

#[test]
#[ignore = "requires supplied updated executable (SHIN_REFERENCE_ROM) and AArch64 native execution"]
fn reference_updated_native_scenario_unlock_callback() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let root = std::path::PathBuf::from(std::env::var_os("SHIN_REFERENCE_ROM").unwrap());
    let executable = root
        .parent()
        .unwrap()
        .join("uncompressed_executable_update");
    let oracle = std::env::temp_dir().join(format!("shin-native-unlock-{}", std::process::id()));
    assert!(
        Command::new("clang")
            .args([
                "-O0",
                "-Wall",
                "-Wextra",
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../reverse-engineering/native-scenario-unlock.c"
                ),
                "-o"
            ])
            .arg(&oracle)
            .status()
            .unwrap()
            .success()
    );
    let mut cases = Vec::new();
    // Every stored 16-bit value, including signed negatives and zero. Native
    // tests low16 for zero rather than treating negative flags as locked zero.
    for bits in 0..=u16::MAX {
        cases.push((1i32, 0i32, [bits; 64]));
    }
    let mut random = 0x31415926u32;
    for _ in 0..4096 {
        let mut slots = [0u16; 64];
        for slot in &mut slots {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            *slot = if random & 3 == 0 {
                0
            } else {
                (random >> 16) as u16
            };
        }
        cases.push((1, 0, slots));
    }
    for flag in [0, 1, 2, -1, i32::MIN, i32::MAX] {
        for button in [-1, 0, 1] {
            cases.push((flag, button, [0; 64]));
        }
    }
    let mut input = Vec::with_capacity(cases.len() * 136);
    for (flag, button, slots) in &cases {
        input.extend(flag.to_le_bytes());
        input.extend(button.to_le_bytes());
        for slot in slots {
            input.extend(slot.to_le_bytes());
        }
    }
    let mut child = Command::new(&oracle)
        .arg(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), cases.len() * 132);
    for ((flag, button, slots), native) in cases.iter().zip(output.stdout.chunks_exact(132)) {
        let mut save = SaveData::default();
        save.persist_data = PersistData(slots.iter().map(|&n| n as i16).collect());
        let mut globals = GlobalData::open(None, Some(save)).unwrap();
        let accepted = *flag == 1 && *button == 0;
        if accepted {
            globals.unlock_main_scenarios();
        }
        for slot in 0..64 {
            let actual = u16::from_le_bytes(native[slot * 2..slot * 2 + 2].try_into().unwrap());
            assert_eq!(
                actual,
                globals.save().persist_data.get(slot) as u16,
                "slot{slot}, flag{flag}, button{button}, input{:x}",
                slots[slot]
            );
        }
        assert_eq!(
            u32::from_le_bytes(native[128..132].try_into().unwrap()),
            u32::from(accepted)
        );
    }
    std::fs::remove_file(oracle).unwrap();
    println!(
        "Updated native scenario unlock: {} callback cases, 64 persistent slots each, exact final/Yes gate, slot0=24, zero-only chapter writes and one queued picker",
        cases.len()
    );
}

#[test]
fn updated_unlock_input_priority_and_dialog_gate() {
    use crate::title::{
        scenario_select::ScenarioSelectSetup,
        scenario_select::selection::{ScenarioSelection, SelectionEvent},
    };
    use crate::app::AppAction;
    use enum_map::EnumMap;
    use shin_input::ActionState;
    let press = |actions: &[AppAction]| {
        let mut state = EnumMap::default();
        for &action in actions {
            state[action] = ActionState {
                is_clicked: true,
                is_clicked_or_repeated: true,
                ..Default::default()
            };
        }
        state
    };
    let mut selection =
        ScenarioSelection::new_active(ScenarioSelectSetup::from_save(&SaveData::default()));
    selection.controller_input(&press(&[AppAction::UnlockScenarios]));
    assert!(selection.take_events().is_empty()); // original or already unlocked
    selection.set_unlock_available(true);
    selection.controller_input(&press(&[AppAction::AnyDown, AppAction::UnlockScenarios]));
    assert_eq!(
        selection.take_events(),
        [SelectionEvent::CaptureDetails, SelectionEvent::MoveSound]
    );
    selection.controller_input(&press(&[AppAction::UnlockScenarios]));
    assert_eq!(
        selection.take_events(),
        [SelectionEvent::ConfirmSound, SelectionEvent::UnlockDialog]
    );
    selection.controller_input(&press(&[AppAction::Enter, AppAction::UnlockScenarios]));
    assert!(selection.take_events().is_empty());
    selection.answer_dialog(true); // start-dialog response cannot accept unlock
    assert!(selection.take_events().is_empty());
    selection.answer_unlock_dialog(false);
    assert!(selection.accepts_input());
    selection.controller_input(&press(&[AppAction::UnlockScenarios]));
    selection.take_events();
    selection.answer_unlock_dialog(true);
    assert_eq!(selection.take_events(), [SelectionEvent::UnlockScenarios]);
    assert!(!selection.accepts_input());
}

#[test]
#[ignore = "requires supplied updated executable next to SHIN_REFERENCE_ROM"]
fn reference_updated_native_unlock_input_prompt_and_footer() {
    let rom = std::path::PathBuf::from(std::env::var_os("SHIN_REFERENCE_ROM").unwrap());
    let exe = std::fs::read(rom.parent().unwrap().join("uncompressed_executable_update")).unwrap();
    let word =
        |address: usize| u32::from_le_bytes(exe[256 + address..260 + address].try_into().unwrap());
    // Constructor snapshots slot25==0. The active task routes movement first,
    // then Cancel, Confirm and pressed Plus, guarded by that snapshot.
    for (a, w) in [
        (0xdcbd8, 0x52800321),
        (0xdcbe0, 0x97ffd1dc),
        (0xdcbe8, 0x72003c1f),
        (0xdcbf0, 0x1a9f17e9),
        (0xdcbf4, 0x3905a149),
        (0xdf568, 0xb858c3a8),
        (0xdf56c, 0x37e804e8),
        (0xdf570, 0x37e005e8),
        (0xdf574, 0x36400608),
        (0xdf578, 0x3945a268),
        (0xdf57c, 0x340005c8),
        (0xdf590, 0xd0000401),
        (0xdf594, 0x91067021),
        (0xdf5a0, 0x320003e1),
        (0xdf5a4, 0x97fd0b4b),
        (0xde2ec, 0x3945a268),
        (0xde2f0, 0x340000c8),
        (0xde300, 0x321e03e2),
        (0xdf114, 0xb00004b7),
        (0xdf118, 0x9126a2f7),
        (0xdf180, 0xbd464103),
        (0xe3068, 0x7100051f),
        (0xe306c, 0x7a400920),
        (0xe3070, 0x540020c1),
    ] {
        assert_eq!(word(a), w, "updated native instruction{a:x}");
    }
    let prompt = super::scene::SCENARIO_UNLOCK_PROMPT.as_bytes();
    assert_eq!(&exe[256 + 0x16119c..256 + 0x16119c + prompt.len()], prompt);
    assert_eq!(exe[256 + 0x16119c + prompt.len()], 0);
    for (i, row) in [
        [1i32, -1, 0, 206],
        [4, -1, 48, 72],
        [7, -1, 96, 236],
        [3, -1, 144, 74],
        [14, -1, 192, 134],
    ]
    .iter()
    .enumerate()
    {
        for (j, &n) in row.iter().enumerate() {
            assert_eq!(word(0x1749a8 + i * 16 + j * 4), n as u32);
        }
    }
    assert_eq!(word(0x15a424), 320.0f32.to_bits());
    assert_eq!(word(0x15a640), 240.0f32.to_bits());
    assert_eq!(word(0x15a954), 0.2f32.to_bits());
    println!(
        "Updated picker: slot25 snapshot, Plus bit8 input priority, exact warning/default No, final Yes gate and five native footer rows verified"
    );
}
