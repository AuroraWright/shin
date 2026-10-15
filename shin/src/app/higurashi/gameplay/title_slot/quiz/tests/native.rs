use crate::{
    adv::{higurashi::host::frame_delta, play_clock::PlayClock},
    app::higurashi::{
        gameplay::{
            bindings,
            quiz::SceneRequest,
            title_slot::{Pending, Return},
        },
        quiz_scene::opening::Opening,
        runtime_game_data::RuntimeGameData,
    },
};
use shin_core::{
    format::{save::higurashi::SaveDate, scenario::higurashi::HigurashiScenario},
    time::Ticks,
};
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

fn source(active: usize, salt: u8, length: usize) -> RuntimeGameData {
    let mut data = crate::app::higurashi::tips_menu::tests::source(100, active);
    data.date = SaveDate {
        year: 2026,
        month: 10,
        day: 13,
        hour: 12,
        minute: 34,
        second: salt,
    };
    for (i, entry) in data.progress.entries.iter_mut().enumerate() {
        entry.scenario_id = 100 + i as i32;
        entry.random_seed = 17 + i as u32;
        entry.save_position = 23 + i as u32;
        entry.replay_target = 31 + i as u32;
        entry.response_data = (0..length + i)
            .map(|j| (j * 7 + usize::from(salt) + i) as u8)
            .collect();
    }
    data.byte_variables = vec![0, 85, 170, 255];
    data
}
fn protocol(data: &RuntimeGameData) -> String {
    let mut text = format!(
        "{:x} {} {:x}",
        bindings::packed_date(data.date),
        data.progress.active_index(),
        data.progress.extra_word
    );
    for entry in &data.progress.entries {
        write!(
            text,
            " {:x} {:x} {:x} {:x} {}",
            entry.scenario_id as u32,
            entry.random_seed,
            entry.save_position,
            entry.replay_target,
            entry.response_data.len()
        )
        .unwrap();
        for byte in &entry.response_data {
            write!(text, " {byte}").unwrap();
        }
    }
    write!(text, " {}", data.trailing_values.len()).unwrap();
    for value in &data.trailing_values {
        write!(text, " {value}").unwrap();
    }
    write!(text, " {}", data.byte_variables.len()).unwrap();
    for byte in &data.byte_variables {
        write!(text, " {byte}").unwrap();
    }
    text
}
fn data_words(data: &RuntimeGameData) -> Vec<u64> {
    let mut words = vec![
        bindings::packed_date(data.date),
        data.progress.active_index() as u64,
        data.progress.extra_word as u64,
    ];
    for entry in &data.progress.entries {
        words.extend([
            entry.scenario_id as u32 as u64,
            entry.random_seed as u64,
            entry.save_position as u64,
            entry.replay_target as u64,
            entry.response_data.len() as u64,
        ]);
        words.extend(entry.response_data.iter().map(|&v| u64::from(v)));
    }
    words.push(data.trailing_values.len() as u64);
    words.extend(data.trailing_values.iter().map(|&v| u64::from(v)));
    words.push(data.byte_variables.len() as u64);
    words.extend(data.byte_variables.iter().map(|&v| u64::from(v)));
    words
}
fn snapshot(
    op: char,
    result: u32,
    pending: Option<[u32; 4]>,
    active: [u32; 4],
    delta: f32,
    clock: &PlayClock,
    seen: f64,
    seeds: u32,
    capture: bool,
    retired: u32,
    events: &[u32],
    data: &RuntimeGameData,
) -> String {
    let mut words = vec![u64::from(result)];
    words.extend(pending.unwrap_or([0; 4]).map(u64::from));
    words.extend(active.map(u64::from));
    words.extend([
        u64::from(delta.to_bits()),
        clock.ticks().to_bits(),
        seen.to_bits(),
        u64::from(clock.saved_seconds()),
        u64::from(seeds),
        u64::from(capture),
        u64::from(retired),
        events.len() as u64,
    ]);
    words.extend(events.iter().map(|&v| u64::from(v)));
    words.extend(data_words(data));
    let mut text = format!("T {op}");
    for word in words {
        write!(text, " {word}").unwrap();
    }
    text
}

