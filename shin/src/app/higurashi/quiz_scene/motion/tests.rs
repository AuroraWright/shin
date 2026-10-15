use super::*;
use crate::adv::higurashi::input::Buttons;
use crate::app::higurashi::quiz_scene::tests::{append_data, append_string, run_native};
use std::fmt::Write as _;

#[derive(Default)]
struct Reference {
    events: Vec<Action>,
    frames: Vec<[u32; 3]>,
    pressed: u32,
    captured: bool,
    voice: u32,
    bgm: u32,
    published: bool,
    retired: bool,
    atlas: bool,
}
impl Services for Reference {
    fn event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Model(action) => self.events.push(action),
            Event::StopBgm(ticks) => self.frames.push([3, ticks, 0]),
            Event::PlayBgm { id, fade } => self.frames.push([4, id as u32, fade]),
            Event::ReturnScore(score) => self.frames.push([8, score as u32, 0]),
        }
        Ok(())
    }
    fn publish_resources(&mut self) -> Result<()> {
        assert!(!self.published);
        self.published = true;
        for i in 0..11 {
            self.frames.push([5, i, 0]);
        }
        Ok(())
    }
    fn retire_first(&mut self) {
        assert!(!self.retired);
        self.retired = true;
    }
    fn advance_voice(&mut self, delta: Ticks) {
        self.frames.push([6, delta.as_f32().to_bits(), 0]);
    }
    fn advance_characters(&mut self, delta: Ticks) {
        for i in 0..11 {
            self.frames.push([7, i, delta.as_f32().to_bits()]);
        }
    }
    fn input(&mut self, model: &mut Model, permission: bool) -> Result<()> {
        let mut permission = permission;
        model.input(
            &mut permission,
            self.captured,
            Buttons {
                pressed: self.pressed,
                repeat: u32::MAX,
                ..Default::default()
            },
        )?;
        assert!(!permission);
        Ok(())
    }
    fn voice_status(&self) -> u32 {
        self.voice
    }
    fn bgm_status(&self) -> u32 {
        self.bgm
    }
}
#[derive(Default)]
struct Comparison {
    motion: Option<Motion>,
    services: Reference,
    capture: bool,
    input: String,
    expected: Vec<String>,
    owners: usize,
}
impl Comparison {
    fn snapshot(&mut self) {
        let motion = self.motion.as_ref().unwrap();
        let m = &motion.model;
        let mut row = format!(
            "M {} {} {} {} {} {} {} {} {} {} {} {} {}",
            u8::from(motion.ready),
            u8::from(m.closing),
            motion.countdown_phase,
            motion.countdown_elapsed.to_bits(),
            motion.question_elapsed.to_bits(),
            motion.next_question,
            m.rng.state(),
            m.score as u32,
            m.clamped_argument,
            m.current_question,
            m.correct_answer,
            m.chosen_answer,
            m.reaction_variant
        );
        for i in m.contact {
            write!(row, " {i}").unwrap();
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
        if let Some(first) = motion.first() {
            let o = first.overlay.as_ref().unwrap();
            write!(
                row,
                " {} {} {} {} {}",
                first.phase,
                u8::from(o.active),
                o.elapsed.to_bits(),
                o.duration.to_bits(),
                u8::from(o.source_present)
            )
            .unwrap();
        } else {
            row.push_str(" -2 0 0 0 0");
        }
        write!(row, " {}", motion.tasks().count()).unwrap();
        for task in motion.tasks() {
            write!(row, " {} {}", task.kind as u32, u8::from(task.done)).unwrap();
        }
        write!(row, " {}", motion.borrowed.len()).unwrap();
        for kind in &motion.borrowed {
            write!(row, " {}", *kind as u32).unwrap();
        }
        let live = !self.services.retired;
        write!(
            row,
            " R {} {} {} {} {} {}",
            u8::from(live),
            u8::from(live && self.capture),
            u8::from(live && self.services.published),
            u8::from(self.services.atlas),
            u8::from(self.services.retired),
            u8::from(motion.score_request.is_some())
        )
        .unwrap();
        write!(row, " E {}", self.services.events.len()).unwrap();
        for action in self.services.events.drain(..) {
            match action {
                Action::Cue(name) => {
                    row.push_str(" 0 0");
                    append_string(&mut row, name);
                }
                Action::Voice { name, reaction } => {
                    write!(row, " 1 {}", u8::from(reaction)).unwrap();
                    append_string(&mut row, name);
                }
                Action::DisableInput => row.push_str(" 2 0"),
            }
        }
        write!(row, " F {}", self.services.frames.len()).unwrap();
        for event in self.services.frames.drain(..) {
            for word in event {
                write!(row, " {word}").unwrap();
            }
        }
        if let Some(score) = motion.score_request {
            write!(row, " Q {}", score as u32).unwrap();
            append_data(&mut row, &m.data);
        }
        self.expected.push(row);
    }
    fn begin(&mut self, args: [i32; 3], seed: u32, capture: bool) {
        let active = self.owners % 2;
        writeln!(
            self.input,
            "N {} {} {} {active} {seed} {}",
            args[0],
            args[1],
            args[2] as u32,
            u8::from(capture)
        )
        .unwrap();
        self.services = Reference::default();
        self.capture = capture;
        let model = Model::new(
            crate::app::higurashi::tips_menu::tests::source(100, active),
            args,
            seed,
        )
        .unwrap();
        self.motion = Some(Motion::new(model, capture));
        self.owners += 1;
        self.snapshot();
    }
    fn frame(
        &mut self,
        status: u32,
        atlas: bool,
        delta: f32,
        pressed: u32,
        captured: bool,
        voice: u32,
        bgm: u32,
    ) {
        writeln!(
            self.input,
            "U {status} {} {:x} {pressed:x} {} {voice} {bgm}",
            u8::from(atlas),
            delta.to_bits(),
            u8::from(captured)
        )
        .unwrap();
        self.services.pressed = pressed;
        self.services.captured = captured;
        self.services.voice = voice;
        self.services.bgm = bgm;
        self.services.atlas |= atlas;
        self.motion
            .as_mut()
            .unwrap()
            .advance(status, Ticks::from_f32(delta), &mut self.services)
            .unwrap();
        self.snapshot();
    }
    fn dispose(&mut self) {
        self.motion.take();
        self.input.push_str("D\n");
        self.expected.push("D".into());
    }
    fn open(&mut self) {
        for status in [0, 1, 3] {
            self.frame(status, true, 10.0, 0xf000, false, 2, 2);
        }
        self.frame(2, true, 29.5, 0xf000, false, 2, 2);
        self.frame(2, true, 0.5, 0xf000, false, 2, 2);
        self.frame(2, true, 0.0, 0xf000, false, 2, 2);
        for _ in 0..4 {
            self.frame(2, true, 59.5, 0xf000, false, 2, 2);
            self.frame(2, true, 0.5, 0xf000, false, 2, 2);
        }
        self.frame(2, true, 0.0, 0xf000, false, 2, 2);
        assert_eq!(self.motion.as_ref().unwrap().model.current_question, 0);
    }
}
#[test]
#[ignore = "original9ed04/9f7ac/d11c8/countdown9cb1c/question cadence/score return17490/factories/destruction,resource job/capture/BUP/audio/input checked;no GPU/audio/window/saves"]
fn higurashi_quiz_scope_countdown_cadence_finish_and_score_return_match_native() {
    let mut c = Comparison::default();
    for pool in 0..8 {
        for context in 0..9 {
            for capture in [false, true] {
                c.begin(
                    [pool, context, [0, 9, 10, -1][c.owners % 4]],
                    [1, 17, u32::MAX][c.owners % 3],
                    capture,
                );
                let data = c.motion.as_ref().unwrap().model.data.clone();
                c.open();
                for index in 0..10 {
                    c.frame(2, true, 9.5, 0xf000, false, 0, 0);
                    c.frame(2, true, 0.5, 0xf000, false, 0, 0);
                    let correct = c.motion.as_ref().unwrap().model.correct_answer;
                    let answer = if (context + index) % 2 == 0 {
                        correct
                    } else {
                        (correct + 1) % 4
                    };
                    c.frame(2, true, 0.0, 1 << (12 + answer), true, 0, 0);
                    c.frame(2, true, 0.0, 1 << (12 + answer), false, 0, 0);
                    c.frame(2, true, 0.0, 0xf000, false, 0, 0);
                    c.frame(2, true, 349.5, 0, false, 0, 0);
                    c.frame(2, true, 0.5, 0, false, 0, 0);
                    c.frame(2, true, 0.0, 0xf000, false, 0, 0);
                }
                assert!(c.motion.as_ref().unwrap().model.closing);
                assert_eq!(c.motion.as_ref().unwrap().model.score, 5);
                assert_eq!(c.motion.as_ref().unwrap().model.data, data);
                for (voice, bgm) in [(2, 0), (0, 2), (2, 2), (3, 3)] {
                    c.frame(2, true, 3.0, 0xf000, false, voice, bgm);
                }
                c.frame(2, true, 29.5, 0xf000, false, 1, 1);
                c.frame(2, true, 0.5, 0xf000, false, 1, 1);
                c.frame(2, true, 59.5, 0xf000, false, 1, 1);
                c.frame(2, true, 0.5, 0xf000, false, 1, 1);
                assert!(c.motion.as_ref().unwrap().score_request.is_none());
                c.frame(2, true, 0.0, 0xf000, false, 1, 1);
                assert_eq!(c.motion.as_ref().unwrap().score_request, Some(5));
                c.dispose();
            }
        }
    }
    // Large deltas do not catch up all countdown/question steps in one frame.
    for delta in [60.0, 240.0, 360.0, 3600.0, 10000.0] {
        for capture in [false, true] {
            c.begin([0, 0, 9], 17, capture);
            c.frame(2, true, 30.0, 0, false, 0, 0);
            c.frame(2, true, 0.0, 0, false, 0, 0);
            c.frame(2, true, delta, 0, false, 0, 0);
            c.frame(2, true, 0.0, 0, false, 0, 0);
            for _ in 0..14 {
                if c.motion.as_ref().unwrap().score_request.is_some() {
                    break;
                }
                c.frame(2, true, delta, 0xf000, false, 0, 0);
            }
            c.dispose();
        }
    }
    // Cancellation while loading/fading/counting down and awaiting voices.
    for steps in 0..18 {
        c.begin([steps % 8, steps % 9, steps - 2], 17, steps % 2 != 0);
        for frame in 0..steps {
            c.frame(
                if frame < 3 { 1 } else { 2 },
                frame > 0,
                60.0,
                0,
                false,
                2,
                2,
            );
        }
        c.dispose();
    }
    let owners = c.owners;
    let observations = c.expected.len();
    run_native(
        c.input,
        c.expected,
        "motion",
        "SHIN_HIGURASHI_NATIVE_QUIZ_SCENE_MOTION",
    );
    eprintln!(
        "Quiz motion:{owners} native owners/{observations} exact observations;original First/resource gate/fade/countdown/scope snapshots/ten-question cadence/answers/voice-music finish gates/90-tick fade/full ADV score return and balanced early/final retirement pass"
    );
}
