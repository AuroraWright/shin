use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
#[ignore = "actual1c804 TIPS prompt/defaults and20844 staged callback over signed button/phase boundaries;no GPU/audio/window/saves"]
fn higurashi_system_menu_tips_prompt_and_staged_return_match_native() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for id in [1000, 1001, 1999, 11000, 11001, 11999] {
        for active in [0, 1] {
            for stage in [0, 1, 2, u32::MAX] {
                for button in [-2, -1, 0, 1, 2, i32::MIN, i32::MAX] {
                    input += &format!("N {id} {active}\nC {stage} {}\nD\n", button as u32);
                    let text = |value: &str| {
                        format!(
                            "{} {}",
                            value.len(),
                            value
                                .bytes()
                                .map(|v| v.to_string())
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                    };
                    expected.push(format!("N 1 1 {} {}", text(PROMPT), text("はい|いいえ")));
                    let action = action(question::Callback { stage, button });
                    expected.push(format!(
                        "C {} {} 0",
                        u32::from(action == Action::Reopen),
                        u32::from(action == Action::Return)
                    ));
                    expected.push("D".into());
                    owners += 1;
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_MENU_TIPS_RETURN").unwrap())
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
    let sent = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(sent.as_bytes()));
    let output = child.wait_with_output().unwrap();
    let sent = writer.join().unwrap();
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_MENU_TIPS_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in [
            ("input.txt", input.as_bytes()),
            ("output.txt", output.stdout.as_slice()),
            ("stderr.txt", output.stderr.as_slice()),
        ] {
            std::fs::write(std::path::Path::new(&dir).join(name), bytes).unwrap();
        }
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    sent.unwrap();
    let output = String::from_utf8(output.stdout).unwrap();
    let rows = output.lines().collect::<Vec<_>>();
    assert_eq!(rows.len(), expected.len());
    for (i, (actual, expected)) in rows.iter().zip(&expected).enumerate() {
        assert_eq!(*actual, expected, "menu TIPS observation{i}");
    }
    eprintln!(
        "Menu TIPS:{owners} native owners/{} exact observations;actual Japanese prompt/No default/cancel-enabled,stage0 rejection reopen and stage1 Yes15c64(unlock0) routing pass",
        rows.len()
    );
}