#[test]
#[ignore = "actual9fae0/275b0/2714c/9bb48/First/a07a0/17490/11cd8 ADV28 prefix and production pure quiz factory/clock/typed data owners;no GPU/audio/window/saves"]
fn higurashi_quiz_scene_factories_match_native_preflight_selection_full_data_and_score_transfer() {
    let scenario = HigurashiScenario::new(
        std::fs::read(std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap())
            .unwrap()
            .into(),
    )
    .unwrap();
    let mut input = String::from("X 3f000000\nX 3f800000\n");
    let mut expected = vec!["X 1056964608".to_owned(), "X 1065353216".to_owned()];
    let mut owners = 0;
    for active in 0..2 {
        for captured in [false, true] {
            for pool in 0..8 {
                for context in 0..9 {
                    let index = owners;
                    let length = [0, 1, 7, 17, 31, 1024, 1144, 1520][index % 8];
                    let mut clock = PlayClock::from_saved_seconds(1000 + index as u32);
                    clock.advance(0.5);
                    let initial = source(active, 1, length);
                    writeln!(
                        input,
                        "N {:x} {} {}",
                        clock.ticks().to_bits(),
                        u8::from(captured),
                        protocol(&initial)
                    )
                    .unwrap();
                    expected.push(snapshot(
                        'N',
                        0,
                        None,
                        [2, 0, 0, 0],
                        0.0,
                        &clock,
                        clock.ticks(),
                        0,
                        false,
                        0,
                        &[],
                        &initial,
                    ));
                    let first = SceneRequest {
                        args: [(pool + 1) % 8, (context + 1) % 9, 9],
                        data: source(1 - active, 2, length),
                    };
                    writeln!(
                        input,
                        "Q {} {} {} 1 {}",
                        first.args[0],
                        first.args[1],
                        first.args[2],
                        protocol(&first.data)
                    )
                    .unwrap();
                    expected.push(snapshot(
                        'Q',
                        0,
                        Some([1, first.args[0] as u32, first.args[1] as u32, 9]),
                        [2, 0, 0, 0],
                        0.0,
                        &clock,
                        clock.ticks(),
                        0,
                        false,
                        0,
                        &[],
                        &first.data,
                    ));
                    let mut pending = Some(Pending::Quiz(first));
                    let args = [pool, context, [-1, 0, 5, 9, 10, i32::MAX][index % 6]];
                    let seed = [0, 17, u32::MAX][index % 3];
                    let packet = source(active, 3, length);
                    writeln!(
                        input,
                        "Q {} {} {} {seed} {}",
                        args[0],
                        args[1],
                        args[2] as u32,
                        protocol(&packet)
                    )
                    .unwrap();
                    drop(pending.replace(Pending::Quiz(SceneRequest {
                        args,
                        data: packet.clone(),
                    })));
                    let Some(Pending::Quiz(request)) = &pending else {
                        unreachable!()
                    };
                    expected.push(snapshot(
                        'Q',
                        0,
                        Some([1, args[0] as u32, args[1] as u32, args[2] as u32]),
                        [2, 0, 0, 0],
                        0.0,
                        &clock,
                        clock.ticks(),
                        0,
                        false,
                        0,
                        &[],
                        &request.data,
                    ));
                    let extent = if captured { (1920, 1080) } else { (1280, 720) };
                    let prepared = Opening::prepare_scene(
                        request.data.clone(),
                        request.args,
                        &scenario,
                        extent,
                    )
                    .unwrap();
                    let mut seed_calls = 0;
                    // This is the production moved Prepared -> Model/Plan path.
                    // Actual Slot source retirement/capture/thread owners are
                    // covered by the separate hardware roundtrip, not this CPU test.
                    let (mut model, plan, actual_extent) = prepared.into_model(|| {
                        seed_calls += 1;
                        seed
                    });
                    drop(pending.take());
                    assert_eq!(seed_calls, 1);
                    assert_eq!(actual_extent, extent);
                    assert_eq!(model.data, packet);
                    assert!(
                        plan.bustups
                            .iter()
                            .all(|&id| (id as usize) < scenario.bustup_info.len())
                    );
                    for (question, path) in model.questions.iter().zip(&plan.questions) {
                        assert_eq!(
                            *path,
                            format!("/quiz/{}.txa", question.descriptor().texture)
                        );
                    }
                    let a = [
                        1,
                        model.args[0] as u32,
                        model.args[1] as u32,
                        model.clamped_argument as u32,
                    ];
                    let elapsed = [0.0, 0.5, 1.0, 3.0, 8.0, 100.0][index % 6];
                    let delta = frame_delta(Ticks::from_f32(elapsed)).unwrap().as_f32();
                    let seen = clock.ticks();
                    clock.advance(elapsed);
                    writeln!(input, "U {elapsed}").unwrap();
                    expected.push(snapshot(
                        'U',
                        0,
                        None,
                        a,
                        delta,
                        &clock,
                        seen,
                        seed_calls,
                        captured,
                        0,
                        &[1, 2, 4, 5, 6, 3, 7, 8],
                        &model.data,
                    ));
                    input.push_str("S\n");
                    let mut selection = format!("S {seed_calls}");
                    for question in &model.questions {
                        write!(selection, " {}", question.index()).unwrap();
                    }
                    expected.push(selection);
                    let score = [0, 1, 5, 10, u32::MAX, 0x8000_0000][index % 6];
                    model.score = score as i32;
                    model.clocks[0].fast_forward_to(0.0);
                    model.clocks[0]
                        .control(1.0, Ticks::from_f32(3.0), 0)
                        .unwrap();
                    writeln!(input, "R {score} 1").unwrap();
                    expected.push(snapshot(
                        'R',
                        u32::from(!model.clocks[0].is_idle()),
                        None,
                        a,
                        delta,
                        &clock,
                        seen,
                        seed_calls,
                        captured,
                        0,
                        &[],
                        &model.data,
                    ));
                    model.clocks[0].fast_forward_to(0.0);
                    let mut returned = Return {
                        data: model.data.clone(),
                        flags: model.score as u32,
                        globals: None,
                        bindings: None,
                        snapshot: None,
                        seeds: None,
                    };
                    writeln!(input, "R {score} 0").unwrap();
                    expected.push(snapshot(
                        'R',
                        u32::from(!model.clocks[0].is_idle()),
                        Some([2, returned.flags, 0, 0]),
                        a,
                        delta,
                        &clock,
                        seen,
                        seed_calls,
                        captured,
                        0,
                        &[],
                        &returned.data,
                    ));
                    // Both factory banks remain complete after the old source
                    // retires. Independently changing the return must not alias it.
                    returned.data.progress.entries[0].response_data.push(99);
                    assert_eq!(model.data, packet);
                    returned.data = model.data.clone();
                    let elapsed = [0.0, 0.5, 8.0][index % 3];
                    let delta = frame_delta(Ticks::from_f32(elapsed)).unwrap().as_f32();
                    let seen = clock.ticks();
                    clock.advance(elapsed);
                    let events: &[u32] = if captured {
                        &[1, 10, 11, 2, 3, 7, 8]
                    } else {
                        &[1, 10, 2, 3, 7, 8]
                    };
                    writeln!(input, "U {elapsed}\nD").unwrap();
                    expected.push(snapshot(
                        'U',
                        0,
                        None,
                        [2, returned.flags, 0, 0],
                        delta,
                        &clock,
                        seen,
                        seed_calls,
                        false,
                        1,
                        events,
                        &returned.data,
                    ));
                    expected.push("D".into());
                    owners += 1;
                }
            }
        }
    }
    assert_eq!(owners, 288);
    assert_eq!(expected.len(), 2594);
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_QUIZ_SCENE_TRANSFER").unwrap())
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
    let write_result = writer.join().unwrap();
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_QUIZ_SCENE_TRANSFER_CAPTURE") {
        let path = std::path::Path::new(&path).join("native");
        std::fs::create_dir_all(&path).unwrap();
        for (name, bytes) in [
            ("input.txt", input.as_bytes()),
            ("output.txt", output.stdout.as_slice()),
            ("stderr.txt", output.stderr.as_slice()),
        ] {
            std::fs::write(path.join(name), bytes).unwrap();
        }
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    write_result.unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), expected.len());
    for (index, (actual, expected)) in text.lines().zip(expected).enumerate() {
        assert_eq!(actual, expected, "native quiz transfer row{index}");
    }
    eprintln!(
        "Quiz native transfer:288 actual factory/dispatch owners/2594 exact rows;production unseeded preflight/moved Model-Plan consumes one constructor seed and matches selected question IDs;all8 pools/9 contexts,both complete active/dormant records/captures,queuecoalescing,clock clamp/full elapsed,busy fade and0/10/signedextreme score factories/actual ADV28 prefix and independent full data pass;native empty ADV focus targets retained;CPU source-ADV/remaining-ADV/worker/capture/audio boundaries,no GPU/audio/window/saves;live slot/runner checked separately"
    );
}
