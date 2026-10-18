//! Execute the supplied Question bodies against the shared CPU owners and Ger
//! geometry. Font measurement, constructor services and GPU calls are bounded
//! contracts in these probes; this does not claim hardware rendering coverage.
use crate::adv::{
    backlog::rollback::{Confirmation, Outcome},
    dialog::input::DialogInput,
};
use crate::app::AppAction;
use enum_map::EnumMap;
use glam::{Vec2, vec2};
use shin_core::time::Ticks;
use shin_input::ActionState;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

fn run(source: &str, input: String) -> Vec<Vec<i64>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let folder = root.join("reverse-engineering/gerokasu2/question");
    std::fs::create_dir_all(&folder).unwrap();
    let binary = folder.join(source);
    assert!(
        Command::new("clang")
            .args(["-std=c11", "-O2"])
            .arg(root.join(format!("gerokasu2/{source}.c")))
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap()
            .success()
    );
    let executable = std::env::var_os("SHIN_GEROKASU2_EXECUTABLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("gerokasu2/uncompressed_executable"));
    let medium = std::env::var_os("SHIN_GEROKASU2_MEDIUM_FONT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"));
    let system = std::env::var_os("SHIN_GEROKASU2_SYSTEM_FONT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("test-fixtures/gerokasu2/system.fnt"));
    let mut child = Command::new(&binary)
        .args([executable, medium, system])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    std::fs::write(folder.join(format!("{source}.input")), input.as_bytes()).unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    std::fs::write(folder.join(format!("{source}.output")), &output.stdout).unwrap();
    std::fs::write(folder.join(format!("{source}.stderr")), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            line.split_whitespace()
                .skip(1)
                .map(|word| word.parse().unwrap())
                .collect()
        })
        .collect()
}

#[test]
#[ignore = "executes supplied original Ger Question constructor geometry; CPU only"]
fn ger_native_question_geometry_including_vertical_width_matches() {
    let mut input = String::new();
    let mut expected = Vec::new();
    for count in [1, 2] {
        for width in [1., 599., 600., 601., 800., 1446.] {
            for (a, b) in [
                (vec2(80., 50.), vec2(120., 50.)),
                (vec2(480., 50.), vec2(300., 90.)),
                (vec2(720., 85.), vec2(1050., 35.)),
                (vec2(0., 0.), vec2(0., 0.)),
            ] {
                let prompt = vec2(width, 110.);
                input.push_str(&format!(
                    "{count} {width} 110 {} {} {} {}\n",
                    a.x, a.y, b.x, b.y
                ));
                let answers = [a, b];
                let g = super::config::dialog::geometry(prompt, &answers[..count]);
                let mut row = vec![count as i64];
                row.extend(
                    [
                        g.origin.x, g.origin.y, g.outer.x, g.outer.y, g.inner.x, g.inner.y,
                    ]
                    .map(|v| i64::from(v.to_bits())),
                );
                row.push(i64::from(g.vertical));
                for button in &g.buttons[..count] {
                    row.extend(button.to_array().map(|v| i64::from(v.to_bits())));
                }
                expected.push(row);
            }
        }
    }
    let actual = run("native-question-geometry", input);
    assert_eq!(actual, expected);
    println!(
        "GEROKASU2_QUESTION_GEOMETRY cases={} exact_original_constructor_rectangles=verified measured_text_GPU_boundaries=explicit",
        expected.len()
    );
}

fn outcome(value: Outcome) -> i64 {
    match value {
        Outcome::Yes => 0,
        Outcome::No => 1,
        Outcome::Cancel => -1,
    }
}
fn snapshot(model: &mut Confirmation) -> Vec<i64> {
    let mut row = vec![model.selected().map_or(-1, |v| v as i64)];
    row.extend([model.scale.value(), model.opacity.value()].map(|v| i64::from(v.to_bits())));
    for i in 0..model.button_count() {
        row.extend(
            [model.focus[i].value(), model.flashes[i].value()].map(|v| i64::from(v.to_bits())),
        );
    }
    let callbacks = [model.take_intermediate_outcome(), model.take_outcome()];
    row.push(callbacks.iter().filter(|v| v.is_some()).count() as i64);
    for (i, callback) in callbacks.into_iter().enumerate() {
        if let Some(callback) = callback {
            row.extend([i as i64, outcome(callback)]);
        }
    }
    row
}

