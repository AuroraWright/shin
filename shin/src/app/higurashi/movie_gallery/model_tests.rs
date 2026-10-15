//! Mandatory original reexecution precedes every candidate/native comparison.
//! No original runner, assets or environment means failure, never skipped success.
use super::*;
use serde_json::{Value, json};
use shin_core::format::scenario::higurashi::HigurashiScenario;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}
fn evidence() -> PathBuf {
    root().join("reverse-engineering/higurashi/movie-gallery-model-native-corpus")
}
fn required(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("Set {name}; native comparison cannot be skipped"))
}
fn rerun_original() -> String {
    let input = std::fs::read(evidence().join("input.txt")).unwrap();
    let golden = std::fs::read(evidence().join("output.txt")).unwrap();
    let stderr = std::fs::read(evidence().join("stderr.txt")).unwrap();
    assert_eq!(
        golden,
        std::fs::read(evidence().join("output-repeat.txt")).unwrap()
    );
    assert_eq!(
        stderr,
        std::fs::read(evidence().join("stderr-repeat.txt")).unwrap()
    );
    for run in 0..2 {
        let mut child = Command::new(required("SHIN_HIGURASHI_MOVIE_GALLERY_MODEL_NATIVE"))
            .arg(root().join("higurashi/uncompressed_executable_update"))
            .arg(required("SHIN_HIGURASHI_MOVIE_GALLERY_FONT"))
            .arg(required("SHIN_HIGURASHI_MOVIE_GALLERY_SCENARIO"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let bytes = input.clone();
        let writer = std::thread::spawn(move || stdin.write_all(&bytes).unwrap());
        let actual = child.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            actual.status.success(),
            "Original Movie run{run} failed: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert!(
            actual.stdout == golden,
            "Original Movie stdout drift on run{run}"
        );
        assert!(
            actual.stderr == stderr,
            "Original Movie stderr/service/balance drift on run{run}"
        );
    }
    String::from_utf8(golden).unwrap()
}
fn word(v: &Value) -> u32 {
    v.as_u64().unwrap().try_into().unwrap()
}
fn words(v: &Value) -> Vec<u32> {
    v.as_array().unwrap().iter().map(word).collect()
}
fn clock(c: &HigurashiTweener) -> Value {
    json!([c.value().to_bits(), u32::from(!c.is_idle())])
}
fn main_scope(scope: &ModeStack<Task>) -> Value {
    json!(
        scope
            .iter()
            .map(|task| match task {
                Task::First { phase, .. } => [0x1b64b0, u32::from(task.complete()), *phase as u32],
                Task::Input { done } => [0x1b6648, u32::from(*done), 0],
                Task::Viewer(_) => [0x1b64e8, u32::from(task.complete()), 0],
                Task::Close { done } => [0x1b66f8, u32::from(*done), 0],
            })
            .collect::<Vec<_>>()
    )
}
fn viewer_scope(scope: &ModeStack<ViewerTask>) -> Value {
    json!(
        scope
            .iter()
            .map(|t| [
                match t {
                    ViewerTask::Ready { .. } => 0x1b67f8,
                    ViewerTask::Input { .. } => 0x1b6868,
                    ViewerTask::Exit { .. } => 0x1b6928,
                    ViewerTask::Backdrop { .. } => 0x1b6998,
                },
                u32::from(t.complete()),
                0
            ])
            .collect::<Vec<_>>()
    )
}
fn viewer_mut(scope: &mut ModeStack<Task>) -> Option<&mut Viewer> {
    scope.iter_mut().find_map(|task| {
        if let Task::Viewer(v) = task {
            Some(v)
        } else {
            None
        }
    })
}
fn event_projection(tag: &str, raw: &[u32]) -> Option<(String, Vec<u32>)> {
    let n = match tag {
        "achievement" | "cue" | "bgm-stop" | "player-start" | "player-pause" | "player-stop"
        | "player-gain" => 1,
        "title-queue" | "player-update" => 2,
        "player-new" | "player-drop" => 3,
        "job-join" | "atlas-drop" => 0,
        _ => return None,
    };
    Some((tag.into(), raw[..n].to_vec()))
}
#[derive(Default)]
struct Transport {
    first_status: u32,
    atlas: bool,
    mask: bool,
    captured: bool,
    buttons: Buttons,
    player_present: bool,
    movie_id: u16,
    status: u32,
    desired: bool,
    frame_present: bool,
    gain: u32,
    starts: u32,
    pauses: u32,
    updates: u32,
    stops: u32,
    created: u32,
    dropped: u32,
    platform: u32,
    achievements: u32,
    queue: u32,
    stop30: u32,
    stop60: u32,
    disabled: u32,
    cues: [u32; 4],
    events: Vec<(String, Vec<u32>)>,
    fail: Option<&'static str>,
}
impl Transport {
    fn record(&mut self, tag: &'static str, raw: &[u32]) {
        self.events.push(event_projection(tag, raw).unwrap());
    }
}
impl Services for Transport {
    fn first_status(&mut self) -> Result<u32> {
        Ok(self.first_status)
    }
    fn mask_published(&self) -> bool {
        self.mask
    }
    fn input_grid(&mut self, model: &mut Model, permission: bool) -> Result<bool> {
        model.controller(permission, self.captured, self.buttons, self)
    }
    fn input_viewer(&mut self, viewer: &mut Viewer, permission: bool) -> Result<bool> {
        viewer.controller(permission, self.captured, self.buttons.pressed, self)
    }
    fn player_status(&mut self, _: u64) -> Result<u32> {
        assert!(self.player_present);
        Ok(self.status)
    }
    fn action(&mut self, action: Action) -> Result<()> {
        if self.fail == Some("stop60") && action == Action::StopBgm(60) {
            anyhow::bail!("controlled audio failure");
        }
        match action {
            Action::Achievement(id) => {
                self.achievements += 1;
                self.record("achievement", &[id]);
            }
            Action::Cue(name) => {
                let i = match name {
                    "move" => 0,
                    "decide" => 1,
                    "cancel" => 2,
                    "error" => 3,
                    _ => panic!("cue"),
                };
                self.cues[i] += 1;
                self.record("cue", &[i as u32]);
            }
            Action::DisableInput(_) => self.disabled += 1,
            Action::StopBgm(fade) => {
                match fade {
                    30 => self.stop30 += 1,
                    60 => self.stop60 += 1,
                    _ => panic!("fade"),
                };
                self.record("bgm-stop", &[fade]);
            }
            Action::ReturnTitle { selected, mode } => {
                self.queue += 1;
                self.record("title-queue", &[selected as u32, mode as u32]);
            }
            Action::OpenPlayer {
                movie_id,
                source_category,
                ..
            } => {
                assert!(!self.player_present);
                self.player_present = true;
                self.movie_id = movie_id;
                self.desired = false;
                self.starts = 0;
                self.pauses = 0;
                self.updates = 0;
                self.stops = 0;
                self.gain = 0;
                self.created += 1;
                self.record(
                    "player-new",
                    &[u32::from(movie_id), source_category, self.status],
                );
            }
            Action::StartPlayer(_) => {
                self.desired = true;
                self.starts += 1;
                self.record("player-start", &[self.status]);
            }
            Action::PausePlayer(_) => {
                self.desired = false;
                self.pauses += 1;
                self.record("player-pause", &[self.status]);
            }
            Action::UpdatePlayer(_) => {
                self.updates += 1;
                self.record("player-update", &[self.status, u32::from(self.desired)]);
            }
            Action::StopPlayer(_) => {
                self.stops += 1;
                self.desired = false;
                self.record("player-stop", &[self.status]);
            }
            Action::ReleasePlayer(_) => {
                self.player_present = false;
                self.dropped += 1;
                self.record(
                    "player-drop",
                    &[
                        u32::from(self.movie_id),
                        self.stops,
                        u32::from(self.desired),
                    ],
                );
            }
            Action::PlayerGain { value, .. } => {
                self.gain = value.to_bits();
                self.record("player-gain", &[self.gain]);
            }
            Action::PlayerColor { value, .. } => {
                assert_eq!(&value[..3], &[1., 1., 1.]);
            }
            Action::PlatformByte => self.platform += 1,
            Action::JoinFirst => self.record("job-join", &[]),
            Action::ReleaseAtlas => {
                if self.atlas {
                    self.record("atlas-drop", &[]);
                }
                self.atlas = false;
            }
        }
        Ok(())
    }
}
struct Fixture {
    model: Model,
    scope: ModeStack<Task>,
    service: Transport,
    volume: u8,
}
impl Fixture {
    fn new(
        catalog: &[MovieInfoItem],
        unlock: u32,
        append: bool,
        capture: bool,
        volume: u8,
    ) -> Self {
        let mut model = Model::new(catalog, &[unlock], append).unwrap();
        let mut service = Transport {
            frame_present: true,
            ..Default::default()
        };
        model.publish_initial(&mut service).unwrap();
        let mut scope = ModeStack::default();
        scope.push(Task::first(capture));
        Self {
            model,
            scope,
            service,
            volume,
        }
    }
    fn absorb(&mut self) {
        for task in self.model.take_pending_tasks() {
            self.scope.push(task);
        }
    }
    fn check(&mut self, n: &Value, result: i32, context: &str) {
        assert_eq!(
            n["result"].as_i64(),
            Some(i64::from(result)),
            "{context} result"
        );
        let m = &self.model;
        for (key, value) in [
            ("append", json!(u32::from(m.append))),
            ("closing", json!(u32::from(m.closing))),
            ("frame", json!(m.frame)),
            ("selection", json!([m.x as u32, m.y as u32])),
            ("height", json!((m.document_height as f32).to_bits())),
            (
                "contact",
                json!([
                    m.region as u32,
                    m.hover as u32,
                    m.contact as u32,
                    m.gesture.to_bits()
                ]),
            ),
            ("scroll", clock(&m.scroll)),
            ("close_flash", clock(&m.close_flash)),
            ("completion", json!(m.completion)),
            ("scope", main_scope(&self.scope)),
        ] {
            assert_eq!(n[key], value, "{context} {key}");
        }
        let rows = json!(
            m.rows
                .iter()
                .enumerate()
                .map(|(i, r)| vec![
                    r.catalog_index as u32,
                    i as u32,
                    u32::from(r.movie_id),
                    u32::from(r.append),
                    u32::from(r.locked),
                    r.x.to_bits(),
                    r.y.to_bits(),
                    r.focus.to_bits()
                ])
                .collect::<Vec<_>>()
        );
        assert_eq!(n["rows"], rows, "{context} all32-byte rows");
        let s = &self.service;
        assert_eq!(
            word(&n["atlas"]),
            u32::from(s.atlas),
            "{context} publication transport"
        );
        assert_eq!(
            words(&n["audio_stops"]),
            [s.stop30, s.stop60],
            "{context} native audio args"
        );
        assert_eq!(
            word(&n["disabled"]),
            s.disabled,
            "{context} InputArea disable"
        );
        assert_eq!(words(&n["cues"]), s.cues, "{context} literal cues");
        let queue = if s.queue == 0 {
            [0, 0, 0]
        } else {
            [s.queue, 17, u32::MAX]
        };
        assert_eq!(words(&n["queue"]), queue, "{context} Title17/-1");
        let counts = words(&n["counts"]);
        assert_eq!(counts[0], 0, "{context} original OS endpoint unused");
        assert_eq!(counts[1], s.achievements, "{context} achievement");
        assert_eq!(counts[2], 1, "{context} original text set once");
        assert_eq!(
            &counts[5..8],
            &[s.created, s.dropped, s.platform],
            "{context} Player/shared-byte services"
        );
        let player = if s.player_present {
            json!([
                u32::from(s.movie_id),
                s.status,
                u32::from(s.desired),
                1,
                s.gain,
                s.starts,
                s.pauses,
                s.updates,
                s.stops,
                u32::from(s.frame_present)
            ])
        } else {
            Value::Null
        };
        assert_eq!(
            n["player_service"], player,
            "{context} explicit checked Player transport"
        );
        let v = self.scope.iter().find_map(|t| {
            if let Task::Viewer(v) = t {
                Some(v)
            } else {
                None
            }
        });
        let viewer = v.map(|v| json!({"closing":u32::from(v.closing),"opacity":clock(&v.opacity),"backdrop":clock(&v.backdrop),"scope":viewer_scope(&v.scope)})).unwrap_or(Value::Null);
        assert_eq!(n["viewer"], viewer, "{context} actual nested private scope");
        self.service.disabled = 0;
        self.service.cues = [0; 4];
    }
}

#[test]
#[ignore = "requires original Movie MODEL_NATIVE runner, FONT and SCENARIO; CPU only, never silently skipped"]
fn higurashi_movie_gallery_model_matches_original_110_owners_904_states() {
    // Fail before constructing any Rust expected state if actual original
    // runner output or stderr differs from immutable capture.
    let golden = rerun_original();
    let scenario = HigurashiScenario::new(
        std::fs::read(required("SHIN_HIGURASHI_MOVIE_GALLERY_SCENARIO"))
            .unwrap()
            .into(),
    )
    .unwrap();
    assert_eq!(scenario.movie_info.len(), 13);
    let mut observed = golden.lines().map(|l| {
        let (tag, text) = l.split_once(' ').unwrap();
        (tag, serde_json::from_str::<Value>(text).unwrap())
    });
    let input = std::fs::read_to_string(evidence().join("input.txt")).unwrap();
    let (mut fixture, mut owners, mut states, mut events, mut packets) =
        (None::<Fixture>, 0, 0, 0, 0);
    for (line_no, line) in input.lines().enumerate() {
        let w = line.split_whitespace().collect::<Vec<_>>();
        let op = w[0];
        let u = |i: usize| w[i].parse::<u32>().unwrap();
        let f = |i: usize| w[i].parse::<f32>().unwrap();
        let hex = |i: usize| u32::from_str_radix(w[i], 16).unwrap();
        let mut result = -1;
        if op == "N" {
            assert!(fixture.is_none());
            fixture = Some(Fixture::new(
                &scenario.movie_info[..u(1) as usize],
                hex(2),
                u(3) != 0,
                u(4) != 0,
                u(5).try_into().unwrap(),
            ));
            owners += 1;
        } else {
            let a = fixture.as_mut().unwrap();
            match op {
                "R" => {
                    assert_eq!(a.service.first_status, 0);
                    a.service.atlas = true;
                    a.service.mask = true;
                    a.service.first_status = 2;
                    result = 1;
                }
                "F" => {
                    a.service.buttons = Buttons {
                        pressed: hex(2),
                        repeat: hex(3),
                        ..Default::default()
                    };
                    a.service.captured = u(4) != 0;
                    advance(
                        &mut a.scope,
                        &mut a.model,
                        Ticks::from_f32(f(1)),
                        a.service.atlas,
                        &mut a.service,
                    )
                    .unwrap();
                    result = 0;
                }
                "U" => {
                    a.service.buttons = Buttons {
                        pressed: hex(3),
                        repeat: hex(4),
                        ..Default::default()
                    };
                    a.service.captured = u(2) != 0;
                    result = i32::from(
                        a.model
                            .controller(
                                u(1) != 0,
                                a.service.captured,
                                a.service.buttons,
                                &mut a.service,
                            )
                            .unwrap(),
                    );
                    a.absorb();
                }
                "S" => {
                    let i = u(1);
                    assert!((i as usize) < a.model.rows.len());
                    a.model.x = (i % 4) as i32;
                    a.model.y = (i / 4) as i32;
                }
                "T" => result = i32::from(a.model.probe(Vec2::new(f(1), f(2)))),
                "P" => a.model.press(),
                "E" => {
                    a.model.release(&mut a.service).unwrap();
                    a.absorb();
                }
                "H" => a.model.long_press(),
                "Z" => a.model.reset(),
                "Q" => a.model.cancel(),
                "A" => a.model.drag(Vec2::new(f(1), f(2)), Vec2::new(f(3), f(4))),
                "G" => a.model.gesture(Vec2::new(f(1), f(2))),
                "V" => {
                    a.service.status = u(1);
                    a.service.frame_present = u(2) != 0;
                }
                "I" => {
                    let v = viewer_mut(&mut a.scope).unwrap();
                    result = i32::from(
                        v.controller(u(1) != 0, u(2) != 0, hex(3), &mut a.service)
                            .unwrap(),
                    );
                    v.append_pending();
                }
                "B" => {
                    let v = viewer_mut(&mut a.scope).unwrap();
                    v.pointer_release(&mut a.service).unwrap();
                    v.append_pending();
                }
                "D" => {
                    let playing = a.service.desired;
                    viewer_mut(&mut a.scope)
                        .unwrap()
                        .prepare_player_draw(playing, a.volume, &mut a.service)
                        .unwrap();
                }
                "X" => {
                    retire(&mut a.scope, &mut a.model, &mut a.service).unwrap();
                    assert!(a.scope.is_empty());
                    assert!(!a.service.player_present);
                }
                _ => panic!("Original opcode {op}"),
            }
        }
        let a = fixture.as_mut().unwrap();
        let mut native_events = vec![];
        let (tag, native) = loop {
            let (t, o) = observed.next().unwrap();
            assert_eq!(word(&o["profile"]), owners);
            match t {
                "E" => {
                    if let Some(e) =
                        event_projection(o["tag"].as_str().unwrap(), &words(&o["words"]))
                    {
                        native_events.push(e);
                        events += 1;
                    }
                }
                "D" => {
                    packets += 1;
                }
                "M" | "X" => break (t, o),
                _ => panic!("Native output tag"),
            }
        };
        assert_eq!(
            a.service.events, native_events,
            "Movie owner{owners} line{line_no} op{op} exact ordered service subset"
        );
        a.service.events.clear();
        if op == "X" {
            assert_eq!(tag, "X");
            assert_eq!(word(&native["heap"]), 0);
            assert_eq!(word(&native["fontrefs"]), 1);
            assert_eq!(native["players"], native["player_drops"]);
            assert_eq!(word(&native["players"]), a.service.created);
            assert_eq!(word(&native["player_drops"]), a.service.dropped);
            assert_eq!(native["files"], native["file_drops"]);
            fixture = None;
        } else {
            assert_eq!(tag, "M");
            assert_eq!(native["op"].as_str(), Some(op));
            a.check(
                &native,
                result,
                &format!("Movie owner{owners} line{line_no} {op}"),
            );
            states += 1;
        }
    }
    assert!(fixture.is_none());
    assert!(observed.next().is_none());
    assert_eq!((owners, states, packets), (110, 904, 73));
    assert_eq!(
        events, 730,
        "Exact modeled service subset of frozen1,994 events"
    );
    eprintln!(
        "Movie native pure model: {owners} owners/{states} states/{events} ordered service projections; {packets} native packets retained as transport evidence only; no GPU/audio/window/save"
    );
}

#[test]
fn higurashi_movie_gallery_failure_stops_before_parent_clocks() {
    let mut m = Model::new(&[], &[], false).unwrap();
    let mut t = Transport {
        first_status: 2,
        mask: true,
        fail: Some("stop60"),
        ..Default::default()
    };
    m.publish_initial(&mut t).unwrap();
    let mut scope = ModeStack::default();
    scope.push(Task::first(true));
    assert!(advance(&mut scope, &mut m, Ticks::from_f32(30.), true, &mut t).is_err());
    assert_eq!(m.frame, 0);
    assert!(m.close_flash.is_idle());
    assert_eq!(t.stop60, 0);
    match scope.iter().next().unwrap() {
        Task::First { phase, overlay } => {
            assert_eq!(*phase, 0);
            assert_eq!(overlay.elapsed, 0.);
            assert!(overlay.active);
        }
        _ => panic!("retained First"),
    }
    // Overlay installation is a successful native prefix before the failed
    // audio call; retry/rollback of that prefix belongs to the concrete owner.
}
