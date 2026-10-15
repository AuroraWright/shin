use super::*;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

pub(super) fn append_data(out: &mut String, data: &RuntimeGameData) {
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
pub(super) fn append_string(row: &mut String, value: &str) {
    write!(row, " {}", value.len()).unwrap();
    for byte in value.bytes() {
        write!(row, " {byte}").unwrap();
    }
}
fn constructor_row(model: &Model) -> String {
    let mut row = format!(
        "N {} {} {} {} {} 0",
        model.args[0] as u32,
        model.args[1] as u32,
        model.clamped_argument,
        model.rng.state(),
        model.score
    );
    for q in &model.questions {
        write!(row, " {}", q.index()).unwrap();
    }
    row.push_str(" 1 1 4 1");
    for position in ANSWER_POSITIONS {
        write!(row, " {} {}", position[0].to_bits(), position[1].to_bits()).unwrap();
    }
    append_data(&mut row, &model.data);
    row
}
fn run(input: String, expected: Vec<String>, tag: &str) {
    run_native(
        input,
        expected,
        tag,
        "SHIN_HIGURASHI_NATIVE_QUIZ_SCENE_MODEL",
    );
}
pub(super) fn run_native(input: String, expected: Vec<String>, tag: &str, oracle: &str) {
    let mut child = Command::new(std::env::var(oracle).unwrap())
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
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_QUIZ_SCENE_CAPTURE") {
        let dir = std::path::Path::new(&dir).join(tag);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in [
            ("input.txt", input.as_bytes()),
            ("output.txt", output.stdout.as_slice()),
            ("stderr.txt", output.stderr.as_slice()),
        ] {
            std::fs::write(dir.join(name), bytes).unwrap();
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
        assert_eq!(*actual, expected, "quiz {tag} native row{i}");
    }
}
const SEEDS: [u32; 14] = [
    0,
    1,
    2,
    3,
    7,
    17,
    0x76543219,
    0x7ffffffd,
    0x7ffffffe,
    0x7fffffff,
    0x80000000,
    0x80000001,
    0xfffffffe,
    u32::MAX,
];

#[test]
#[ignore = "actual updated490 question descriptors via relocated native tables,including UTF8 answer ordering/voice/texture strings;no GPU/audio/window/saves"]
fn higurashi_quiz_all_native_question_descriptors_match_catalog() {
    let mut expected = Vec::new();
    for i in 0..490 {
        let question = QuestionId(i).descriptor();
        let mut row = format!("T {i}");
        for value in [
            question.voice,
            question.texture,
            question.answers[0],
            question.answers[1],
            question.answers[2],
            question.answers[3],
        ] {
            write!(row, " {}", value.len()).unwrap();
            for b in value.bytes() {
                write!(row, " {b}").unwrap();
            }
        }
        expected.push(row);
    }
    run("T\n".into(), expected, "catalog");
    eprintln!(
        "Quiz catalog:490 native descriptors/2940 exact voice-texture-answer strings;8x60 normal questions and10 position-dependent specials pass"
    );
}

#[test]
#[ignore = "actual minstd_rand/libc++ signed32 and unsigned64 uniform helpers over every quiz range,raw seed normalization/rejection/state continuation;no GPU/audio/window/saves"]
fn higurashi_quiz_random_helpers_match_native_without_modulo_sampling() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut pairs = 0;
    for seed in SEEDS {
        for width in [32, 64] {
            for limit in [1, 2, 3, 4, 5, 10, 16, 32, 59, 60] {
                let mut rng = random::Random::new(seed);
                writeln!(input, "R {} {width} {limit} 256", rng.state()).unwrap();
                let mut row = "R".to_owned();
                for _ in 0..256 {
                    let value = rng.uniform(limit);
                    write!(row, " {value} {}", rng.state()).unwrap();
                    pairs += 1;
                }
                expected.push(row);
            }
        }
    }
    run(input, expected, "random");
    eprintln!(
        "Quiz random:280 native profiles/{pairs} exact value-state pairs;minstd_rand seed normalization and signed32/unsigned64 libc++ engine-range/request-range rejection over1..60 pass"
    );
}

