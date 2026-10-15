use super::super::{
    autosave::{ProgressEntry, SaveProgress},
    browser_controls::{RowMetadata, Viewport},
    system_menu::Cue,
};
use super::*;
use crate::{
    adv::input_area::{PointerEvent, PointerKind},
    layer::higurashi_properties::HigurashiTweener,
};
use shin_core::format::save::PersistData;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

#[derive(Default)]
struct Service {
    events: Vec<(u32, u32)>,
    questions: Vec<Question>,
    playing: bool,
    scheduled: Vec<ProgressMemory>,
    navigation_views: Vec<NavigationViewRequest>,
}
impl browser_controls::Services for Service {
    fn event(&mut self, e: Event) {
        let value = match e {
            Event::Cue(c) => (
                match c {
                    Cue::Move => 0,
                    Cue::Cancel => 1,
                    Cue::Confirm => 2,
                },
                0,
            ),
            Event::Close | Event::Jump(_) => panic!("Lifecycle must retain feedback tasks"),
            Event::Navigation => (4, 0),
            Event::Voice(i) => (6, i as u32),
            Event::StopVoice => (7, 0),
            Event::VoiceTick => (8, 0),
            Event::MusicMultiplier(v) => (9, v.to_bits()),
        };
        self.events.push(value);
    }
    fn voice_is_playing(&self) -> bool {
        self.playing
    }
}
impl Services for Service {
    fn navigation_view(&mut self, request: NavigationViewRequest) {
        self.navigation_views.push(request);
    }
    fn returned(&mut self, complete: bool) {
        self.events.push((10, u32::from(complete)));
    }
    fn question(&mut self, q: Question) {
        assert_eq!(q.text, JUMP_TEXT);
        assert_eq!(q.buttons, JUMP_BUTTONS);
        assert_eq!(q.selected, 1);
        self.events.push((12, q.id as u32));
        self.questions.push(q);
    }
    fn question_cue(&mut self, cue: question::Cue) {
        self.events.push((
            13 + match cue {
                question::Cue::Move => 0,
                question::Cue::Cancel => 1,
                question::Cue::Confirm => 2,
                question::Cue::NoSelection => 3,
            },
            0,
        ));
    }
    fn reconstruct(&mut self, source: &ProgressMemory, mode: u32) -> Result<()> {
        assert_eq!(mode, 0);
        self.scheduled.push(source.clone());
        self.events.push((11, 0));
        Ok(())
    }
}
#[derive(Default)]
struct Compare {
    integrated: bool,
    navigation: bool,
    browser: Option<BrowserLifecycle>,
    service: Service,
    input: String,
    expected: Vec<String>,
    frame: u32,
    scenarios: usize,
    original: Option<ProgressMemory>,
}
fn tween(out: &mut String, value: &HigurashiTweener) {
    write!(
        out,
        " {} {}",
        value.value().to_bits(),
        u8::from(value.is_waiting())
    )
    .unwrap();
}
fn kind(k: Kind) -> (u32, u64) {
    match k {
        Kind::Initializer => (0, 0),
        Kind::Opening => (1, 0),
        Kind::CloseFeedback => (2, 0),
        Kind::JumpFeedback => (3, 0),
        Kind::Closing => (4, 0),
        Kind::Question(id) => (5, id),
    }
}
impl Compare {
    fn snapshot(&mut self, op: char) {
        if self.integrated {
            let pending: Vec<_> = self
                .browser
                .as_ref()
                .unwrap()
                .tasks
                .iter()
                .filter_map(|task| match task.kind {
                    Kind::Question(id) if !task.done && task.question.is_none() => Some(id),
                    _ => None,
                })
                .collect();
            for id in pending {
                self.browser
                    .as_mut()
                    .unwrap()
                    .attach_question(id, question_model(), None)
                    .unwrap();
            }
        }
        let b = self.browser.as_mut().unwrap();
        let mut out = format!(
            "L {op} 0 {} {} {} {} {}",
            b.slide.to_bits(),
            b.completion_word(),
            u8::from(b.jumped),
            u8::from(b.controls.blocked()),
            b.controls.selected()
        );
        write!(
            out,
            " {} {} {} {}",
            b.controls.scroll().to_bits(),
            u8::from(b.controls.scroll_waiting()),
            b.controls.close_flash().to_bits(),
            u8::from(b.controls.close_flash_waiting())
        )
        .unwrap();
        let cached: Vec<_> = (0..b.boundaries.len())
            .filter_map(|i| b.controls.flashes(i).map(|f| (i, f)))
            .collect();
        write!(out, " {}", cached.len()).unwrap();
        for (i, f) in cached {
            write!(out, " {i}").unwrap();
            tween(&mut out, &f.jump);
            tween(&mut out, &f.voice);
        }
        write!(out, " {}", b.tasks.len()).unwrap();
        for t in b.tasks.iter() {
            let (k, id) = kind(t.kind);
            write!(out, " {k} {} {id}", u8::from(t.done)).unwrap();
        }
        let mut draw = Vec::new();
        b.tasks.draw(|t| draw.push(kind(t.kind)));
        write!(out, " {}", draw.len()).unwrap();
        for (k, id) in draw {
            write!(out, " {k} {id}").unwrap();
        }
        write!(out, " {}", b.inputs.len()).unwrap();
        for t in b.inputs.iter() {
            write!(
                out,
                " {} {} 0",
                if t.navigation.is_some() { 7 } else { 6 },
                u8::from(t.done)
            )
            .unwrap();
        }
        let mut input_draw = Vec::new();
        b.inputs
            .draw(|t| input_draw.push(if t.navigation.is_some() { 7 } else { 6 }));
        write!(out, " {}", input_draw.len()).unwrap();
        for kind in input_draw {
            write!(out, " {kind} 0").unwrap();
        }
        let p = b.source.progress();
        write!(out, " {} {}", p.active_index(), p.extra_word).unwrap();
        for e in &p.entries {
            write!(
                out,
                " {} {} {} {} {}",
                e.scenario_id as u32,
                e.random_seed,
                e.save_position,
                e.replay_target,
                e.response_data.len()
            )
            .unwrap();
            for v in &e.response_data {
                write!(out, " {v}").unwrap();
            }
        }
        write!(out, " {}", self.service.events.len()).unwrap();
        for (k, a) in self.service.events.drain(..) {
            write!(out, " {k} {a}").unwrap();
        }
        self.expected.push(out);
        if self.integrated {
            let mut out = String::from("I 0");
            for task in b.tasks.iter() {
                if let (Kind::Question(id), Some(owner)) = (task.kind, &task.question) {
                    let model = &owner.model;
                    out = format!(
                        "I {id} {} {} {} {} {}",
                        model.phase(),
                        model.selected(),
                        model.scale().to_bits(),
                        model.opacity().to_bits(),
                        model.buttons().len()
                    );
                    for button in model.buttons() {
                        tween(&mut out, &button.focus);
                        tween(&mut out, &button.flash);
                    }
                }
            }
            self.expected.push(out);
        }
        if self.navigation {
            let owners: Vec<_> = b
                .inputs
                .iter()
                .filter_map(|t| t.navigation.as_ref())
                .collect();
            let mut out = format!("O {}", owners.len());
            for owner in owners {
                write!(
                    out,
                    " {} {} {} {} {}",
                    owner.model.phase(),
                    owner.model.slide().to_bits(),
                    owner.model.selected(),
                    owner.model.scroll().to_bits(),
                    u8::from(owner.model.scroll_waiting())
                )
                .unwrap();
            }
            write!(out, " {}", b.controls.navigation()).unwrap();
            self.expected.push(out);
        }
    }
    fn begin(
        &mut self,
        count: usize,
        selected: usize,
        top: f32,
        height: f32,
        minimum: f32,
        cached: bool,
        active: usize,
    ) {
        assert!(self.browser.is_none());
        self.service = Service::default();
        self.frame = 0;
        self.scenarios += 1;
        let extent = top + count as f32 * height;
        let rows: Vec<_> = (0..count)
            .map(|i| RowMetadata {
                kind: 0,
                voice_count: 1,
                top: top + i as f32 * height,
                height,
            })
            .collect();
        let boundaries: Vec<_> = (0..count).map(|i| 77 + i as u32 * 31).collect();
        let entries = std::array::from_fn(|i| ProgressEntry {
            scenario_id: if i == 0 { 1 } else { -7 },
            random_seed: 42 + i as u32,
            save_position: 23 + i as u32 * 58,
            replay_target: 24 + i as u32 * 58,
            response_data: if i == 0 {
                vec![1, 255, 8, 99]
            } else {
                (0..16).map(|n| n * 11).collect()
            },
        });
        let progress = SaveProgress::new(active, 0x5a17, entries).unwrap();
        let source = ProgressMemory::new(
            PersistData::default(),
            progress,
            (0..16).map(|i| i * 13 + 7).collect(),
        );
        self.original = Some(source.clone());
        writeln!(
            self.input,
            "N {count} 0 {count} {selected} {minimum} {extent} 0 {active}"
        )
        .unwrap();
        let cached_indices: Vec<_> = (0..count.min(64)).filter(|_| cached).collect();
        for (i, r) in rows.iter().enumerate() {
            writeln!(
                self.input,
                "{} {} {} {} {} {}",
                r.kind,
                r.voice_count,
                r.top,
                r.height,
                u8::from(cached_indices.contains(&i)),
                boundaries[i]
            )
            .unwrap();
        }
        for e in &source.progress().entries {
            write!(
                self.input,
                "{} {} {} {} {}",
                e.scenario_id as u32,
                e.random_seed,
                e.save_position,
                e.replay_target,
                e.response_data.len()
            )
            .unwrap();
            for v in &e.response_data {
                write!(self.input, " {v}").unwrap();
            }
            self.input.push('\n');
        }
        let mut controls = BrowserControls::new(
            rows,
            Viewport {
                first: 0,
                end: count,
                minimum,
                extent,
            },
            selected,
            0.0,
        )
        .unwrap();
        controls.set_cached_rows(cached_indices);
        self.browser = Some(BrowserLifecycle::new(controls, boundaries, source).unwrap());
        self.snapshot('N');
    }
    fn ready(&mut self, status: u32, active: bool) {
        writeln!(self.input, "A {status} {}", u8::from(active)).unwrap();
        self.browser
            .as_mut()
            .unwrap()
            .publish_readiness(status, active);
        self.snapshot('A');
    }
    fn navigation(&mut self, current: usize, markers: Vec<u32>) {
        assert!(self.navigation);
        write!(self.input, "V {current} {}", markers.len()).unwrap();
        for i in &markers {
            write!(self.input, " {i}").unwrap();
        }
        self.input.push('\n');
        let b = self.browser.as_mut().unwrap();
        let indices = (0..b.controls.row_count()).map(|i| i as u32).collect();
        b.configure_navigation(NavigationHistory::new(indices, markers, current).unwrap())
            .unwrap();
        self.snapshot('V');
    }
    fn tick(&mut self, delta: f32, permission: u32, pressed: u32, events: &[PointerEvent]) {
        self.tick_repeated(delta, permission, pressed, 0, events);
    }
    fn tick_repeated(
        &mut self,
        delta: f32,
        permission: u32,
        pressed: u32,
        repeated: u32,
        events: &[PointerEvent],
    ) {
        self.frame += 1;
        let micros = self.frame as u64 * 16667;
        let playing = self.frame % 3 == 0;
        writeln!(
            self.input,
            "F {} {micros} {permission} {pressed:x} {repeated:x} {delta} 1 {} {}",
            self.frame,
            u8::from(playing),
            events.len()
        )
        .unwrap();
        for e in events {
            writeln!(
                self.input,
                "{} {} {} {}",
                e.kind as i32, e.id, e.position.x, e.position.y
            )
            .unwrap();
        }
        self.service.playing = playing;
        let frame = PointerFrame {
            number: self.frame,
            time_seconds: micros as f64 / 1e6,
            events,
        };
        assert_eq!(
            self.browser
                .as_mut()
                .unwrap()
                .poll(
                    Ticks::from_f32(delta),
                    permission,
                    pressed,
                    repeated,
                    &frame,
                    true,
                    &mut self.service
                )
                .unwrap(),
            0
        );
        self.snapshot('F');
    }
    fn frames(&mut self, count: usize, delta: f32) {
        for _ in 0..count {
            self.tick(delta, 1, 0, &[]);
        }
    }
    fn callback(&mut self, id: u64, stage: u32, button: i32) {
        writeln!(self.input, "Q {id} {stage} {button}").unwrap();
        self.browser
            .as_mut()
            .unwrap()
            .question_callback(id, stage, button)
            .unwrap();
        self.snapshot('Q');
    }
    fn retire(&mut self, id: u64) {
        writeln!(self.input, "E {id}").unwrap();
        self.browser.as_mut().unwrap().finish_question(id).unwrap();
        self.snapshot('E');
    }
    fn finish(&mut self) {
        let b = self.browser.take().unwrap();
        let original = self.original.take().unwrap();
        let inactive = 1 - original.progress().active_index();
        assert_eq!(
            b.source.progress().entries[inactive],
            original.progress().entries[inactive]
        );
        assert_eq!(b.source.responses(), original.responses());
        assert_eq!(b.source.bytes(), original.bytes());
        assert_eq!(b.source.persistent().0, original.persistent().0);
        for source in &self.service.scheduled {
            assert_eq!(source.progress(), b.source.progress());
            assert_eq!(source.bytes(), original.bytes());
        }
        self.input.push_str("D\n");
    }
    fn verify(self) {
        assert!(self.browser.is_none());
        let native = std::env::var(if self.navigation {
            "SHIN_HIGURASHI_NATIVE_BROWSER_NAVIGATION_OWNER"
        } else if self.integrated {
            "SHIN_HIGURASHI_NATIVE_BROWSER_QUESTION"
        } else {
            "SHIN_HIGURASHI_NATIVE_BROWSER_LIFECYCLE"
        })
        .expect("native lifecycle oracle");
        let exe = std::env::var("SHIN_HIGURASHI_EXECUTABLE")
            .unwrap_or_else(|_| "higurashi/uncompressed_executable_update".into());
        let font = std::env::var("SHIN_HIGURASHI_FONT")
            .unwrap_or_else(|_| "/tmp/higurashi-newrodin.fnt".into());
        let mut child = Command::new(native)
            .args([exe, font])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = self.input.clone();
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        let dir = std::env::var("SHIN_HIGURASHI_BROWSER_LIFECYCLE_CAPTURE").unwrap_or_else(|_| {
            if self.navigation {
                "/tmp/higurashi-browser-navigation-owner-packets".into()
            } else if self.integrated {
                "/tmp/higurashi-browser-question-packets".into()
            } else {
                "/tmp/higurashi-browser-lifecycle-packets".into()
            }
        });
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("input.txt"), self.input).unwrap();
        std::fs::write(dir.join("native.txt"), &output.stdout).unwrap();
        std::fs::write(dir.join("native.log"), &output.stderr).unwrap();
        std::fs::write(dir.join("rust.txt"), self.expected.join("\n") + "\n").unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = String::from_utf8(output.stdout).unwrap();
        let rows: Vec<_> = actual.lines().collect();
        assert_eq!(rows.len(), self.expected.len());
        for (i, (native, rust)) in rows.iter().zip(&self.expected).enumerate() {
            assert_eq!(*native, rust, "lifecycle observation{i}");
        }
        println!(
            "Higurashi browser lifecycle:{} scenarios/{} exact native observations; initializer, retained task/draw snapshots, opening/input/feedback/question/callback/closing/reconstruction-copy ordering pass",
            self.scenarios,
            self.expected.len()
        );
    }
}

