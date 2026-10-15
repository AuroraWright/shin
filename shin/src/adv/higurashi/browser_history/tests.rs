use super::super::autosave::ProgressEntry;
use super::*;
use binrw::BinWrite;
use shin_core::format::{
    save::{PersistData, index_set::IndexSet},
    scenario::instruction_elements::{NumberSpec, Register},
};
use std::{
    fmt::Write as _,
    io::{Cursor, Write as _},
    process::{Command, Stdio},
};

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
fn hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        "-".into()
    } else {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
fn fixture(code: &[u8]) -> HigurashiScenario {
    let mut bytes = vec![0; 0x50];
    bytes[..4].copy_from_slice(b"SNR ");
    bytes[12..16].copy_from_slice(&65u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&133u32.to_le_bytes());
    for i in 0..11 {
        let at = bytes.len() as u32;
        bytes[36 + i * 4..40 + i * 4].copy_from_slice(&at.to_le_bytes());
        if !matches!(i, 7 | 8) {
            bytes.extend(4u32.to_le_bytes());
        }
        bytes.extend(0u32.to_le_bytes());
    }
    let at = bytes.len() as u32;
    bytes[32..36].copy_from_slice(&at.to_le_bytes());
    bytes.extend(code);
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    HigurashiScenario::new(bytes.into()).unwrap()
}
fn source(active: usize, target: u32, route: Vec<u8>, id: i32, seed: u32) -> ProgressMemory {
    let mut entries = std::array::from_fn(|i| ProgressEntry {
        scenario_id: 100 + i as i32,
        random_seed: 99,
        save_position: 17,
        replay_target: 19,
        response_data: vec![37; 7],
    });
    entries[active] = ProgressEntry {
        scenario_id: id,
        random_seed: seed,
        save_position: target,
        replay_target: target,
        response_data: route,
    };
    ProgressMemory::new(
        PersistData((0..64).map(|i| (i * 7919) as i16).collect()),
        SaveProgress::new(active, 0, entries).unwrap(),
        vec![0, 1, 127, 255],
    )
}
fn num(code: &mut Vec<u8>, value: i32) {
    let mut cursor = Cursor::new(Vec::new());
    NumberSpec::constant(value).write_le(&mut cursor).unwrap();
    code.extend(cursor.into_inner());
}
fn string(code: &mut Vec<u8>, long: bool, text: &[u8]) {
    let size = text.len() + 1;
    if long {
        code.extend((size as u16).to_le_bytes());
    } else {
        code.push(size as u8);
    }
    code.extend(text);
    code.push(0);
}
fn message(code: &mut Vec<u8>, id: u32, quick: u8, text: &[u8]) {
    code.push(0x86);
    code.extend(&id.to_le_bytes()[..3]);
    code.push(quick);
    num(code, -1);
    string(code, true, text);
}
fn select(code: &mut Vec<u8>, index: u16, visibility: i32) {
    code.push(0x8d);
    code.extend(1u16.to_le_bytes());
    code.extend(index.to_le_bytes());
    code.extend(31u16.to_le_bytes());
    num(code, visibility);
    string(code, false, b"Title\xb6");
    let mut strings = Vec::new();
    for i in 0..12 {
        strings.extend(format!("*{i}.Row{i}").bytes());
        strings.push(0);
    }
    strings.push(0);
    code.push(strings.len() as u8);
    code.extend(strings);
}
fn text(out: &mut String, text: &str) {
    write!(out, " {}", text.len()).unwrap();
    for b in text.bytes() {
        write!(out, " {b}").unwrap();
    }
}
fn snapshot(
    builder: &LogBuilder,
    vm: &HigurashiScripter,
    offset: u32,
    available: bool,
) -> Vec<String> {
    let mut out = format!(
        "B {} {} {} {} {} {} {} {} {} {}",
        u32::from(available),
        u32::from(builder.done()),
        builder.boundary,
        builder.row_count,
        builder.current_row,
        builder.style,
        builder.event,
        u32::from(builder.layer_back),
        u32::from(builder.message_present),
        builder.route().1
    );
    let persist: Vec<_> = builder
        .private_persistent()
        .0
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    for (bytes, stride) in [
        (builder.route().0, 1),
        (persist.as_slice(), 2),
        (builder.private_bytes(), 1),
    ] {
        write!(out, " {} {:016x}", bytes.len() / stride, hash(bytes)).unwrap();
    }
    write!(
        out,
        " {} {} {}",
        vm.position().0 - offset,
        vm.next_position().0 - offset,
        vm.random_state()
    )
    .unwrap();
    for i in 0..64 {
        write!(
            out,
            " {}",
            vm.read_register(Register::from_regular_register(i))
        )
        .unwrap();
    }
    let mut rows = vec![out];
    for row in &builder.rows {
        let mut out = format!(
            "R {} {} {}",
            match row.item {
                Item::Message { .. } => 0,
                Item::Choice { .. } => 1,
                Item::Checkpoint { .. } => 2,
                Item::CharacterChoice { .. } => 3,
            },
            row.boundary,
            row.index
        );
        match &row.item {
            Item::Message {
                text: t,
                style,
                argument,
            } => {
                write!(
                    out,
                    " {style} {} {}",
                    u32::from(argument.is_some()),
                    argument.unwrap_or(0)
                )
                .unwrap();
                text(&mut out, t);
            }
            Item::Choice { title, text: t } | Item::Checkpoint { title, text: t } => {
                text(&mut out, title);
                text(&mut out, t);
            }
            Item::CharacterChoice { response } => write!(out, " {response}").unwrap(),
        }
        rows.push(out);
        let mut out = String::from("T");
        text(&mut out, &row.item.display_text().unwrap());
        rows.push(out);
    }
    let mut out = format!("K {}", builder.markers.len());
    for marker in &builder.markers {
        write!(out, " {marker}").unwrap();
    }
    rows.push(out);
    rows
}

#[test]
#[ignore = "executes supplied original LogBuilder callbacks and real authored history; no GPU/audio/save effects"]
fn higurashi_browser_history_matches_native_replay_future_routes_and_source_text() {
    let helper = std::env::var_os("SHIN_HIGURASHI_NATIVE_BROWSER_HISTORY").unwrap();
    let exe = std::env::var_os("SHIN_HIGURASHI_EXE").unwrap();
    let font = std::env::var_os("SHIN_HIGURASHI_FONT").unwrap();
    let authored = std::env::var_os("SHIN_HIGURASHI_SCENARIO").unwrap();
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut count = 0;
    let mut authored_message_rows = 0;
    let mut compare = |scenario: &HigurashiScenario,
                       code: Option<&[u8]>,
                       source: ProgressMemory,
                       read_only: bool,
                       last: Vec<u8>,
                       bits: IndexSet| {
        let source = source.with_chart_unlocks(super::super::chart_progress::ChartUnlocks::from_saved_words(
            vec![0x1234, 0, 0x80],
        ));
        let mut history = MessageReadHistory::new();
        for id in bits.iter() {
            history.record_completion(true, id as u32);
        }
        let mut words = vec![0u32; bits.size_in_words()];
        for id in bits.iter() {
            words[id / 32] |= 1 << (id % 32);
        }
        let choices = ChoiceHistory::new(IndexSet::new(), last.clone());
        let original_progress = source.progress().clone();
        let original_persistent = source.persistent().0.clone();
        let original_bytes = source.bytes().to_vec();
        let available =
            BrowserHistory::available(scenario, &source, &history, &choices, read_only, 100000)
                .unwrap();
        let active = source.progress().active_entry();
        let mut vm =
            HigurashiScripter::new(scenario, active.scenario_id, active.random_seed).unwrap();
        let mut builder = LogBuilder::new(&source).unwrap();
        let mut response = CommandResult::None;
        let mut commands = 0;
        while !builder.done() {
            commands += 1;
            assert!(commands < 100000);
            let mut command = vm.run(response, 100000).unwrap();
            browser_text(scenario, vm.position(), &mut command).unwrap();
            response = builder
                .command(&command, &history, &choices, read_only)
                .unwrap();
        }
        expected.extend(snapshot(
            &builder,
            &vm,
            if code.is_some() {
                scenario.header.code_offset
            } else {
                0
            },
            available,
        ));
        let rows = builder.rows.clone();
        if code.is_none() {
            authored_message_rows += rows
                .iter()
                .filter(|r| matches!(r.item, Item::Message { .. }))
                .count();
        }
        let markers = builder.markers.clone();
        let route = builder.route().0.to_vec();
        let bytes = builder.private_bytes().to_vec();
        let result = builder.finish(&source);
        assert_eq!(result.rows, rows);
        assert_eq!(result.markers, markers);
        assert_eq!(result.jump_source.responses().0, route);
        assert_eq!(result.jump_source.bytes(), bytes);
        assert_eq!(result.jump_source.persistent().0, original_persistent);
        assert_eq!(source.progress(), &original_progress);
        assert_eq!(source.persistent().0, original_persistent);
        assert_eq!(source.bytes(), original_bytes);
        let collected =
            BrowserHistory::collect(scenario, &source, &history, &choices, read_only, 100000)
                .unwrap();
        assert_eq!(collected.rows, result.rows);
        assert_eq!(collected.markers, result.markers);
        assert_eq!(collected.current_row, result.current_row);
        assert_eq!(collected.jump_source.chart_unlocks.saved_words(), source.chart_unlocks.saved_words());
        result.jump_source.chart_unlocks.construct(1, &[95], &mut Vec::new());
        assert_eq!(source.chart_unlocks.saved_words(), vec![0x1234, 0, 0x80000080]);
        assert_eq!(collected.jump_source.chart_unlocks.saved_words(), source.chart_unlocks.saved_words());
        write!(
            input,
            "D {} {} {} {} {} {} {} {} {} {} {}",
            source.progress().active_index(),
            active.save_position,
            active.scenario_id,
            active.random_seed,
            u32::from(read_only),
            source.persistent().0.len(),
            source.bytes().len(),
            last.len(),
            words.len(),
            hex(&active.response_data),
            code.map_or("@".into(), hex)
        )
        .unwrap();
        for v in &source.persistent().0 {
            write!(input, " {v}").unwrap();
        }
        for v in source.bytes() {
            write!(input, " {v}").unwrap();
        }
        for v in last {
            write!(input, " {v}").unwrap();
        }
        for v in &words {
            write!(input, " {v:x}").unwrap();
        }
        input.push('\n');
        count += 1;
    };
    let mut basic = vec![0x85, 2, 0, 0];
    message(&mut basic, 1, 0, b"@rKana\xb6\xdc\xb2\xb2");
    basic.extend([0xa2, 3]);
    message(&mut basic, 2, 1, b"Event");
    basic.push(0xa3);
    message(&mut basic, 3, 0, b"Future");
    basic.extend([0x8a, 0]);
    basic.push(0x8c);
    string(&mut basic, true, b"Log\xb6");
    select(&mut basic, 1, 3);
    basic.extend([0xa0, 0]);
    string(&mut basic, false, b"Chapter\xb6");
    basic.extend([0xa0, 1]);
    string(&mut basic, false, b"Name");
    basic.push(0xa1);
    basic.extend([0, 0, 0]);
    let mut variables = vec![0x82, 1];
    num(&mut variables, 70000);
    variables.extend([0x81, 31, 0, 1, 0xdb, 2]);
    num(&mut variables, 513);
    variables.extend([0xda, 32, 0, 2, 0xa1, 0x82, 1]);
    num(&mut variables, -32769);
    variables.extend([0x81, 33, 0, 1, 0xdb, 6, 0, 0xda, 34, 0, 6, 0, 0, 0]);
    let mut choice = vec![0x8a, 0];
    select(&mut choice, 1, 3);
    select(&mut choice, 0, 3);
    choice.extend([0, 0, 0]);
    let mut extra = vec![
        0xc5, 0, 0x8e, 0, 0, 0, 0, 0xd0, 0, 0xd1, 1, 0, 31, 0, 3, 0xd4, 0, 0xd5, 0xd7, 32, 0, 0, 0,
        0, 0, 0, 0,
    ];
    message(&mut extra, 0, 0, b"Negative");
    extra.extend([0, 0, 0]);
    for code in [&basic, &variables, &choice, &extra] {
        let scenario = fixture(code);
        for target in [0, 1, 2, 3, 4, 5, 8, 20, u32::MAX] {
            for active in 0..2 {
                for read_only in [false, true] {
                    for last in [0, 1, 2, 3] {
                        let mut bits = IndexSet::new();
                        if last & 1 != 0 {
                            for i in 0..4 {
                                bits.add(i);
                            }
                        }
                        compare(
                            &scenario,
                            Some(code),
                            source(active, target, vec![0; 8], 42, 0x12345678),
                            read_only,
                            vec![last; 32],
                            bits,
                        );
                    }
                }
            }
        }
    }
    let mut recorded = vec![0x81, 30, 0, 1];
    select(&mut recorded, 1, 15);
    recorded.extend([0xda, 32, 0, 2, 0xd7, 33, 0, 0, 0, 0]);
    message(&mut recorded, 0, 0, b"Recorded");
    recorded.extend([0, 0, 0]);
    let scenario = fixture(&recorded);
    for value in [0, 1, 32767, -32768, -123] {
        for choice in 0..4 {
            for byte in [0, 255] {
                for active in 0..2 {
                    let mut route = HigurashiVariables::new(Default::default(), Vec::new());
                    route.response(false, false, value).unwrap();
                    route.response_choice(false, choice).unwrap();
                    route.response(true, false, byte).unwrap();
                    route.response_choice(false, 3).unwrap();
                    compare(
                        &scenario,
                        Some(&recorded),
                        source(active, 20, route.route().0.to_vec(), 42, 42),
                        true,
                        vec![0; 32],
                        IndexSet::new(),
                    );
                }
            }
        }
    }
    // Real prologue includes original Shift-JIS labels, arithmetic branches,
    // persistent lookups, authored message text and the first unread boundary.
    let scenario = HigurashiScenario::new(std::fs::read(&authored).unwrap().into()).unwrap();
    for target in [1, 2, 3, 5, 10, 20, 50, 100] {
        compare(
            &scenario,
            None,
            source(0, target, vec![0; 1024], 0, 42),
            true,
            vec![0; 32],
            IndexSet::new(),
        );
    }
    assert!(
        authored_message_rows > 0,
        "Real authored history must reach messages"
    );
    eprintln!("Authored MessageBrowser message rows:{authored_message_rows}");
    let mut child = Command::new(helper)
        .args([exe, font, authored])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = actual.lines().collect();
    assert_eq!(
        lines.len(),
        expected.len(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (i, (a, b)) in lines.iter().zip(&expected).enumerate() {
        assert_eq!(a, &b, "native browser line{i}");
    }
    eprintln!(
        "Higurashi browser histories:{count} original constructor/preflight/VM/listener cases;{} exact rows/state observations",
        expected.len()
    );
}
