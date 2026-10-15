use super::*;
use crate::adv::input_area::{PointerEvent, PointerKind};
use glam::ivec2;
use std::{
    fmt::Write as _,
    io::Write as _,
    process::{Command, Stdio},
};

#[derive(Default)]
struct Service {
    events: Vec<Event>,
    playing: bool,
}
impl Services for Service {
    fn event(&mut self, e: Event) {
        self.events.push(e);
    }
    fn voice_is_playing(&self) -> bool {
        self.playing
    }
}
#[derive(Default)]
struct Compare {
    browser: Option<BrowserControls>,
    services: Service,
    input: String,
    expected: Vec<String>,
    number: u32,
    micros: u64,
    scenarios: usize,
}
fn tween(out: &mut String, t: &HigurashiTweener) {
    write!(
        out,
        " {} {}",
        t.value().to_bits(),
        u32::from(t.is_waiting())
    )
    .unwrap();
}
impl Compare {
    fn snapshot(&mut self, result: bool) {
        let s = &self.browser.as_ref().unwrap().state;
        let mut out = format!(
            "B {} {} {} {} {} {} {} {}",
            u32::from(result),
            s.selected,
            s.region,
            s.pointed_row,
            s.velocity.to_bits(),
            s.navigation,
            u32::from(s.blocked),
            s.blink
        );
        tween(&mut out, &s.scroll);
        tween(&mut out, &s.close_flash);
        let v = s.scrollbar;
        write!(
            out,
            " {} {} {} {} {}",
            v.position,
            v.visible,
            v.first,
            v.end,
            s.flashes.len()
        )
        .unwrap();
        for (i, f) in &s.flashes {
            write!(out, " {i}").unwrap();
            tween(&mut out, &f.jump);
            tween(&mut out, &f.voice);
        }
        write!(out, " {}", self.services.events.len()).unwrap();
        for e in self.services.events.drain(..) {
            let (kind, argument) = match e {
                Event::Cue(c) => (
                    match c {
                        Cue::Move => 0,
                        Cue::Cancel => 1,
                        Cue::Confirm => 2,
                    },
                    0,
                ),
                Event::Close => (3, 0),
                Event::Navigation => (4, 0),
                Event::Jump(i) => (5, i as u32),
                Event::Voice(i) => (6, i as u32),
                Event::StopVoice => (7, 0),
                Event::VoiceTick => (8, 0),
                Event::MusicMultiplier(v) => (9, v.to_bits()),
            };
            write!(out, " {kind} {argument}").unwrap();
        }
        self.expected.push(out);
    }
    fn begin(
        &mut self,
        rows: Vec<RowMetadata>,
        viewport: Viewport,
        selected: usize,
        scroll: f32,
        cached: &[usize],
    ) {
        assert!(self.browser.is_none());
        self.number = 0;
        self.micros = 0;
        self.services = Service::default();
        self.scenarios += 1;
        writeln!(
            self.input,
            "N {} {} {} {selected} {} {} {scroll}",
            rows.len(),
            viewport.first,
            viewport.end,
            viewport.minimum,
            viewport.extent
        )
        .unwrap();
        for (i, r) in rows.iter().enumerate() {
            writeln!(
                self.input,
                "{} {} {} {} {}",
                r.kind,
                r.voice_count,
                r.top,
                r.height,
                u8::from(cached.contains(&i))
            )
            .unwrap();
        }
        let mut b = BrowserControls::new(rows, viewport, selected, scroll).unwrap();
        b.set_cached_rows(cached.iter().copied());
        self.browser = Some(b);
        self.snapshot(true);
    }
    fn finish(&mut self) {
        self.input.push_str("D\n");
        self.browser = None;
    }
    fn frame(
        &mut self,
        dt: f32,
        permission: u8,
        pressed: u32,
        repeated: u32,
        duck: bool,
        playing: bool,
        events: &[PointerEvent],
    ) {
        self.number += 1;
        self.micros += 50_000;
        writeln!(
            self.input,
            "F {} {} {permission} {pressed:x} {repeated:x} {dt} {} {} {}",
            self.number,
            self.micros,
            u8::from(duck),
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
        self.services.playing = playing;
        let b = self.browser.as_mut().unwrap();
        let result = b.input(
            permission,
            pressed,
            repeated,
            &PointerFrame {
                number: self.number,
                time_seconds: self.micros as f64 * 1e-6,
                events,
            },
            &mut self.services,
        );
        b.tick(Ticks::from_f32(dt), duck, &mut self.services);
        self.snapshot(result);
    }
    fn idle(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(1.0, 1, 0, 0, false, false, &[]);
        }
    }
    fn external(&mut self, scroll: f32, blocked: bool) {
        writeln!(self.input, "S {scroll} {}", u8::from(blocked)).unwrap();
        let b = self.browser.as_mut().unwrap();
        b.set_scroll(scroll);
        b.set_blocked(blocked);
        self.snapshot(true);
    }
    fn contact(&mut self, op: char, p: Vec2) {
        writeln!(self.input, "{op} {} {}", p.x, p.y).unwrap();
        let b = self.browser.as_mut().unwrap();
        let mut c = Contact {
            state: &mut b.state,
            services: &mut self.services,
        };
        match op {
            'P' => {
                assert!(c.probe(p));
            }
            'H' => c.press(p),
            'R' => c.release(p),
            'G' => c.gesture(p),
            _ => panic!("callback"),
        };
        self.snapshot(true);
    }
    fn drag(&mut self, p: Vec2, c: Vec2) {
        writeln!(self.input, "M {} {} {} {}", p.x, p.y, c.x, c.y).unwrap();
        let b = self.browser.as_mut().unwrap();
        Contact {
            state: &mut b.state,
            services: &mut self.services,
        }
        .drag(p, c);
        self.snapshot(true);
    }
    fn compare(self) {
        assert!(self.browser.is_none());
        let mut process =
            Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_BROWSER_CONTROLS").unwrap())
                .args([
                    std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
                    std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
        let mut stdin = process.stdin.take().unwrap();
        let data = self.input.clone();
        let writer = std::thread::spawn(move || stdin.write_all(data.as_bytes()).unwrap());
        let output = process.wait_with_output().unwrap();
        writer.join().unwrap();
        let dir = std::path::PathBuf::from(
            std::env::var("SHIN_HIGURASHI_BROWSER_CONTROL_CAPTURE").unwrap(),
        );
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("input.txt"), &self.input).unwrap();
        std::fs::write(dir.join("native.txt"), &output.stdout).unwrap();
        std::fs::write(dir.join("native.log"), &output.stderr).unwrap();
        std::fs::write(dir.join("rust.txt"), self.expected.join("\n") + "\n").unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains(&format!(
            "Native browser controls:{} observations; balanced contact/tween/task/font ownership",
            self.expected.len()
        )));
        let actual = String::from_utf8(output.stdout).unwrap();
        let actual: Vec<_> = actual.lines().collect();
        assert_eq!(actual.len(), self.expected.len());
        for (i, (native, rust)) in actual.iter().zip(&self.expected).enumerate() {
            assert_eq!(*native, rust, "browser observation{i}");
        }
        println!(
            "Higurashi browser controls:{} scenarios/{} exact native observations; navigation, contact, inertia, flashes, scrollbar, permission and audio-service requests pass;32 authored measured rows",
            self.scenarios,
            self.expected.len()
        );
    }
}
fn rows(n: usize) -> Vec<RowMetadata> {
    (0..n)
        .map(|i| RowMetadata {
            kind: if i % 5 == 4 {
                2
            } else if i % 7 == 3 {
                1
            } else {
                0
            },
            voice_count: if i % 3 == 0 { 0 } else { 1 },
            top: i as f32 * 120.0,
            height: 120.0,
        })
        .collect()
}
fn viewport(rows: &[RowMetadata], first: usize, end: usize) -> Viewport {
    Viewport {
        first,
        end,
        minimum: rows[first].top,
        extent: rows[end - 1].top + rows[end - 1].height,
    }
}
fn event(kind: PointerKind, x: i32, y: i32) -> PointerEvent {
    PointerEvent {
        kind,
        id: 0,
        position: ivec2(x, y),
    }
}