fn question_model() -> question::Question {
    use shin_core::format::font::read_font_metrics;
    let font = std::sync::Arc::new(
        read_font_metrics(&mut std::io::BufReader::new(
            std::fs::File::open(
                std::env::var("SHIN_HIGURASHI_FONT")
                    .unwrap_or_else(|_| "/tmp/higurashi-newrodin.fnt".into()),
            )
            .unwrap(),
        ))
        .unwrap(),
    );
    question::Question::from_text(font, JUMP_TEXT, &JUMP_BUTTONS, 1, true).unwrap()
}

#[test]
#[ignore = "original browser/navigation/question stacks and8162c callbacks execute together"]
fn higurashi_browser_retains_navigation_and_accepted_question_closing() {
    let mut c = Compare {
        integrated: true,
        navigation: true,
        ..Default::default()
    };
    for active in [0, 1] {
        for cached in [false, true] {
            for delta in [0.25, 1.0, 20.0] {
                for choice in [0, 1, -1] {
                    c.begin(4, 2, -800.0, 100.0, -800.0, cached, active);
                    c.navigation(1, vec![0, 1, 2, 3]);
                    c.ready(2, false);
                    c.frames(12, 1.0);
                    c.tick(delta, 1, 1 << 12, &[]);
                    let b = c.browser.as_mut().unwrap();
                    let mut draw = 0;
                    b.draw_navigation(|_| draw += 1);
                    assert_eq!(draw, 0);
                    assert_eq!(b.controls.navigation(), 1);
                    c.frames(6, 1.0);
                    c.tick_repeated(delta, 1, 1 << 6, 1 << 6, &[]);
                    c.frames(5, 1.0);
                    let row = c.browser.as_ref().unwrap().controls.selected();
                    c.tick(delta, 1, 1 << 15, &[]);
                    while c.service.questions.is_empty() {
                        c.frames(1, delta);
                    }
                    for _ in 0..100 {
                        if c.browser
                            .as_ref()
                            .unwrap()
                            .tasks
                            .iter()
                            .any(|t| t.question.as_ref().is_some_and(|q| q.model.phase() == 2))
                        {
                            break;
                        }
                        c.frames(1, delta);
                    }
                    if choice == 0 {
                        c.tick_repeated(delta, 1, 1 << 7, 1 << 7, &[]);
                    }
                    c.tick(delta, 1, 1 << if choice == -1 { 25 } else { 24 }, &[]);
                    for _ in 0..180 {
                        if c.browser
                            .as_ref()
                            .unwrap()
                            .tasks
                            .iter()
                            .any(|t| t.question.as_ref().is_some_and(|q| q.model.complete()))
                        {
                            break;
                        }
                        c.frames(1, delta);
                    }
                    let b = c.browser.as_ref().unwrap();
                    assert_eq!(b.jumped(), choice == 0);
                    let nav = b.inputs.iter().find_map(|t| t.navigation.as_ref()).unwrap();
                    assert_eq!(
                        nav.model.phase(),
                        if choice == 0 {
                            if delta >= 5.0 { -1 } else { 2 }
                        } else {
                            1
                        }
                    );
                    // The question's final stage1 callback changes navigation
                    // before the later input stack polls in this same frame.
                    if choice == 0 {
                        assert!(nav.model.slide() < 1.0);
                    } else {
                        assert_eq!(
                            b.private_source().progress(),
                            c.original.as_ref().unwrap().progress()
                        );
                    }
                    if choice != 0 {
                        c.frames(1, delta);
                        c.tick(delta, 1, 1 << 25, &[]);
                    }
                    c.frames(70, delta);
                    let b = c.browser.as_mut().unwrap();
                    assert_eq!(b.controls.navigation(), 0);
                    let mut draw = 0;
                    b.draw_navigation(|_| draw += 1);
                    assert_eq!(draw, 0);
                    if choice == 0 {
                        assert_eq!(
                            b.private_source().progress().active_entry().save_position,
                            b.boundaries[row]
                        );
                        assert_eq!(c.service.scheduled.len(), 1);
                        assert_eq!(b.slide(), 0.0);
                    } else {
                        assert!(c.service.scheduled.is_empty());
                        assert_eq!(b.slide(), 1.0);
                    }
                    c.finish();
                }
            }
        }
    }
    // Touching the shared close button requests parent close and navigation
    // close together; cancelling only the navigation leaves the browser open.
    for close in [false, true] {
        c.begin(4, 2, -800.0, 100.0, -800.0, true, 0);
        c.navigation(1, vec![0, 1, 2, 3]);
        c.ready(2, false);
        c.frames(12, 1.0);
        c.tick(1.0, 1, 1 << 12, &[]);
        c.frames(7, 1.0);
        if close {
            for kind in [PointerKind::Down, PointerKind::Up] {
                c.tick(
                    1.0,
                    1,
                    0,
                    &[PointerEvent {
                        kind,
                        id: 4,
                        position: glam::ivec2(1200, 40),
                    }],
                );
            }
        } else {
            c.tick(1.0, 1, 1 << 25, &[]);
        }
        let mut final_draw = false;
        for _ in 0..40 {
            let b = c.browser.as_mut().unwrap();
            b.draw_navigation(|nav| {
                if nav.complete() {
                    final_draw = true;
                }
            });
            c.frames(1, 1.0);
        }
        assert!(final_draw);
        assert_eq!(
            c.browser.as_ref().unwrap().completion_word(),
            u32::from(close)
        );
        c.finish();
    }
    for end in [0, 1, 3, 6, 12, 18, 24, 40] {
        c.begin(24, 12, -800.0, 100.0, -800.0, true, 0);
        c.navigation(13, (0..24).collect());
        c.ready(2, false);
        c.frames(12, 1.0);
        c.tick(1.0, 1, 1 << 12, &[]);
        for i in 0..end {
            c.tick(1.0, 1, if i == 10 { 1 << 15 } else { 0 }, &[]);
        }
        c.finish();
    }
    c.verify();
}