#[test]
#[ignore = "actual9bb48 scene constructors/shuffle/special choice/clamp/text positions,full independent GameData vectors/date/destruction;First/input/text services checked,no GPU/audio/window/saves"]
fn higurashi_quiz_scene_constructors_match_native_question_selection_and_full_data() {
    let mut profiles = Vec::new();
    for group in 0..8 {
        for seed in SEEDS {
            for active in [0, 1] {
                for argument in [-1, 0, 1, 9, 10, i32::MIN, i32::MAX] {
                    let context = [i32::MIN, -1, 0, 1, 8, 9, i32::MAX][profiles.len() % 7];
                    profiles.push(([group, context, argument], seed, active));
                }
            }
        }
    }
    for seed in 1..=256 {
        profiles.push((
            [(seed % 8) as i32, (seed % 9) as i32, (seed % 12) as i32 - 2],
            seed,
            (seed % 2) as usize,
        ));
    }
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut specials = 0;
    for &(args, seed, active) in &profiles {
        let data = super::super::tips_menu::tests::source(100, active);
        let before = data.clone();
        let model = Model::new(data, args, seed).unwrap();
        assert_eq!(model.data, before);
        assert_eq!(model.marks, [0; 11]);
        let mut ids = std::collections::BTreeSet::new();
        for (position, q) in model.questions.iter().enumerate() {
            assert!(ids.insert(q.index()));
            if q.index() >= 480 {
                assert!((1..=8).contains(&position));
                assert_eq!(q.index(), 480 + position as u16);
                specials += 1;
            }
        }
        writeln!(
            input,
            "N {} {} {} {active} {seed}",
            args[0] as u32, args[1] as u32, args[2] as u32
        )
        .unwrap();
        expected.push(constructor_row(&model));
        input.push_str("D\n");
        expected.push("D".into());
    }
    run(input, expected, "constructors");
    eprintln!(
        "Quiz scene:1824 native constructors/3648 exact observations;all8 catalogs/seed extremes/both full data records/clamped third argument/raw context/10 unique questions/{specials} position-dependent specials/native text positions and balanced disposal pass"
    );
}

