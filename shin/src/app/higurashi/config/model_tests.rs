use super::*;
use crate::adv::higurashi::mode_stack::{Mode as OwnedMode, ModePush, ModeStack};
use std::collections::VecDeque;

fn settings(bytes: &[u32]) -> Settings {
    let mut s = Settings::default();
    let c = &mut s.common;
    c.v0_bgmvol = bytes[0] as u8;
    c.v1_sfxvol = bytes[1] as u8;
    c.v2_voicevol = bytes[2] as u8;
    c.v3_sysvol = bytes[3] as u8;
    c.v4_voicefocus = bytes[4] != 0;
    c.v5_voicepanapot = bytes[5] != 0;
    c.v6 = bytes[6] != 0;
    c.v7 = bytes[7] as u8;
    c.v8 = bytes[8] as u8;
    c.v9_msgspeed = bytes[9] as u8;
    c.v10_skipspeed = bytes[10] as u8;
    c.v11_disallowskipunread = bytes[11] != 0;
    c.v12 = bytes[12] != 0;
    c.v13_msgwinalpha = bytes[13] as u8;
    c.v14_showroutenavi = bytes[14] != 0;
    c.v15 = bytes[15] != 0;
    c.v16_showtoucheffect = bytes[16] != 0;
    c.v17_showscenetitle = bytes[17] != 0;
    c.v18_showsongtitle = bytes[18] != 0;
    c.v19 = u32::from_le_bytes(
        bytes[20..24]
            .iter()
            .map(|v| *v as u8)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
    );
    s.extra_flag = bytes[24] != 0;
    s.extra_percentage = bytes[25] as u8;
    s
}
fn settings_words(s: &Settings) -> Vec<u32> {
    let c = &s.common;
    let mut words = vec![
        c.v0_bgmvol.into(),
        c.v1_sfxvol.into(),
        c.v2_voicevol.into(),
        c.v3_sysvol.into(),
        c.v4_voicefocus.into(),
        c.v5_voicepanapot.into(),
        c.v6.into(),
        c.v7.into(),
        c.v8.into(),
        c.v9_msgspeed.into(),
        c.v10_skipspeed.into(),
        c.v11_disallowskipunread.into(),
        c.v12.into(),
        c.v13_msgwinalpha.into(),
        c.v14_showroutenavi.into(),
        c.v15.into(),
        c.v16_showtoucheffect.into(),
        c.v17_showscenetitle.into(),
        c.v18_showsongtitle.into(),
        0,
    ];
    words.extend(c.v19.to_le_bytes().map(u32::from));
    words.extend([u32::from(s.extra_flag), u32::from(s.extra_percentage)]);
    words
}
struct Native {
    op: char,
    result: i32,
    header: Vec<u32>,
    settings: Vec<u32>,
    rows: Vec<Vec<u32>>,
    headings: Vec<u32>,
    dividers: Vec<u32>,
    cues: Vec<u32>,
    returns: Vec<u32>,
    children: Vec<u32>,
    prompt: String,
    labels: String,
}
fn native(line: &str) -> Option<Native> {
    if line == "X" {
        return None;
    }
    let mut s = line.split_whitespace();
    assert_eq!(s.next(), Some("M"));
    let op = s.next().unwrap().chars().next().unwrap();
    let result = s.next().unwrap().parse().unwrap();
    let mut words = s
        .map(|v| v.parse::<u32>().unwrap())
        .collect::<VecDeque<_>>();
    let mut take = |n: usize| words.drain(..n).collect::<Vec<_>>();
    let header = take(17);
    let settings = take(26);
    let count = take(1)[0];
    let mut rows = vec![];
    for _ in 0..count {
        let mut row = take(9);
        if row[0] == 0 {
            row.extend(take(row[8] as usize));
        }
        rows.push(row);
    }
    let count = take(1)[0];
    let headings = take(count as usize * 3);
    let count = take(1)[0];
    let dividers = take(count as usize);
    let cues = take(4);
    let count = take(1)[0];
    let returns = take(count as usize);
    let children = take(8);
    let count = take(1)[0];
    let prompt =
        String::from_utf8(take(count as usize).into_iter().map(|v| v as u8).collect()).unwrap();
    let count = take(1)[0];
    let labels =
        String::from_utf8(take(count as usize).into_iter().map(|v| v as u8).collect()).unwrap();
    drop(take);
    assert!(words.is_empty());
    Some(Native {
        op,
        result,
        header,
        settings,
        rows,
        headings,
        dividers,
        cues,
        returns,
        children,
        prompt,
        labels,
    })
}
enum Mode {
    Private(Task),
    Question(bool),
    Write(bool),
}
struct Callbacks {
    buttons: Buttons,
    captured: bool,
    events: Vec<Action>,
}
impl Services for Callbacks {
    fn input(&mut self, m: &mut Model, permission: bool) {
        m.poll_controls(permission, self.captured, self.buttons);
    }
    fn event(&mut self, event: Action) -> Result<()> {
        self.events.push(event);
        Ok(())
    }
}
struct Poll<'a> {
    model: &'a mut Model,
    status: u32,
    callbacks: &'a mut Callbacks,
    writes: &'a mut u32,
    write_flag: &'a mut u32,
    prompt: &'a mut String,
    labels: &'a mut String,
}
impl OwnedMode<Poll<'_>> for Mode {
    fn completion(&mut self) -> u32 {
        u32::from(match self {
            Self::Private(t) => t.done,
            Self::Question(done) | Self::Write(done) => *done,
        })
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        p: &mut Poll<'_>,
        push: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        if let Self::Private(task) = self {
            let first = p.callbacks.events.len();
            for task in task.poll_native(p.model, p.status, delta, permission, p.callbacks)? {
                push.push(Self::Private(task));
            }
            for event in &p.callbacks.events[first..] {
                match event {
                    Action::ResetQuestion => {
                        *p.prompt = "設定を初期値に戻します。よろしいですか？".into();
                        *p.labels = "はい|いいえ".into();
                        push.push(Self::Question(false));
                    }
                    Action::Save { flag } => {
                        *p.writes += 1;
                        *p.write_flag = *flag;
                        push.push(Self::Write(false));
                    }
                    _ => {}
                }
            }
        }
        Ok(0)
    }
}
struct Fixture {
    model: Model,
    scope: ModeStack<Mode>,
    ready: bool,
    status: u32,
    scrollbar: Scrollbar,
    callbacks: Callbacks,
    writes: u32,
    write_flag: u32,
    prompt: String,
    labels: String,
    observed: Vec<Action>,
    platform_value: u32,
}
impl Fixture {
    fn new(model: Model, status: u32, platform_value: u32) -> Self {
        let mut scope = ModeStack::default();
        scope.push(Mode::Private(Task {
            kind: Kind::First,
            done: false,
        }));
        Self {
            model,
            scope,
            status,
            ready: false,
            scrollbar: Scrollbar {
                position: 0,
                visible: 1,
                first: 0,
                end: 1,
            },
            callbacks: Callbacks {
                buttons: Buttons::default(),
                captured: false,
                events: vec![],
            },
            writes: 0,
            write_flag: 0,
            prompt: String::new(),
            labels: String::new(),
            observed: vec![],
            platform_value,
        }
    }
    fn collect(&mut self) {
        self.observed.extend(self.model.take_actions());
        self.observed.append(&mut self.callbacks.events);
        for a in &self.observed {
            if let Action::SetPlatformFlag(v) = a {
                self.platform_value = (*v).into();
            }
        }
    }
    fn frame(&mut self, permission: bool, delta: f32) {
        for task in self.model.take_pending_tasks() {
            self.scope.push(Mode::Private(task));
        }
        self.callbacks.buttons = Buttons::default();
        self.callbacks.events.clear();
        if self.scope.iter().any(|m| {
            matches!(
                m,
                Mode::Private(Task {
                    kind: Kind::First,
                    done: false
                })
            )
        }) && self.status == 2
        {
            self.ready = true;
        }
        self.scope
            .poll(
                Ticks::from_f32(delta),
                u32::from(permission),
                &mut Poll {
                    model: &mut self.model,
                    status: self.status,
                    callbacks: &mut self.callbacks,
                    writes: &mut self.writes,
                    write_flag: &mut self.write_flag,
                    prompt: &mut self.prompt,
                    labels: &mut self.labels,
                },
            )
            .unwrap();
        self.model
            .advance_clocks(Ticks::from_f32(delta), self.ready, &mut self.scrollbar);
        self.collect();
    }
    fn compare(&mut self, n: &Native, index: usize) {
        let m = &self.model;
        let v = m.view_state(self.ready, self.scrollbar);
        let header = vec![
            m.append.into(),
            m.flags,
            m.progress.to_bits(),
            m.selected as u32,
            v.content_height as u32,
            m.hit as u32,
            m.speed.to_bits(),
            m.chrome as u32,
            m.frame,
            m.closing.into(),
            m.complete.into(),
            m.scroll.value().to_bits(),
            u32::from(!m.scroll.is_idle()),
            m.close_flash.value().to_bits(),
            u32::from(!m.close_flash.is_idle()),
            m.reset_flash.value().to_bits(),
            u32::from(!m.reset_flash.is_idle()),
        ];
        assert_eq!(
            header, n.header,
            "native Config observation{index} {} controller/clock fields",
            n.op
        );
        let mut initialized_settings = n.settings.clone();
        // a9934 leaves byte13 unwritten; a9aac never serializes it. Reset
        // copies that stack padding into global+fb. Preserve raw captures,
        // but compare exactly the25 initialized settings lanes here.
        initialized_settings[19] = 0;
        assert_eq!(
            settings_words(&m.settings),
            initialized_settings,
            "native Config observation{index} initialized settings (only+fb padding excluded)"
        );
        let rows = m
            .rows
            .iter()
            .map(|r| match &r.state.kind {
                Control::Slider { subtype, value } => vec![
                    1,
                    r.state.y as u32,
                    r.state.title_band,
                    *subtype,
                    r.state.focused.into(),
                    *value as u32,
                    r.drag_value as u32,
                    r.drag_distance.to_bits(),
                    r.dragging.into(),
                ],
                Control::Buttons {
                    labels,
                    value,
                    cursor,
                } => {
                    let mut words = vec![
                        0,
                        r.state.y as u32,
                        r.state.title_band,
                        *value as u32,
                        r.state.focused.into(),
                        *cursor as u32,
                        r.pointed as u32,
                        *value as u32,
                        labels.len() as u32,
                    ];
                    words.extend(labels);
                    words
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rows, n.rows,
            "native Config observation{index} complete widget rows"
        );
        assert_eq!(
            v.headings
                .iter()
                .flat_map(|h| [h.band, h.y as u32, h.bottom as u32])
                .collect::<Vec<_>>(),
            n.headings,
            "native Config headings{index}"
        );
        assert_eq!(
            v.dividers.iter().map(|v| *v as u32).collect::<Vec<_>>(),
            n.dividers,
            "native Config dividers{index}"
        );
        let mut counts = [0; 4];
        let mut returns = vec![];
        let mut platform_count = 0;
        for action in self.observed.drain(..) {
            match action {
                Action::DisableInput => counts[0] += 1,
                Action::Cue("move") => counts[1] += 1,
                Action::Cue("decide") => counts[2] += 1,
                Action::Cue("cancel") => counts[3] += 1,
                Action::ReturnSource(stage) => returns.push(stage),
                Action::SetPlatformFlag(_) => platform_count += 1,
                _ => {}
            }
        }
        assert_eq!(
            counts.as_slice(),
            n.cues,
            "native Config observation{index} literal cues/denied capture"
        );
        assert_eq!(returns, n.returns, "native Config callbacks{index}");
        let q = self.scope.iter().any(|m| matches!(m, Mode::Question(_)));
        let w = self.scope.iter().any(|m| matches!(m, Mode::Write(_)));
        let child = vec![
            q.into(),
            u32::from(!self.prompt.is_empty()),
            u32::from(!self.prompt.is_empty()),
            w.into(),
            self.writes,
            self.write_flag,
            platform_count,
            self.platform_value,
        ];
        assert_eq!(
            child, n.children,
            "native Config observation{index} child/writer/service ownership"
        );
        assert_eq!(self.prompt, n.prompt);
        assert_eq!(self.labels, n.labels);
    }
}

#[test]
fn higurashi_config_matches_original_141_owners_4231_states() {
    let input =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../reverse-engineering/higurashi/config-model-capture/input.txt"));
    let output = include_str!(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../reverse-engineering/higurashi/config-model-capture/output.txt")
    );
    let mut fixture: Option<Fixture> = None;
    let mut first_status = 2;
    let mut platform_value = 0;
    let mut owners = 0;
    let mut observations = 0;
    for (index, (line, expected)) in input.lines().zip(output.lines()).enumerate() {
        let mut words = line.split_whitespace();
        let op = words.next().unwrap().chars().next().unwrap();
        if op == 'N' {
            let flags = words.next().unwrap().parse().unwrap();
            let append = words.next().unwrap() == "1";
            let raw = words.map(|v| v.parse().unwrap()).collect::<Vec<u32>>();
            fixture = Some(Fixture::new(
                Model::new(settings(&raw), flags, append).unwrap(),
                first_status,
                platform_value,
            ));
            owners += 1;
        } else if op == 'X' {
            assert_eq!(expected, "X");
            platform_value = fixture.take().unwrap().platform_value;
            continue;
        } else {
            let f = fixture.as_mut().unwrap();
            let m = &mut f.model;
            let mut result = -1;
            match op {
                'A' => {
                    first_status = words.next().unwrap().parse().unwrap();
                    f.status = first_status;
                }
                'F' => {
                    let permission = words.next().unwrap() == "1";
                    let delta = words.next().unwrap().parse().unwrap();
                    f.frame(permission, delta);
                    result = 0;
                }
                'U' => {
                    let permission = words.next().unwrap() == "1";
                    f.callbacks.captured = words.next().unwrap() == "1";
                    let pressed = u32::from_str_radix(words.next().unwrap(), 16).unwrap();
                    let repeat = u32::from_str_radix(words.next().unwrap(), 16).unwrap();
                    result = i32::from(m.poll_controls(
                        permission,
                        f.callbacks.captured,
                        Buttons {
                            pressed,
                            repeat,
                            ..Default::default()
                        },
                    ));
                }
                'S' => {
                    m.select(words.next().unwrap().parse().unwrap()).unwrap();
                }
                'V' | 'P' | 'R' => {
                    let x = words.next().unwrap().parse().unwrap();
                    let y = words.next().unwrap().parse().unwrap();
                    match op {
                        'V' => result = i32::from(m.contact(x, y)),
                        'P' => m.press_contact(Vec2::new(x, y)),
                        _ => m.release_contact(Vec2::new(x, y)),
                    }
                }
                'G' => {
                    let v = words.map(|v| v.parse().unwrap()).collect::<Vec<f32>>();
                    m.drag_contact(Vec2::new(v[0], v[1]), Vec2::new(v[2], v[3]));
                }
                'I' => {
                    words.next().unwrap();
                    m.inertia(words.next().unwrap().parse().unwrap());
                }
                'Z' => m.reset_contact(),
                'C' => m.cancel_contact(),
                'Q' => {
                    let stage = words.next().unwrap().parse().unwrap();
                    let answer = words.next().unwrap().parse().unwrap();
                    m.reset_answer(stage, answer);
                    if stage == 1 {
                        for mode in f.scope.iter_mut() {
                            if let Mode::Question(done) = mode {
                                *done = true;
                            }
                        }
                    }
                }
                'W' => {
                    m.save_complete();
                    for mode in f.scope.iter_mut() {
                        if let Mode::Write(done) = mode {
                            *done = true;
                        }
                    }
                }
                _ => panic!("Unknown native operation{op}"),
            }
            f.collect();
            assert_eq!(
                result,
                native(expected).unwrap().result,
                "native operation result{index}"
            );
        }
        let f = fixture.as_mut().unwrap();
        f.collect();
        f.compare(&native(expected).unwrap(), index);
        observations += 1;
    }
    assert!(fixture.is_none());
    assert_eq!((owners, observations), (141, 4231));
    assert_eq!(input.lines().count(), output.lines().count());
}

#[test]
fn higurashi_config_checked_constructor_rejects_unknown_flags_without_mutation() {
    assert!(Model::new(Settings::default(), 4, true).is_err());
    let mut m = Model::new(Settings::default(), 2, true).unwrap();
    let before = m.view_state(false, Scrollbar::default());
    assert!(m.select(12).is_err());
    let after = m.view_state(false, Scrollbar::default());
    assert_eq!(before.rows, after.rows);
    assert_eq!(before.scroll.to_bits(), after.scroll.to_bits());
    assert!(m.take_actions().is_empty());
}

#[test]
#[ignore = "reruns unchanged AArch64 original-function Config corpus, no hardware"]
fn higurashi_config_original_cpu_capture_is_reproducible() {
    let exe = std::env::var("SHIN_HIGURASHI_NATIVE_CONFIG_MODEL")
        .expect("native-config-model executable");
    let mut child = std::process::Command::new(exe)
        .args([
            "higurashi/uncompressed_executable_update",
            "/tmp/higurashi-newrodin.fnt",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    let input =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../reverse-engineering/higurashi/config-model-capture/input.txt"));
    let mut stdin = child.stdin.take().unwrap();
    let bytes = input.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        stdin.write_all(&bytes).unwrap();
    });
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        initialized_native_capture(&result.stdout),
        initialized_native_capture(include_bytes!(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../reverse-engineering/higurashi/config-model-capture/output.txt")
        ))
    );
}
fn initialized_native_capture(bytes: &[u8]) -> Vec<String> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| {
            if line == "X" {
                return line.to_owned();
            }
            let mut words = line.split_whitespace().collect::<Vec<_>>();
            assert_eq!(words[0], "M");
            //3 protocol words +17 header fields +settings padding lane19.
            words[39] = "0";
            words.join(" ")
        })
        .collect()
}