#[test]
#[ignore = "runs actual updated browser stacks and question constructor/poll/destructor together"]
fn higurashi_browser_retains_question_and_staged_jump_callbacks() {
    let mut c = Compare {
        integrated: true,
        ..Default::default()
    };
    for active in [0, 1] {
        for cached in [false, true] {
            for delta in [0.25, 1.0, 20.0] {
                for choice in [0, 1, -1] {
                    c.begin(4, 2, -800.0, 100.0, -800.0, cached, active);
                    c.ready(2, false);
                    c.frames(12, 1.0);
                    c.tick(delta, 1, 1 << 15, &[]);
                    while c.service.questions.is_empty() {
                        c.tick(delta, 1, 0, &[]);
                    }
                    // Newly created question is owned immediately, but absent
                    // from the borrowed draw snapshot until the next poll.
                    let b = c.browser.as_mut().unwrap();
                    let mut draw = Vec::new();
                    b.draw_questions(|id| draw.push(id));
                    assert!(draw.is_empty());
                    assert!(b.attach_question(1, question_model(), None).is_err());
                    assert!(b.question_callback(1, 1, 0).is_err());
                    assert!(b.finish_question(1).is_err());
                    loop {
                        let phase = c
                            .browser
                            .as_ref()
                            .unwrap()
                            .tasks
                            .iter()
                            .find_map(|t| t.question.as_ref().map(|q| q.model.phase()))
                            .unwrap();
                        if phase == 2 {
                            break;
                        }
                        c.tick(delta, 1, 0, &[]);
                    }
                    // A blocked permission low bit cannot confirm or cancel.
                    c.tick(delta, 2, (1 << 24) | (1 << 25), &[]);
                    if choice == 0 {
                        c.tick_repeated(delta, 1, 1 << 7, 1 << 7, &[]);
                    }
                    c.tick(delta, 3, 1 << if choice == -1 { 25 } else { 24 }, &[]);
                    let mut saw_closing = false;
                    for _ in 0..180 {
                        let b = c.browser.as_ref().unwrap();
                        let phase = b
                            .tasks
                            .iter()
                            .find_map(|t| t.question.as_ref().map(|q| q.model.phase()))
                            .unwrap();
                        if phase == 4 {
                            saw_closing = true;
                            assert!(!b.jumped());
                            assert_eq!(
                                b.private_source().progress(),
                                c.original.as_ref().unwrap().progress()
                            );
                        }
                        if phase == -1 {
                            break;
                        }
                        c.tick(delta, 1, 0, &[]);
                    }
                    assert!(saw_closing);
                    let b = c.browser.as_mut().unwrap();
                    assert!(
                        b.tasks
                            .iter()
                            .any(|t| t.question.as_ref().is_some_and(|q| q.model.complete()))
                    );
                    assert_eq!(b.jumped(), choice == 0);
                    let mut draw = Vec::new();
                    b.draw_questions(|id| draw.push(id));
                    assert_eq!(draw, [1]); // completing frame remains drawable
                    c.tick(delta, 1, 0, &[]);
                    let b = c.browser.as_mut().unwrap();
                    let mut draw = Vec::new();
                    b.draw_questions(|id| draw.push(id));
                    assert!(draw.is_empty());
                    assert!(b.attach_question(1, question_model(), None).is_err());
                    c.frames(45, delta);
                    let b = c.browser.as_ref().unwrap();
                    if choice == 0 {
                        assert_eq!(b.source.progress().active_entry().save_position, 139);
                        assert_eq!(c.service.scheduled.len(), 1);
                        assert_eq!(b.slide(), 0.0);
                        assert_eq!(b.completion_word(), 0);
                    } else {
                        assert_eq!(
                            b.private_source().progress(),
                            c.original.as_ref().unwrap().progress()
                        );
                        assert!(c.service.scheduled.is_empty());
                        assert_eq!(b.slide(), 1.0);
                    }
                    c.finish();
                }
            }
        }
    }
    // Destroy each owner while the concrete dialog is retained at different
    // animation stages; native deleting destructors must balance all owners.
    for end in [0, 1, 4, 14, 17, 29] {
        c.begin(4, 0, -800.0, 100.0, -800.0, true, 0);
        c.ready(2, false);
        c.frames(12, 1.0);
        c.tick(1.0, 1, 1 << 15, &[]);
        while c.service.questions.is_empty() {
            c.frames(1, 1.0);
        }
        for i in 0..end {
            c.tick(1.0, 1, if i == 14 { 1 << 25 } else { 0 }, &[]);
        }
        c.finish();
    }
    c.verify();
}