#[derive(Default)]
struct Interaction {
    input: String,
    expected: Vec<String>,
    model: Option<Model>,
    owners: usize,
    reactions: std::collections::BTreeSet<(i32, u32)>,
    permutations: std::collections::BTreeSet<[usize; 4]>,
}
impl Interaction {
    fn begin(&mut self, args: [i32; 3], seed: u32) {
        let active = self.owners % 2;
        writeln!(
            self.input,
            "N {} {} {} {active} {seed}",
            args[0] as u32, args[1] as u32, args[2] as u32
        )
        .unwrap();
        let model = Model::new(
            super::super::tips_menu::tests::source(100, active),
            args,
            seed,
        )
        .unwrap();
        self.expected.push(constructor_row(&model));
        self.model = Some(model);
        self.owners += 1;
    }
    fn snapshot(&mut self, op: char, result: bool) {
        let m = self.model.as_mut().unwrap();
        let mut row = format!(
            "H {op} {} {} {} {} {} {} {} {}",
            u8::from(result),
            m.rng.state(),
            m.current_question,
            m.correct_answer,
            m.chosen_answer,
            m.clamped_argument,
            m.score as u32,
            m.reaction_variant
        );
        for value in m.contact {
            write!(row, " {value}").unwrap();
        }
        for clock in &m.clocks {
            write!(
                row,
                " {} {}",
                clock.value().to_bits(),
                u8::from(clock.is_waiting())
            )
            .unwrap();
        }
        for value in m.marks {
            write!(row, " {}", value as u32).unwrap();
        }
        for text in m.answers().unwrap() {
            append_string(&mut row, text);
        }
        let actions = m.take_actions();
        write!(row, " E {}", actions.len()).unwrap();
        for action in actions {
            match action {
                Action::Cue(name) => {
                    row.push_str(" 0 0");
                    append_string(&mut row, name);
                }
                Action::Voice { name, reaction } => {
                    write!(row, " 1 {}", u8::from(reaction)).unwrap();
                    append_string(&mut row, name);
                    if reaction {
                        self.reactions.insert((m.args[1], m.reaction_variant));
                    }
                }
                Action::DisableInput => row.push_str(" 2 0"),
            }
        }
        self.expected.push(row);
    }
    fn prepare(&mut self, index: usize) {
        writeln!(self.input, "Q {index}").unwrap();
        let m = self.model.as_mut().unwrap();
        m.prepare_question(index).unwrap();
        self.permutations.insert(m.answer_order);
        self.snapshot('Q', false);
    }
    fn advance(&mut self, delta: f32) {
        writeln!(self.input, "A {:x}", delta.to_bits()).unwrap();
        self.model
            .as_mut()
            .unwrap()
            .advance_clocks(Ticks::from_f32(delta));
        self.snapshot('A', false);
    }
    fn answer(&mut self, answer: i32) {
        writeln!(self.input, "S {answer}").unwrap();
        self.model.as_mut().unwrap().answer(answer).unwrap();
        self.snapshot('S', false);
    }
    fn input(&mut self, permission: bool, captured: bool, pressed: u32, repeat: u32) {
        writeln!(
            self.input,
            "I {} {} {pressed:x} {repeat:x}",
            u8::from(permission),
            u8::from(captured)
        )
        .unwrap();
        let mut p = permission;
        let result = self
            .model
            .as_mut()
            .unwrap()
            .input(
                &mut p,
                captured,
                Buttons {
                    pressed,
                    repeat,
                    held: u32::MAX,
                    rapid_repeat: u32::MAX,
                },
            )
            .unwrap();
        assert!(!p);
        self.snapshot('I', result);
    }
    fn probe(&mut self, x: f32, y: f32) {
        writeln!(self.input, "V {:x} {:x}", x.to_bits(), y.to_bits()).unwrap();
        let result = self.model.as_mut().unwrap().probe(x, y);
        self.snapshot('V', result);
    }
    fn release(&mut self) {
        self.input.push_str("P\n");
        self.model.as_mut().unwrap().release().unwrap();
        self.snapshot('P', false);
    }
    fn cancel(&mut self) {
        self.input.push_str("C\n");
        self.model.as_mut().unwrap().reset_contact();
        self.snapshot('C', false);
    }
    fn dispose(&mut self) {
        self.input.push_str("D\n");
        self.model.take();
        self.expected.push("D".into());
    }
}

