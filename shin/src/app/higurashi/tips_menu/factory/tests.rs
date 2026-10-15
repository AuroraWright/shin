use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

fn words(length: usize) -> Vec<u32> {
    (0..length)
        .map(|i| (i as u32 * 0x12345) ^ 0x5a5a5a5a)
        .collect()
}
fn vector(out: &mut String, words: &[u32]) {
    write!(out, " {}", words.len()).unwrap();
    for value in words {
        write!(out, " {value}").unwrap();
    }
}
fn data(out: &mut String, data: &RuntimeGameData) {
    write!(
        out,
        " {} {}",
        data.progress.active_index(),
        data.progress.extra_word
    )
    .unwrap();
    for e in &data.progress.entries {
        for value in [
            e.scenario_id as u32,
            e.random_seed,
            e.save_position,
            e.replay_target,
        ] {
            write!(out, " {value}").unwrap();
        }
        write!(out, " {}", e.response_data.len()).unwrap();
        for value in &e.response_data {
            write!(out, " {value}").unwrap();
        }
    }
    write!(out, " {}", data.trailing_values.len()).unwrap();
    for value in &data.trailing_values {
        write!(out, " {value}").unwrap();
    }
    write!(out, " {}", data.byte_variables.len()).unwrap();
    for value in &data.byte_variables {
        write!(out, " {value}").unwrap();
    }
}
#[test]
#[ignore = "actual15c64 completion lookup/bit-vector growth/active record switch/full dcf30 and post-save18268/dcf00 factory arguments"]
fn higurashi_tips_exit_lookup_completion_and_full_factory_payloads_match_native() {
    let snr = HigurashiScenario::new(
        std::fs::read(std::env::var("SHIN_HIGURASHI_SNR").unwrap())
            .unwrap()
            .into(),
    )
    .unwrap();
    let mut profiles = vec![];
    for tip in &snr.tips_info {
        for active in [0, 1] {
            profiles.push((i32::from(tip.updated_field.unwrap()), active, 0, 1));
        }
    }
    for index in [0, 15, 16, 31, 187, 188, 244, 314] {
        for active in [0, 1] {
            for length in [0, 1, 20, 257] {
                for unlock in [0, 1, 2, u32::MAX] {
                    profiles.push((
                        i32::from(snr.tips_info[index].updated_field.unwrap()),
                        active,
                        length,
                        unlock,
                    ));
                }
            }
        }
    }
    for id in [-1, 0, 9999, 10000, 65535, i32::MAX] {
        for active in [0, 1] {
            for length in [0, 1, 257] {
                profiles.push((id, active, length, 1));
            }
        }
    }
    let mut input = String::new();
    let mut expected = vec![];
    for &(id, active, length, unlock) in &profiles {
        let source = super::super::tests::source(id, active);
        let before = source.clone();
        let mut vectors = SaveVectors::default();
        vectors.extra_words_0 = words(length);
        let returned = from_exit(&source, &snr, &mut vectors, unlock).unwrap();
        assert_eq!(source, before);
        if let Some(copy) = &returned.request.data {
            assert_eq!(copy.date, source.date);
            assert_eq!(copy.progress.entries, source.progress.entries);
            assert_eq!(copy.progress.extra_word, source.progress.extra_word);
            assert_eq!(copy.trailing_values, source.trailing_values);
            assert_eq!(copy.byte_variables, source.byte_variables);
            assert_ne!(
                copy.progress.entries[1].response_data.as_ptr(),
                source.progress.entries[1].response_data.as_ptr()
            );
        }
        writeln!(input, "N {} {active} {length} {unlock}", id as u32).unwrap();
        let save = vectors.extra_words_0.clone();
        let snapshot = |actual: &[u32], kind: u32| {
            let mut out = format!(
                "R {} 1 {kind} {} {} 0",
                u32::from(returned.save),
                if kind == 0 {
                    0
                } else {
                    returned.request.selected
                },
                if kind == 0 {
                    0
                } else {
                    returned.request.alternate
                }
            );
            vector(&mut out, actual);
            vector(&mut out, if returned.save { &save } else { &[] });
            if let Some(d) = &returned.request.data {
                data(&mut out, d);
            }
            out
        };
        expected.push(snapshot(
            &vectors.extra_words_0,
            if returned.save { 0 } else { 1 },
        ));
        if returned.save {
            input.push_str("M\n");
            if let Some(word) = vectors.extra_words_0.first_mut() {
                *word ^= u32::MAX;
            }
            expected.push(snapshot(&vectors.extra_words_0, 0));
            input.push_str("C\n");
            expected.push(snapshot(&vectors.extra_words_0, 2));
        }
        input.push_str("D\n");
        expected.push("D".into());
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_TIPS_RETURN").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
            std::env::var("SHIN_HIGURASHI_SNR").unwrap(),
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
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_TIPS_RETURN_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        for (name, data) in [
            ("input.txt", input.as_bytes()),
            ("output.txt", output.stdout.as_slice()),
            ("stderr.txt", output.stderr.as_slice()),
        ] {
            std::fs::write(std::path::Path::new(&dir).join(name), data).unwrap();
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
    for (i, (a, e)) in rows.iter().zip(&expected).enumerate() {
        assert_eq!(*a, e, "TIPS return native row{i}");
    }
    eprintln!(
        "TIPS return:{} original owners/{} exact observations;all315 scenario indexes,unknown signed IDs,odd-only completion bits/256-word growth,full active1-to0 copies and frozen post-save dcf00(index,alternate) callbacks pass",
        profiles.len(),
        rows.len()
    );
}