#[test]
#[ignore = "actual question asset publication and retained cache drawing on Metal; no audio or saves"]
fn higurashi_browser_question_publication_and_draw_snapshot_on_gpu() {
    use super::super::message_glyphs::tests::gpu::{gpu_assets, render_prepared_frame};
    use shin_core::primitives::color::UnormColor;
    let (device, queue, assets) = gpu_assets();
    let mut c = Compare::default();
    c.begin(4, 2, -800.0, 100.0, -800.0, true, 0);
    c.ready(2, false);
    c.frames(12, 1.0);
    c.tick(1.0, 1, 1 << 15, &[]);
    while c.service.questions.is_empty() {
        c.frames(1, 1.0);
    }
    let request = c.service.questions[0];
    let mut other = Compare::default();
    other.begin(4, 2, -800.0, 100.0, -800.0, true, 0);
    other.ready(2, false);
    other.frames(12, 1.0);
    other.tick(1.0, 1, 1 << 15, &[]);
    while other.service.questions.is_empty() {
        other.frames(1, 1.0);
    }
    assert_eq!(other.service.questions[0].id, request.id);
    let misplaced = shin_tasks::block_on(request.load(&assets)).unwrap();
    assert!(
        other
            .browser
            .as_mut()
            .unwrap()
            .publish_question(misplaced)
            .is_err()
    );
    other.finish();
    let loaded = shin_tasks::block_on(request.load(&assets)).unwrap();
    c.browser
        .as_mut()
        .unwrap()
        .publish_question(loaded)
        .unwrap();
    let render = |browser: &mut BrowserLifecycle, size| {
        let browser = std::cell::RefCell::new(browser);
        render_prepared_frame(
            &device,
            &queue,
            size,
            UnormColor::WHITE,
            |context| browser.borrow_mut().prepare_questions(context),
            |pass| browser.borrow_mut().render_questions(pass),
        )
    };
    let sizes = [(1280, 720), (1920, 1080)];
    // Publication must not extend the last poll's draw snapshot.
    for size in sizes {
        let pixels = render(c.browser.as_mut().unwrap(), size);
        assert!(pixels.iter().all(|v| *v == 255));
    }
    let mut visible = 0;
    for step in 0..45 {
        c.tick(1.0, 1, if step == 16 { 1 << 24 } else { 0 }, &[]);
        if ![0, 6, 14, 17, 24, 32, 44].contains(&step) {
            continue;
        }
        for size in sizes {
            let b = c.browser.as_mut().unwrap();
            let pixels = render(b, size);
            // Renderer arithmetic already has independent native packet/GPU
            // evidence. Compare retained dispatch against that renderer here.
            let mut reference = None;
            b.tasks.draw(|task| {
                if let Some(RetainedQuestion {
                    model,
                    view: Some(view),
                }) = &task.question
                {
                    reference = Some(render_prepared_frame(
                        &device,
                        &queue,
                        size,
                        UnormColor::WHITE,
                        |context| view.prepare(context, model),
                        |pass| view.render(pass, model),
                    ));
                }
            });
            if let Some(reference) = reference {
                assert_eq!(
                    pixels
                        .iter()
                        .zip(&reference)
                        .filter(|(a, b)| a != b)
                        .count(),
                    0
                );
                if pixels.iter().any(|v| *v != 255) {
                    visible += 1;
                }
            } else {
                assert!(pixels.iter().all(|v| *v == 255));
            }
        }
    }
    assert!(visible >= 6);
    assert!(
        c.browser
            .as_ref()
            .unwrap()
            .tasks
            .iter()
            .all(|t| t.question.is_none())
    );
    // A cancelled pending owner rejects the successful late GPU load. A new
    // question cannot inherit that result merely because the text is equal.
    c.tick(1.0, 1, 1 << 15, &[]);
    while c.service.questions.len() < 2 {
        c.frames(1, 1.0);
    }
    let late = shin_tasks::block_on(c.service.questions[1].load(&assets)).unwrap();
    c.browser.as_mut().unwrap().finish_question(2).unwrap();
    assert!(c.browser.as_mut().unwrap().publish_question(late).is_err());
    c.frames(1, 1.0);
    for size in sizes {
        assert!(
            render(c.browser.as_mut().unwrap(), size)
                .iter()
                .all(|v| *v == 255)
        );
    }
    c.finish();
    println!(
        "Retained question GPU:18 cache/composition frames at720p/1080p; publication, borrowed draw lifetime and expired load rejection pass"
    );
}