#[test]
#[ignore = "actual9ce2c answer layout/permutation and9e78c score/marks/reactions,9e878 input/9e990 contact/9ecc0 release/9ec90 reset/native tween gates;CPU services checked,no GPU/audio/window/saves"]
fn higurashi_quiz_question_answers_controls_and_reactions_match_native() {
    let mut c = Interaction::default();
    c.input.push_str("B\n");
    for (context, voices) in catalog::REACTIONS.iter().enumerate() {
        for (variant, name) in voices.iter().enumerate() {
            let mut row = format!("B {}", context * 10 + variant);
            append_string(&mut row, name);
            c.expected.push(row);
        }
    }
    for pool in 0..8 {
        for context in 0..9 {
            for seed in [1, 17, u32::MAX] {
                let argument = [-1, 0, 1, 9, 10, i32::MIN, i32::MAX][c.owners % 7];
                c.begin([pool, context, argument], seed);
                let before = c.model.as_ref().unwrap().data.clone();
                for index in 0..10 {
                    c.prepare(index);
                    // The opening opacity blocks both controller and contacts.
                    c.input(true, false, 0xf000, 0xf000);
                    c.probe(800.0, 800.0);
                    c.release();
                    c.advance(9.5);
                    c.input(true, false, 0xf000, 0);
                    c.advance(0.5);
                    let correct = c.model.as_ref().unwrap().correct_answer;
                    let mode = (pool as usize + context as usize + index) % 4;
                    let answer = if mode % 2 == 0 {
                        correct
                    } else {
                        (correct + 1) % 4
                    };
                    match mode {
                        0 | 1 => c.answer(answer),
                        2 => c.input(true, false, 1 << (12 + answer), 0),
                        3 => {
                            let r = ANSWER_RECTS[answer as usize];
                            c.probe(r[0], r[1]);
                            c.release();
                        }
                        _ => unreachable!(),
                    }
                    let state = c.model.as_ref().unwrap().rng.state();
                    c.input(true, false, 0xf000, 0xf000);
                    c.probe(0.0, 0.0);
                    assert_eq!(c.model.as_ref().unwrap().rng.state(), state);
                }
                assert_eq!(c.model.as_ref().unwrap().data, before);
                assert_eq!(c.model.as_ref().unwrap().marks[10], 0);
                c.dispose();
            }
        }
    }
    // Raw callback repetition keeps the original score/decrement/mark rules;
    // it is the controller wrapper that prevents a second answer.
    for seed in SEEDS {
        c.begin([seed as i32 & 7, (seed % 9) as i32, 9], seed);
        for index in 0..10 {
            c.prepare(index);
            c.advance(10.0);
            let correct = c.model.as_ref().unwrap().correct_answer;
            c.answer(correct);
            c.answer(correct);
            c.answer((correct + 1) % 4);
        }
        assert_eq!(c.model.as_ref().unwrap().score, 20);
        assert_eq!(c.model.as_ref().unwrap().clamped_argument, 0);
        c.dispose();
    }
    for permission in [false, true] {
        for captured in [false, true] {
            for mask in 0..16 {
                c.begin([0, 0, 0], mask + 1);
                c.prepare(0);
                c.advance(10.0);
                c.input(permission, captured, mask << 12, 0xf000);
                c.input(true, false, 0xf000, 0xf000);
                c.dispose();
            }
        }
    }
    for bit in 0..32 {
        c.begin([0, 0, 0], 17);
        c.prepare(0);
        c.advance(10.0);
        c.input(true, false, 1 << bit, u32::MAX);
        c.dispose();
    }
    c.begin([0, 0, 0], 17);
    c.prepare(0);
    c.advance(10.0);
    for r in ANSWER_RECTS {
        for x in [
            r[0] - 0.001,
            r[0],
            r[0] + 0.001,
            r[2] - 0.001,
            r[2],
            r[2] + 0.001,
        ] {
            for y in [
                r[1] - 0.001,
                r[1],
                r[1] + 0.001,
                r[3] - 0.001,
                r[3],
                r[3] + 0.001,
            ] {
                c.probe(x, y);
            }
        }
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        c.probe(value, 800.0);
        c.probe(800.0, value);
    }
    c.probe(800.0, 800.0);
    c.cancel();
    c.release();
    c.probe(800.0, 800.0);
    c.release();
    c.release();
    c.dispose();
    assert_eq!(c.permutations.len(), 24);
    assert_eq!(c.reactions.len(), 90);
    let owners = c.owners;
    let observations = c.expected.len();
    run(c.input, c.expected, "interaction");
    eprintln!(
        "Quiz interaction:{owners} native owners/{observations} exact observations;all24 answer permutations/all90 reaction strings and slots,10-question score/marks/argument decrement,original repeated callback,input priority/permission/capture/pressed-only gates,half-open contact edges and nonfinite coordinates/native opening tween gates/full data preservation/balanced disposal pass"
    );
}