#[test]
#[ignore = "executes supplied original Ger Question controller, callbacks and tweens; CPU only"]
fn ger_native_question_open_flash_close_and_final_callback_match_shared_owner() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut cases = 0;
    for count in [1, 2] {
        for selected in 0..count {
            for cancel in [false, true] {
                for decision in 0..3 {
                    for dt in [0.5, 1., 2.5, 100.] {
                        input.push_str(&format!("N {count} {selected} {}\n", u8::from(cancel)));
                        let mut model = Confirmation::new(count, selected, cancel);
                        model.set_deferred_cancel(true);
                        let mut keys = DialogInput::default();
                        expected.push((count, snapshot(&mut model)));
                        cases += 1;
                        let mut accepted = false;
                        let mut moved = false;
                        for _ in 0..150 {
                            let mut actions: EnumMap<AppAction, ActionState> = EnumMap::default();
                            let mut pressed = 0u32;
                            let mut repeated = 0u32;
                            if model.is_open() && !accepted {
                                let action = if decision == 2 && cancel {
                                    accepted = true;
                                    pressed = 1 << 29;
                                    AppAction::Cancel
                                } else if decision == 1 && !moved {
                                    moved = true;
                                    repeated = 1 << 26;
                                    AppAction::Left
                                } else {
                                    accepted = true;
                                    pressed = 1 << 28;
                                    AppAction::Enter
                                };
                                actions[action].is_clicked = true;
                                actions[action].is_clicked_or_repeated = true;
                            }
                            keys.keyboard(&mut model, false, &actions);
                            model.update(Ticks::from_f32(dt));
                            input.push_str(&format!("F 1 {pressed:x} {repeated:x} {dt}\n"));
                            expected.push((count, snapshot(&mut model)));
                        }
                    }
                }
            }
        }
    }
    let actual = run("native-question-model", input);
    assert_eq!(actual.len(), expected.len());
    let mut max_difference = 0f32;
    for (index, (actual, (count, expected))) in actual.iter().zip(&expected).enumerate() {
        let floats = 2 + 2 * count;
        assert_eq!(actual.len(), expected.len(), "snapshot{index}");
        for (word, (&a, &e)) in actual.iter().zip(expected).enumerate() {
            if (1..=floats).contains(&word) {
                let difference = (f32::from_bits(a as u32) - f32::from_bits(e as u32)).abs();
                max_difference = max_difference.max(difference);
                assert!(
                    difference <= 0.000003,
                    "snapshot{index} word{word}: native{} Rust{}",
                    f32::from_bits(a as u32),
                    f32::from_bits(e as u32)
                );
            } else {
                assert_eq!(a, e, "snapshot{index} word{word}");
            }
        }
    }
    println!(
        "GEROKASU2_QUESTION_CLOCK cases={cases} snapshots={} callbacks_selection=exact tween_max_abs={max_difference}",
        actual.len()
    );
}

#[test]
#[ignore = "executes supplied original Ger Question button hit function; CPU only"]
fn ger_native_question_hit_inset_uses_thirty_pixels_on_both_axes() {
    let mut input = String::from("N 2 1 1\n");
    let mut expected = vec![vec![1, 1056964608, 0, 0, 0, 1065353216, 0, 0]];
    for x in [129., 130., 309., 310., 339., 340., 519., 520.] {
        for y in [109., 110., 173., 174.] {
            input.push_str(&format!("H {x} {y}\n"));
            let hit = if (110. ..174.).contains(&y) {
                if (130. ..310.).contains(&x) {
                    0
                } else if (340. ..520.).contains(&x) {
                    1
                } else {
                    -1
                }
            } else {
                -1
            };
            expected.push(vec![hit]);
        }
    }
    assert_eq!(run("native-question-model", input), expected);
}