#[test]
#[ignore = "loads native navigation assets and checks retained offscreen GPU ownership"]
fn higurashi_browser_navigation_publication_and_draw_snapshot_on_gpu() {
    use super::super::{
        browser_history::{Item, Row},
        message_glyphs::tests::gpu::{gpu_assets, render_prepared_frame},
    };
    use shin_core::primitives::color::UnormColor;
    let (device, queue, assets) = gpu_assets();
    let start = || {
        let mut c = Compare::default();
        c.begin(20, 10, -800.0, 100.0, -800.0, true, 0);
        let rows: Arc<[Row]> = (0..20)
            .map(|i| Row {
                index: i,
                boundary: 77 + i * 31,
                item: Item::Message {
                    text: format!("履歴 {i}"),
                    style: 0,
                    argument: None,
                },
            })
            .collect();
        c.browser
            .as_mut()
            .unwrap()
            .configure_navigation_rendering(rows, (0..20).collect(), 10)
            .unwrap();
        c.ready(2, false);
        c.frames(12, 1.0);
        c.tick(1.0, 1, 1 << 12, &[]);
        assert_eq!(c.service.navigation_views.len(), 1);
        c
    };
    let mut c = start();
    let request = c.service.navigation_views[0].clone();
    let mut other = start();
    assert_eq!(other.service.navigation_views[0].id, request.id);
    let misplaced = shin_tasks::block_on(request.clone().load(&assets)).unwrap();
    assert!(
        other
            .browser
            .as_mut()
            .unwrap()
            .publish_navigation_view(misplaced)
            .unwrap_err()
            .to_string()
            .contains("another browser")
    );
    other.finish();
    let loaded = shin_tasks::block_on(request.clone().load(&assets)).unwrap();
    c.browser
        .as_mut()
        .unwrap()
        .publish_navigation_view(loaded)
        .unwrap();
    let duplicate = shin_tasks::block_on(request.load(&assets)).unwrap();
    assert!(
        c.browser
            .as_mut()
            .unwrap()
            .publish_navigation_view(duplicate)
            .unwrap_err()
            .to_string()
            .contains("already published")
    );
    let render = |browser: &mut BrowserLifecycle, size| {
        browser.prepare_navigation_views(&assets).unwrap();
        render_prepared_frame(
            &device,
            &queue,
            size,
            UnormColor::WHITE,
            |_| {},
            |pass| browser.render_navigation_views(pass),
        )
    };
    let sizes = [(1280, 720), (1920, 1080)];
    let mut frames = 0;
    // A freshly pushed child is absent from the preceding borrowed draw list.
    for size in sizes {
        assert!(
            render(c.browser.as_mut().unwrap(), size)
                .iter()
                .all(|v| *v == 255)
        );
        frames += 1;
    }
    let mut visible = 0;
    let mut completing = false;
    for step in 0..18 {
        c.tick(1.0, 1, if step == 7 { 1 << 25 } else { 0 }, &[]);
        if ![0, 4, 7, 9, 11, 12, 13, 14, 17].contains(&step) {
            continue;
        }
        for size in sizes {
            let b = c.browser.as_mut().unwrap();
            let pixels = render(b, size);
            let mut reference = None;
            b.inputs.draw(|task| {
                if let Some(owner) = &task.navigation {
                    let view = owner.view.as_ref().unwrap();
                    completing |= owner.model.complete();
                    reference = Some(render_prepared_frame(
                        &device,
                        &queue,
                        size,
                        UnormColor::WHITE,
                        |_| {},
                        |pass| view.render(pass, &owner.model, &b.controls),
                    ));
                }
            });
            if let Some(reference) = reference {
                assert_eq!(
                    pixels
                        .iter()
                        .zip(&reference)
                        .filter(|(a, b)| a != b)
                        .count(),
                    0
                );
                visible += usize::from(pixels.iter().any(|v| *v != 255));
            } else {
                assert!(pixels.iter().all(|v| *v == 255));
            }
            frames += 1;
        }
    }
    assert!(visible >= 6);
    assert!(
        completing,
        "Completing child retains its view for the last draw"
    );
    assert_eq!(c.service.navigation_views.len(), 1);
    assert_eq!(c.browser.as_ref().unwrap().controls.navigation(), 0);
    assert!(
        c.browser
            .as_ref()
            .unwrap()
            .inputs
            .iter()
            .all(|task| task.navigation.is_none())
    );
    // A completed load must not migrate to a later child in the same browser.
    c.tick(1.0, 1, 1 << 12, &[]);
    let late = shin_tasks::block_on(c.service.navigation_views[1].clone().load(&assets)).unwrap();
    c.frames(6, 1.0);
    c.tick(1.0, 1, 1 << 25, &[]);
    c.frames(8, 1.0);
    assert_eq!(c.browser.as_ref().unwrap().controls.navigation(), 0);
    c.tick(1.0, 1, 1 << 12, &[]);
    assert!(
        c.browser
            .as_mut()
            .unwrap()
            .publish_navigation_view(late)
            .unwrap_err()
            .to_string()
            .contains("Expired")
    );
    let loaded = shin_tasks::block_on(c.service.navigation_views[2].clone().load(&assets)).unwrap();
    c.browser
        .as_mut()
        .unwrap()
        .publish_navigation_view(loaded)
        .unwrap();
    for size in sizes {
        assert!(
            render(c.browser.as_mut().unwrap(), size)
                .iter()
                .all(|v| *v == 255)
        );
        frames += 1;
    }
    c.frames(1, 1.0);
    for size in sizes {
        assert!(
            render(c.browser.as_mut().unwrap(), size)
                .iter()
                .any(|v| *v != 255)
        );
        frames += 1;
    }
    assert_eq!(c.service.navigation_views.len(), 3);
    c.finish();
    println!(
        "Retained navigation GPU:{frames} frames at720p/1080p; owner identity, duplicate/late publication, borrowed draw lifetime and reopening pass"
    );
}

