use super::*;
use crate::adv::higurashi::save_game::{FutureServices, Model, Predicate, Services};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn bytes(n: u32, salt: u32, len: u32) -> Vec<u8> {
    (0..len)
        .map(|i| ((n * 19 + salt * 7 + i * 13) & 255) as u8)
        .collect()
}
fn profile(base: SaveData, n: u32) -> (SaveData, Slots) {
    let mut slots = Slots::loaded(&base);
    for i in 0..101u32 {
        let slot = if i == 0 {
            &mut slots.auto
        } else {
            &mut slots.manual[i as usize - 1]
        };
        let active = slot.data.progress.active_index();
        if slot.valid {
            for e in 0..=active {
                let record = &mut slot.data.progress.entries[e];
                record.replay_target = 0xdeadbeef;
                record.response_data = bytes(n, i + e as u32, (n + i + e as u32) % 9);
            }
            slot.data.trailing_values = (0..(n + i) % 7)
                .map(|j| ((n * 37 + i * 11 + j * 71) & 1023) as u16)
                .collect();
            slot.data.byte_variables = bytes(n, i, (n * 3 + i) % 11);
        }
        slot.data.progress.extra_word = 0xdead0000 + ((n + i) & 7);
        if active == 0 {
            slot.data.progress.entries[1] = ProgressEntry {
                scenario_id: 2084 + i as i32,
                random_seed: 0x87654321 + i,
                save_position: 17 + i,
                replay_target: 19 + i,
                response_data: bytes(n, i + 1, (n + i) % 9),
            };
        }
    }
    (base, slots)
}
fn hash(base: &SaveData, slots: &Slots) -> u64 {
    let mut bytes = vec![base.save_menu_position];
    bytes.extend(base.play_seconds.to_le_bytes());
    bytes.extend(base.extra_word.to_le_bytes());
    fn vector(bytes: &mut Vec<u8>, data: Vec<u8>, width: usize) {
        bytes.extend((data.len() as u64 / width as u64).to_le_bytes());
        bytes.extend(data);
    }
    vector(
        &mut bytes,
        base.persist_data
            .0
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect(),
        2,
    );
    let v = &base.save_vectors;
    for (words, small) in [
        (&v.seen_messages, None),
        (&v.select_seen_choices, None),
        (&vec![], Some(&v.select_last_choices)),
        (&v.unlocked_cgs, None),
        (&v.unlocked_bgms, None),
        (&v.unlocked_movies, None),
        (&v.extra_words_0, None),
        (&v.extra_words_1, None),
    ] {
        if let Some(small) = small {
            vector(&mut bytes, small.clone(), 1);
        } else {
            vector(
                &mut bytes,
                words.iter().flat_map(|v| v.to_le_bytes()).collect(),
                4,
            );
        }
    }
    let s = &base.settings.common;
    bytes.extend([
        s.v0_bgmvol,
        s.v1_sfxvol,
        s.v2_voicevol,
        s.v3_sysvol,
        s.v4_voicefocus as u8,
        s.v5_voicepanapot as u8,
        s.v6 as u8,
        s.v7,
        s.v8,
        s.v9_msgspeed,
        s.v10_skipspeed,
        s.v11_disallowskipunread as u8,
        s.v12 as u8,
        s.v13_msgwinalpha,
        s.v14_showroutenavi as u8,
        s.v15 as u8,
        s.v16_showtoucheffect as u8,
        s.v17_showscenetitle as u8,
        s.v18_showsongtitle as u8,
    ]);
    bytes.extend(s.v19.to_le_bytes());
    bytes.extend([
        base.settings.extra_flag as u8,
        base.settings.extra_percentage,
    ]);
    for slot in std::iter::once(&slots.auto).chain(&slots.manual) {
        let d = &slot.data;
        bytes.push(slot.valid as u8);
        bytes.extend(d.date.year.to_le_bytes());
        bytes.extend([
            d.date.month,
            d.date.day,
            d.date.hour,
            d.date.minute,
            d.date.second,
        ]);
        bytes.extend((d.progress.active_index() as u32).to_le_bytes());
        bytes.extend(d.progress.extra_word.to_le_bytes());
        for e in &d.progress.entries {
            bytes.extend(e.scenario_id.to_le_bytes());
            bytes.extend(e.random_seed.to_le_bytes());
            bytes.extend(e.save_position.to_le_bytes());
            bytes.extend(e.replay_target.to_le_bytes());
            vector(&mut bytes, e.response_data.clone(), 1);
        }
        vector(
            &mut bytes,
            d.trailing_values
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect(),
            2,
        );
        vector(&mut bytes, d.byte_variables.clone(), 1);
    }
    bytes.iter().fold(0xcbf29ce484222325, |h, &v| {
        (h ^ u64::from(v)).wrapping_mul(0x100000001b3)
    })
}
#[derive(Default)]
struct State {
    children: u32,
    polls: u32,
    notices: u32,
    starts: u32,
    callbacks: u32,
    releases: u32,
    status: u32,
}
impl Services for State {
    fn notification(&mut self) -> anyhow::Result<()> {
        self.notices += 1;
        self.children += 1;
        Ok(())
    }
    fn completed(&mut self) -> anyhow::Result<()> {
        self.callbacks += 1;
        Ok(())
    }
    fn children(&mut self) -> anyhow::Result<()> {
        self.polls += 1;
        Ok(())
    }
}
impl FutureServices for State {
    fn start(&mut self) -> anyhow::Result<()> {
        self.starts += 1;
        self.releases += 1;
        Ok(())
    }
    fn status(&mut self) -> anyhow::Result<u32> {
        Ok(self.status)
    }
}
fn observe(result: bool, m: &Model, s: &State, h: u64) -> String {
    format!(
        "S {} {} {} {} {} {} {} {} {} {} {h:016x}",
        result as u8,
        m.flag as u8,
        m.phase(),
        s.children,
        s.polls,
        s.notices,
        s.starts,
        s.callbacks,
        s.releases,
        m.complete(s.children == 0) as u8
    )
}
#[test]
#[ignore = "original ab700 complete101-row snapshots,alias-free copies and SaveGame phase/predicate/final callback with prepared native child/future boundaries"]
fn higurashi_save_task_full_settings_snapshots_and_lifecycle_match_native() {
    let golden: serde_json::Value = serde_json::from_slice(
        &std::fs::read("reverse-engineering/higurashi/native-save-codec-golden.json").unwrap(),
    )
    .unwrap();
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut owners = 0;
    for p in golden["profiles"].as_array().unwrap() {
        let n = p["profile"].as_u64().unwrap() as u32;
        let raw = p["raw"].as_str().unwrap();
        let data = SaveData::decode_raw(
            (0..raw.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&raw[i..i + 2], 16).unwrap())
                .collect(),
        )
        .unwrap();
        let (base, slots) = profile(data, n);
        for flag in [0, 1, 2, 255, u32::MAX] {
            let mut m = Model::new(flag);
            let mut s = State::default();
            let mut pred = Predicate::default();
            let cloned = (base.clone(), slots.clone());
            let h = hash(&cloned.0, &cloned.1);
            input += &format!("N {n} {flag}\n");
            expected.push(observe(false, &m, &s, h));
            input += "M\n";
            let mut modified = (base.clone(), slots.clone());
            modified.0.play_seconds = modified.0.play_seconds.wrapping_add(1);
            modified.1.auto.data.progress.extra_word = 0;
            assert_ne!(hash(&modified.0, &modified.1), h);
            assert_eq!(hash(&cloned.0, &cloned.1), h);
            expected.push(observe(false, &m, &s, h));
            for (permission, delta) in [(0, 0.0), (1, 3.0)] {
                input += &format!("U {permission} {delta}\n");
                m.poll(&mut s).unwrap();
                expected.push(observe(false, &m, &s, h));
            }
            for status in [2, 0, 1, 3, u32::MAX, 2, 2] {
                input += &format!("P {status}\n");
                s.status = status;
                let result = pred.poll(&mut s).unwrap();
                expected.push(observe(result, &m, &s, h));
            }
            input += "C 0\n";
            m.callback(0);
            expected.push(observe(false, &m, &s, h));
            input += "U 1 0.5\n";
            m.poll(&mut s).unwrap();
            expected.push(observe(false, &m, &s, h));
            input += "C 1\n";
            s.releases += 1;
            m.callback(1);
            expected.push(observe(false, &m, &s, h));
            input += "U 0 3\n";
            m.poll(&mut s).unwrap();
            expected.push(observe(false, &m, &s, h));
            input += "E 0\n";
            s.children = 0;
            expected.push(observe(false, &m, &s, h));
            input += "U 1 0\n";
            m.poll(&mut s).unwrap();
            expected.push(observe(false, &m, &s, h));
            input += "D\n";
            expected.push("D".into());
            owners += 1;
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_TASK").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_FONT").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let copy = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(copy.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    assert_eq!(actual.lines().count(), expected.len());
    for (i, (a, e)) in actual.lines().zip(&expected).enumerate() {
        assert_eq!(a, e, "native SaveGame observation{i}");
    }
    let path = "/tmp/higurashi-save-task-capture";
    std::fs::create_dir_all(path).unwrap();
    for (name, data) in [
        ("input.txt", input.as_bytes()),
        ("output.txt", actual.as_bytes()),
        ("stderr.txt", &output.stderr),
    ] {
        std::fs::write(format!("{path}/{name}"), data).unwrap();
    }
    println!(
        "SaveGame:{owners} original owners/{} exact observations;complete101-slot/dormant/full-word snapshots,no alias,flag masking,first predicate false,all status gates,stage0 ignored,stage1 future release,final callback before child pruning and balanced disposal pass",
        expected.len() - owners
    );
}