#[test]
#[ignore = "executes updated native browser contact/navigation/scroll callbacks with checked child/audio boundaries"]
fn higurashi_browser_controls_match_native_navigation_contact_scroll_and_requests() {
    let mut c = Compare::default();
    // Repeat priority, clamping, non-dialogue scans and all button requests.
    for count in [1, 4, 12] {
        for selected in [0, count / 2, count - 1] {
            for cached in [false, true] {
                let r = rows(count);
                let v = viewport(&r, 0, count);
                let indices: Vec<_> = if cached { (0..count).collect() } else { vec![] };
                c.begin(r, v, selected, 0.0, &indices);
                for repeat in [
                    1 << 4,
                    1 << 6,
                    1 << 7,
                    1 << 5,
                    1 << 11,
                    1 << 10,
                    (1 << 4) | (1 << 6),
                    (1 << 7) | (1 << 11),
                    (1 << 5) | (1 << 10),
                ] {
                    c.frame(1.0, 1, 1 << 25, repeat, false, false, &[]);
                    c.idle(4);
                }
                for pressed in [
                    1 << 24,
                    1 << 15,
                    1 << 12,
                    1 << 25,
                    (1 << 25) | (1 << 24),
                    (1 << 12) | (1 << 15),
                ] {
                    c.frame(0.5, 1, pressed, 0, false, false, &[]);
                    c.idle(8);
                }
                c.finish();
            }
        }
    }
    // Nonzero byte permission, measured intervals, negative document anchors,
    // busy scroll suppressing buttons and tick clocks independent of delta.
    for permission in [0, 1, 2, 3, 255] {
        let mut r = rows(16);
        for row in &mut r {
            row.top -= 600.25;
            row.height = 120.25;
        }
        let v = viewport(&r, 2, 14);
        c.begin(r, v, 6, -360.25, &[2, 4, 6, 8, 10, 12]);
        for dt in [0.0, 0.25, 1.0, 4.0, 15.0] {
            for (duck, playing) in [(false, false), (false, true), (true, false), (true, true)] {
                c.frame(dt, permission, 1 << 24, 1 << 6, duck, playing, &[]);
            }
        }
        c.external(-200.5, true);
        c.frame(1.0, permission, 0, 0, true, true, &[]);
        c.finish();
    }
    // Exact hit edges, voice strip, row padding, close rectangle and cached
    // flash ownership. Direct callbacks use authored1920x1080 coordinates.
    for x in [
        149.0, 150.0, 285.0, 286.0, 1763.0, 1764.0, 1769.0, 1770.0, 1847.0, 1848.0,
    ] {
        for y in [
            29.0, 30.0, 113.0, 114.0, 161.0, 162.0, 179.0, 180.0, 263.0, 264.0, 979.0, 980.0,
        ] {
            let r = rows(12);
            let v = viewport(&r, 0, 12);
            c.begin(r, v, 1, 0.0, &[0, 1, 2, 4, 5, 7, 8, 10]);
            c.contact('P', Vec2::new(x, y));
            c.contact('H', Vec2::new(x, y));
            c.idle(2);
            c.contact('R', Vec2::new(x, y));
            c.idle(8);
            c.finish();
        }
    }
    // Vertical drag/gesture inertia, damping/snap, document clamps and angled
    // horizontal navigation. Restore scroll explicitly between direct cases.
    for movement in [
        Vec2::new(0.0, 80.0),
        Vec2::new(0.0, -80.0),
        Vec2::new(80.0, 0.0),
        Vec2::new(-80.0, 0.0),
        Vec2::new(80.0, 46.0),
        Vec2::new(80.0, 47.0),
        Vec2::new(-80.0, -47.0),
        Vec2::new(-80.0, 46.0),
        Vec2::new(0.0, 0.05),
        Vec2::new(15.0, 10.0),
    ] {
        let r = rows(32);
        let v = viewport(&r, 0, 32);
        c.begin(r, v, 12, 1300.5, &[10, 11, 12, 13, 14, 15, 16, 17]);
        c.contact('P', Vec2::new(500.0, 450.0));
        c.drag(Vec2::new(500.0, 450.0), Vec2::new(500.0, 450.0) + movement);
        c.contact('G', movement);
        for dt in [0.0, 0.5, 1.0, 4.0, 30.0, 150.0] {
            c.frame(dt, 1, 0, 0, false, false, &[]);
        }
        c.external(0.0, false);
        c.contact('G', Vec2::new(0.0, 100.0));
        c.idle(2);
        c.external(3022.0, false);
        c.contact('G', Vec2::new(0.0, -100.0));
        c.idle(2);
        c.finish();
    }
    // Physical1280x720 contacts exercise shared capture, hold, movement,
    // release velocity, cancellation, multitouch and permission loss.
    for start in [(1200, 48), (130, 225), (350, 300)] {
        let r = rows(32);
        let v = viewport(&r, 0, 32);
        c.begin(r, v, 8, 800.0, &(0..32).collect::<Vec<_>>());
        c.frame(
            1.0,
            1,
            1 << 25,
            0,
            false,
            false,
            &[event(PointerKind::Down, start.0, start.1)],
        );
        c.idle(4);
        c.frame(1.0, 1, 0, 0, false, false, &[event(PointerKind::Up, 0, 0)]);
        c.idle(8);
        c.frame(
            1.0,
            1,
            0,
            0,
            false,
            false,
            &[event(PointerKind::Down, 350, 400)],
        );
        c.frame(
            1.0,
            1,
            0,
            0,
            false,
            false,
            &[event(PointerKind::Move, 350, 320)],
        );
        c.frame(
            1.0,
            1,
            0,
            0,
            true,
            true,
            &[event(PointerKind::Move, 352, 240)],
        );
        c.frame(1.0, 1, 0, 0, true, true, &[event(PointerKind::Up, 0, 0)]);
        c.idle(5);
        c.frame(
            1.0,
            1,
            0,
            0,
            false,
            false,
            &[event(PointerKind::Down, 1200, 48)],
        );
        c.frame(1.0, 0, 1 << 25, 0, false, false, &[]);
        c.frame(1.0, 1, 0, 0, false, false, &[event(PointerKind::Up, 0, 0)]);
        c.frame(
            1.0,
            1,
            0,
            0,
            false,
            false,
            &[
                event(PointerKind::Down, 350, 400),
                PointerEvent {
                    kind: PointerKind::Down,
                    id: 1,
                    position: ivec2(500, 400),
                },
            ],
        );
        c.frame(
            1.0,
            1,
            0,
            0,
            false,
            false,
            &[
                event(PointerKind::Cancel, 0, 0),
                PointerEvent {
                    kind: PointerKind::Cancel,
                    id: 1,
                    position: ivec2(0, 0),
                },
            ],
        );
        c.finish();
    }
    // Actual updated MSGSET strings supply different measured heights/voice
    // counters; worker publication itself remains a separate native boundary.
    let font = shin_core::format::font::read_font_metrics(&mut std::io::BufReader::new(
        std::fs::File::open(std::env::var("SHIN_HIGURASHI_FONT").unwrap()).unwrap(),
    ))
    .unwrap();
    let mut authored = Vec::new();
    let mut top = -300.0;
    for line in std::fs::read_to_string(std::env::var("SHIN_HIGURASHI_LISTING").unwrap())
        .unwrap()
        .lines()
        .filter(|l| l.contains("MSGSET {"))
    {
        let Some((_, value)) = line.split_once("text: ScriptString(") else {
            continue;
        };
        let Some(value) = value.strip_suffix(") })") else {
            continue;
        };
        if let Ok(text) = serde_json::from_str::<String>(value) {
            if (25..=200).contains(&text.chars().count()) {
                let row = super::super::browser_history::Row {
                    boundary: 1,
                    index: authored.len() as u32,
                    item: Item::Message {
                        text,
                        style: 0,
                        argument: Some(9),
                    },
                };
                let row = PositionedRow::measure(row, top, &font).unwrap();
                top += row.height;
                authored.push(RowMetadata::from(&row));
            }
        }
        if authored.len() == 32 {
            break;
        }
    }
    assert_eq!(authored.len(), 32);
    let v = viewport(&authored, 0, 32);
    c.begin(authored, v, 8, 0.0, &(0..32).collect::<Vec<_>>());
    for _ in 0..12 {
        c.frame(1.0, 1, 0, 1 << 6, false, false, &[]);
        c.idle(4);
    }
    c.contact('P', Vec2::new(500.0, 450.0));
    c.drag(Vec2::new(500.0, 450.0), Vec2::new(500.0, 300.0));
    c.contact('G', Vec2::new(0.0, -35.0));
    c.idle(20);
    c.finish();
    c.compare();
}