#[test]
#[ignore = "runs updated native browser modal tasks and bounded question/reconstruction service owners"]
fn higurashi_browser_lifecycle_matches_native_opening_closing_and_jump_handoff() {
    let mut c = Compare::default();
    // Atlas status must be exactly2. A short history cannot open while worker
    // publication is active. Opening pushes input into the later stack in the
    // same poll, but that input still receives blocked permission this frame.
    for status in [0, 1, 3, u32::MAX] {
        c.begin(4, 0, -800.0, 100.0, -800.0, true, 0);
        c.ready(status, false);
        c.frames(3, 4.0);
        assert_eq!(c.browser.as_ref().unwrap().slide(), 0.0);
        c.finish();
    }
    for count in [1, 4, 99, 100, 101] {
        c.begin(count, 0, -800.0, 100.0, -800.0, false, 0);
        c.ready(2, true);
        c.frames(3, 4.0);
        assert_eq!(
            c.browser.as_ref().unwrap().slide(),
            if count >= 100 { 0.8 } else { 0.0 }
        );
        c.ready(2, false);
        c.frames(6, 4.0);
        c.finish();
    }
    // Document anchoring includes above/below viewport, exact818 edges, small
    // extents, negative/fractional minimum and both private progress entries.
    for (top, height, minimum) in [
        (-1000.0, 100.0, -1200.0),
        (-800.0, 18.0, -800.0),
        (-799.75, 17.75, -799.5),
        (-799.0, 900.0, -800.0),
        (-0.25, 18.25, -900.0),
        (0.0, 18.0, -800.0),
        (0.0, 18.25, -800.0),
        (100.0, 1200.0, -300.25),
    ] {
        for selected in [0, 3] {
            for cached in [false, true] {
                for active in [0, 1] {
                    c.begin(4, selected, top, height, minimum, cached, active);
                    c.ready(2, false);
                    for delta in [0.0, 0.25, 1.0, 4.0, 17.0] {
                        c.tick(delta, 1, 1 << 25, &[]);
                    }
                    c.tick(1.0, 1, 1 << 25, &[]);
                    c.frames(28, 1.0);
                    assert_eq!(c.browser.as_ref().unwrap().completion_word(), 1);
                    assert_eq!(c.service.scheduled.len(), 0);
                    c.finish();
                }
            }
        }
    }
    for cached in [false, true] {
        for active in [0, 1] {
            for delta in [0.25, 1.0, 4.0, 20.0] {
                for (stage, button) in [(0, 0), (1, 0), (1, 1), (2, 0), (u32::MAX, 0), (1, -1)] {
                    c.begin(4, 2, -800.0, 100.0, -800.0, cached, active);
                    c.ready(2, false);
                    c.frames(12, 1.0);
                    // Even words with a set second bit cannot enable the native input task.
                    c.tick(1.0, 2, 1 << 15, &[]);
                    assert!(c.service.questions.is_empty());
                    c.tick(delta, 3, 1 << 15, &[]);
                    for _ in 0..100 {
                        if !c.service.questions.is_empty() {
                            break;
                        }
                        c.tick(delta, 1, 1 << 25, &[]);
                    }
                    assert_eq!(c.service.questions.len(), 1);
                    assert_eq!(c.service.questions[0].id, 1);
                    c.frames(3, delta);
                    c.callback(1, stage, button);
                    // Native callback and dialog retirement are separate. Accepted jump
                    // slides can run above the still-retained question UI owner.
                    c.frames(2, delta);
                    c.retire(1);
                    c.frames(50, delta);
                    let b = c.browser.as_ref().unwrap();
                    assert_eq!(b.jumped(), stage == 1 && button == 0);
                    if b.jumped() {
                        assert_eq!(b.completion_word(), 0);
                        assert_eq!(c.service.scheduled.len(), 1);
                        let e = b.source.progress().active_entry();
                        assert_eq!(e.save_position, 139);
                        assert_eq!(
                            e.replay_target,
                            c.original
                                .as_ref()
                                .unwrap()
                                .progress()
                                .active_entry()
                                .save_position
                        );
                    } else {
                        assert_eq!(b.slide(), 1.0);
                        assert!(c.service.scheduled.is_empty());
                        assert_eq!(b.source.progress(), c.original.as_ref().unwrap().progress());
                    }
                    c.finish();
                }
            }
        }
    }
    // Real contact callbacks create the alternate native jump callback; close
    // touch and physical capture also traverse the retained input task.
    for close in [false, true] {
        c.begin(4, 0, -800.0, 100.0, -800.0, true, 0);
        c.ready(2, false);
        c.frames(12, 1.0);
        let position = if close {
            glam::ivec2(1200, 40)
        } else {
            glam::ivec2(500, 130)
        };
        c.tick(
            1.0,
            1,
            0,
            &[PointerEvent {
                kind: PointerKind::Down,
                id: 4,
                position,
            }],
        );
        c.tick(
            1.0,
            1,
            0,
            &[PointerEvent {
                kind: PointerKind::Up,
                id: 4,
                position,
            }],
        );
        c.frames(10, 1.0);
        if !close {
            assert_eq!(c.service.questions.len(), 1);
            c.callback(1, 1, 0);
            c.retire(1);
        }
        c.frames(20, 1.0);
        c.finish();
    }
    // Owner cancellation at each retained phase must balance native tasks,
    // copied question functions and tween allocations without a callback.
    for end in [0, 1, 2, 6, 12, 14, 18, 24, 28] {
        c.begin(4, 0, -800.0, 100.0, -800.0, true, 0);
        c.ready(2, false);
        for i in 0..end {
            c.tick(1.0, 1, if i == 13 { 1 << 15 } else { 0 }, &[]);
        }
        c.finish();
    }
    c.verify();
}
