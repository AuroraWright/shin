use super::*;
use crate::adv::higurashi::input::Buttons;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

struct Host {
    buttons: Buttons,
    captured: bool,
    events: Vec<Event>,
    seeds: u32,
    dates: u32,
    retired: u32,
}
impl Services for Host {
    fn input(&mut self, model: &mut Model, permission: bool) {
        model.poll_controls(permission, self.captured, self.buttons);
    }
    fn fresh_date(&mut self) -> Result<SaveDate> {
        self.dates += 1;
        Ok(SaveDate::now())
    }
    fn seed(&mut self) -> Result<u32> {
        self.seeds += 1;
        Ok(0x8badf00d)
    }
    fn event(&mut self, event: Event) -> Result<()> {
        self.events.push(event);
        Ok(())
    }
    fn retire_first(&mut self) {
        self.retired += 1;
    }
}
fn data(
    out: &mut String,
    data: &crate::app::higurashi::runtime_game_data::RuntimeGameData,
    unknown: bool,
) {
    write!(
        out,
        " {} {}",
        data.progress.active_index(),
        data.progress.extra_word
    )
    .unwrap();
    for (i, e) in data.progress.entries.iter().enumerate() {
        for v in [
            e.scenario_id as u32,
            e.random_seed,
            e.save_position,
            e.replay_target,
        ] {
            if unknown && i == 1 {
                out.push_str(" *");
            } else {
                write!(out, " {v}").unwrap();
            }
        }
        write!(out, " {}", e.response_data.len()).unwrap();
        for v in &e.response_data {
            write!(out, " {v}").unwrap();
        }
    }
    write!(out, " {}", data.trailing_values.len()).unwrap();
    for v in &data.trailing_values {
        write!(out, " {v}").unwrap();
    }
    write!(out, " {}", data.byte_variables.len()).unwrap();
    for v in &data.byte_variables {
        write!(out, " {v}").unwrap();
    }
}
struct Profile {
    motion: Motion,
    host: Host,
    input: String,
    expected: Vec<String>,
    captured: bool,
    mask: bool,
    atlas: bool,
}
impl Profile {
    fn new(
        snr: &HigurashiScenario,
        has: bool,
        scenario: i32,
        active: usize,
        alt: u32,
        selected: i32,
        pattern: usize,
        capture: bool,
        missing: bool,
    ) -> Self {
        let model = Model::new(
            snr,
            has.then(|| super::super::tests::source(scenario, active)),
            selected,
            alt,
            &super::super::tests::vectors(pattern),
        )
        .unwrap();
        let mut motion = Motion::new(model, capture, snr);
        if missing && motion.model.alternate {
            motion.bgm_id = None;
        }
        let mut p = Self {
            motion,
            host: Host {
                buttons: Buttons::default(),
                captured: false,
                events: vec![],
                seeds: 0,
                dates: 0,
                retired: 0,
            },
            input: format!(
                "N {} {} {active} {alt} {selected} {pattern} {} {}\n",
                u32::from(has),
                scenario as u32,
                u32::from(capture),
                u32::from(missing)
            ),
            expected: vec![],
            captured: capture,
            mask: false,
            atlas: false,
        };
        p.snapshot();
        p
    }
    fn snapshot(&mut self) {
        let m = &self.motion;
        let model = &m.model;
        let (kind, flags) = match &m.request {
            None => (0, 0),
            Some(SceneRequest::Story { flags, .. }) => (1, *flags),
            Some(SceneRequest::Title { selected, .. }) => (2, *selected as u32),
            Some(SceneRequest::Origin { selected }) => (3, *selected as u32),
        };
        let mut cues = [0; 4];
        let mut plays = 0;
        let mut stop30 = 0;
        let mut stop60 = 0;
        let mut last = -1;
        for event in &self.host.events {
            match *event {
                Event::PlayBgm(id) => {
                    plays += 1;
                    last = id;
                }
                Event::StopBgm(30) => stop30 += 1,
                Event::StopBgm(60) => stop60 += 1,
                Event::StopBgm(x) => panic!("unexpected fade{x}"),
                Event::Cue(name) => {
                    cues[["move", "decide", "cancel", "error"]
                        .iter()
                        .position(|v| *v == name)
                        .unwrap()] += 1
                }
            }
        }
        let mut out = format!(
            "M {} {} {} {} {} {} {} {} {} {} {plays} {stop30} {stop60} {last} {} {} {} {} {} {kind} {flags}",
            m.first().map_or(-2, |f| f.phase),
            u32::from(model.closing),
            model.selected,
            model.scroll.value().to_bits(),
            model.feedback.value().to_bits(),
            model.background_phase.to_bits(),
            m.scrollbar.position,
            m.scrollbar.visible,
            m.scrollbar.first,
            m.scrollbar.end,
            cues[0],
            cues[1],
            cues[2],
            cues[3],
            self.host.seeds
        );
        if let Some(f) = m.first() {
            let c = f.clock.as_ref().unwrap();
            write!(
                out,
                " {} {} {} {} {} {} {}",
                u32::from(c.active),
                c.elapsed.to_bits(),
                c.duration.to_bits(),
                u32::from(c.source_present),
                u32::from(c.mask_present),
                c.softness.to_bits(),
                c.flags
            )
            .unwrap();
        } else {
            out.push_str(" 0 0 0 0 0 0 0");
        }
        write!(out, " {}", m.tasks().count()).unwrap();
        for t in m.tasks() {
            let (k, arg) = match t.kind {
                Kind::First => (0, t.phase),
                Kind::Input => (1, 0),
                Kind::Callback => match t.callback.unwrap() {
                    super::super::Pending::Start(at) => (2, at as i32),
                    super::super::Pending::Retreat => (3, 0),
                },
            };
            write!(out, " {k} {} {arg}", u32::from(t.done)).unwrap();
        }
        write!(out, " {}", m.borrowed.len()).unwrap();
        for (i, k) in m.borrowed.iter().enumerate() {
            let task = m.tasks().nth(i).unwrap();
            let k = match k {
                Kind::First => 0,
                Kind::Input => 1,
                Kind::Callback => {
                    if matches!(task.callback, Some(super::super::Pending::Start(_))) {
                        2
                    } else {
                        3
                    }
                }
            };
            write!(out, " {k}").unwrap();
        }
        let has_first = m.first().is_some();
        write!(
            out,
            " {} {} {} {} {} {}",
            u32::from(has_first),
            u32::from(has_first && self.captured),
            if has_first && self.mask { 2 } else { 0 },
            u32::from(self.atlas),
            self.host.retired,
            model.rows.len()
        )
        .unwrap();
        for row in &model.rows {
            write!(out, " {}", row.flash.value().to_bits()).unwrap();
        }
        if let Some(SceneRequest::Story { data: d, flags }) = &m.request {
            data(&mut out, d, model.data.is_none() && *flags == 0);
        }
        self.expected.push(out);
    }
    fn tick(
        &mut self,
        status: u32,
        mask: bool,
        atlas: bool,
        delta: f32,
        pressed: u32,
        repeat: u32,
        captured: bool,
    ) {
        writeln!(
            self.input,
            "U {status} {} {} {delta} {pressed:x} {repeat:x} {}",
            u32::from(mask),
            u32::from(atlas),
            u32::from(captured)
        )
        .unwrap();
        if self.motion.first().is_some_and(|f| f.phase == 0) && status == 2 {
            self.mask = mask;
        }
        self.atlas |= atlas;
        self.host.buttons = Buttons {
            pressed,
            repeat,
            ..Buttons::default()
        };
        self.host.captured = captured;
        self.motion
            .advance(
                status,
                mask,
                self.atlas,
                Ticks::from_f32(delta),
                &mut self.host,
            )
            .unwrap();
        self.snapshot();
    }
    fn finish(mut self, input: &mut String, expected: &mut Vec<String>) {
        self.input.push_str("D\n");
        self.expected.push("D".into());
        input.push_str(&self.input);
        expected.extend(self.expected);
    }
}
#[test]
#[ignore = "actual TIPS First/d11c8 scope/Input/Start/Retreat/full callback payloads/pre-clock scrollbar and resource retirement"]
fn higurashi_tips_first_scope_and_frame_order_match_native() {
    let snr = HigurashiScenario::new(
        std::fs::read(std::env::var("SHIN_HIGURASHI_SNR").unwrap())
            .unwrap()
            .into(),
    )
    .unwrap();
    let mut input = String::new();
    let mut expected = vec![];
    let mut owners = 0;
    for (has, scenario, active, alt) in [
        (false, 100, 0, 0),
        (false, 100, 0, 1),
        (true, 9999, 0, 1),
        (true, 11000, 1, 0),
    ] {
        for capture in [false, true] {
            for mask in [false, true] {
                for missing in [false, true] {
                    for pattern in [0, 1, 3, 4] {
                        let mut p = Profile::new(
                            &snr, has, scenario, active, alt, 244, pattern, capture, missing,
                        );
                        p.tick(0, false, false, 0.0, 1 << 24, 0x40, false);
                        p.tick(1, false, true, 3.0, 1 << 25, 0x20, false);
                        p.tick(3, false, true, 0.5, 1 << 24, 0x40, true);
                        p.tick(2, mask, true, 3.0, 1 << 24, 0x40, false);
                        for _ in 0..9 {
                            p.tick(2, mask, true, 3.0, 1 << 24, 0x40, false);
                        }
                        p.tick(2, mask, true, 0.0, 1 << 24, 0x40, false); // queues Input;cannot poll it in this snapshot
                        p.tick(2, mask, true, 0.0, 0, 0, true); // prune First before first Input
                        for repeat in [0x400, 0x800, 0x10, 0x40, 0x20, 0x80] {
                            p.tick(2, mask, true, 1.0, 0, repeat, false);
                            p.tick(2, mask, true, 3.0, 0, repeat, false);
                            p.tick(2, mask, true, 1.0, 0, 0, false);
                        }
                        p.tick(
                            2,
                            mask,
                            true,
                            0.0,
                            if pattern == 0 { 1 << 25 } else { 1 << 24 },
                            0,
                            false,
                        );
                        for _ in 0..8 {
                            p.tick(2, mask, true, 1.0, 1 << 24, 0x40, false);
                        }
                        if p.motion.request.is_none() {
                            p.tick(2, mask, true, 0.0, 1 << 25, 0, false);
                            for _ in 0..8 {
                                p.tick(2, mask, true, 1.0, 0, 0, false);
                            }
                        }
                        assert!(p.motion.request.is_some());
                        assert_eq!(p.host.retired, 1);
                        p.finish(&mut input, &mut expected);
                        owners += 1;
                    }
                }
            }
        }
    }
    // Retire the actual First while its worker is still pending or while its
    // fade is active;native ownership checks must finish with no held producer.
    for capture in [false, true] {
        for phase in 0..3 {
            let mut p = Profile::new(&snr, true, 100, 0, 0, 0, 3, capture, false);
            if phase > 0 {
                p.tick(
                    2,
                    true,
                    true,
                    if phase == 1 { 0.0 } else { 3.0 },
                    0,
                    0,
                    false,
                );
            }
            p.finish(&mut input, &mut expected);
            owners += 1;
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_TIPS_MOTION").unwrap())
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
    if let Ok(dir) = std::env::var("SHIN_HIGURASHI_TIPS_MOTION_CAPTURE") {
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
        let a = a.split_whitespace().collect::<Vec<_>>();
        let e = e.split_whitespace().collect::<Vec<_>>();
        assert_eq!(a.len(), e.len(), "TIPS motion row{i} token count");
        for (j, (a, e)) in a.iter().zip(&e).enumerate() {
            if *e != "*" {
                assert_eq!(a, e, "TIPS motion row{i} token{j}");
            }
        }
    }
    eprintln!(
        "TIPS motion:{owners} original owners/{} exact scope/frame/payload observations;First status gates/30-tick mask/music77-or-org/hou/Tip/input-next-snapshot/retirement,pre-clock integer scrollbar,callback flash gates/full requests and pending/fading producer disposal pass",
        rows.len()
    );
}
