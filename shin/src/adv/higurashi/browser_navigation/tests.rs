use super::super::browser_controls::{RowMetadata, Viewport};
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
}
impl Services for Service {
    fn event(&mut self, event: Event) {
        self.events.push(event);
    }
    fn voice_is_playing(&self) -> bool {
        false
    }
}
#[derive(Default)]
struct Compare {
    model: Option<Navigation>,
    browser: Option<BrowserControls>,
    service: Service,
    input: String,
    expected: Vec<String>,
    frame: u32,
    scenarios: usize,
}
fn tween(out: &mut String, t: &HigurashiTweener) {
    write!(out, " {} {}", t.value().to_bits(), u8::from(t.is_waiting())).unwrap();
}
impl Compare {
    fn snapshot(&mut self, op: char) {
        let s = &self.model.as_ref().unwrap().state;
        let mut out = format!(
            "N {op} {} {} {} {} {} {}",
            s.phase,
            s.slide.to_bits(),
            s.pointed,
            u8::from(s.dragging),
            s.velocity.to_bits(),
            s.selected
        );
        tween(&mut out, &s.scroll);
        write!(out, " {}", s.entries.len()).unwrap();
        for e in &s.entries {
            write!(out, " {} {} {}", e.kind, e.marker, e.row).unwrap();
        }
        let b = self.browser.as_ref().unwrap();
        write!(
            out,
            " {} {} {}",
            b.selected(),
            b.navigation_region(),
            b.navigation()
        )
        .unwrap();
        // Public accessors preserve encapsulation of the parent's core; only
        // the two active tween clocks are needed for this composition proof.
        write!(
            out,
            " {} {} {} {}",
            b.scroll().to_bits(),
            u8::from(b.scroll_waiting()),
            b.close_flash().to_bits(),
            u8::from(b.close_flash_waiting())
        )
        .unwrap();
        write!(out, " {}", self.service.events.len()).unwrap();
        for e in self.service.events.drain(..) {
            let (kind, arg) = match e {
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
            write!(out, " {kind} {arg}").unwrap();
        }
        self.expected.push(out);
    }
    fn begin(
        &mut self,
        count: usize,
        first: usize,
        end: usize,
        selected: usize,
        current: usize,
        markers: Vec<u32>,
        scrambled: bool,
    ) {
        assert!(self.model.is_none());
        self.scenarios += 1;
        self.frame = 0;
        let rows: Vec<_> = (0..count)
            .map(|i| RowMetadata {
                kind: (i % 4) as u8,
                voice_count: if i % 5 == 0 { 0 } else { 1 },
                top: -800.0 + i as f32 * 100.25,
                height: 100.25,
            })
            .collect();
        let indices: Vec<_> = (0..count)
            .map(|i| if scrambled { count - 1 - i } else { i } as u32)
            .collect();
        let extent = -800.0 + count as f32 * 100.25;
        writeln!(
            self.input,
            "N {count} {first} {end} {selected} {current} -800 {extent} -800 {}",
            markers.len()
        )
        .unwrap();
        for (i, r) in rows.iter().enumerate() {
            writeln!(
                self.input,
                "{} {} {} {} {}",
                r.kind, r.voice_count, indices[i], r.top, r.height
            )
            .unwrap();
        }
        for i in &markers {
            write!(self.input, "{i} ").unwrap();
        }
        self.input.push('\n');
        let mut b = BrowserControls::new(
            rows,
            Viewport {
                first,
                end,
                minimum: -800.0,
                extent,
            },
            selected,
            -800.0,
        )
        .unwrap();
        self.model = Some(Navigation::new(&indices, &markers, current, &mut b).unwrap());
        self.browser = Some(b);
        self.snapshot('N');
    }
    fn tick(
        &mut self,
        delta: f32,
        permission: u32,
        pressed: u32,
        repeated: u32,
        events: &[PointerEvent],
    ) {
        self.frame += 1;
        let micros = self.frame as u64 * 16667;
        writeln!(
            self.input,
            "F {} {micros} {permission} {pressed:x} {repeated:x} {delta} {}",
            self.frame,
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
        let b = self.browser.as_mut().unwrap();
        assert_eq!(
            self.model.as_mut().unwrap().poll(
                Ticks::from_f32(delta),
                permission,
                pressed,
                repeated,
                &PointerFrame {
                    number: self.frame,
                    time_seconds: micros as f64 / 1e6,
                    events
                },
                b,
                &mut self.service
            ),
            0
        );
        b.tick(Ticks::from_f32(delta), false, &mut self.service);
        self.snapshot('F');
    }
    fn frames(&mut self, count: usize, delta: f32) {
        for _ in 0..count {
            self.tick(delta, 1, 0, 0, &[]);
        }
    }
    fn contact(&mut self, kind: PointerKind, id: i32, x: i32, y: i32) {
        self.tick(
            1.0,
            1,
            0,
            0,
            &[PointerEvent {
                kind,
                id,
                position: ivec2(x, y),
            }],
        );
    }
    fn finish(&mut self) {
        self.model
            .take()
            .unwrap()
            .retire(self.browser.as_mut().unwrap());
        assert_eq!(self.browser.take().unwrap().navigation(), 0);
        self.input.push_str("D\n");
    }
    fn verify(self) {
        let exe = std::env::var("SHIN_HIGURASHI_EXECUTABLE")
            .unwrap_or_else(|_| "game-data/higurashi/uncompressed_executable_update".into());
        let font = std::env::var("SHIN_HIGURASHI_FONT")
            .unwrap_or_else(|_| "/tmp/higurashi-newrodin.fnt".into());
        let mut child = Command::new(
            std::env::var("SHIN_HIGURASHI_NATIVE_BROWSER_NAVIGATION")
                .expect("native navigation fixture"),
        )
        .args([exe, font])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let input = self.input.clone();
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap().unwrap();
        let dir = std::path::Path::new("/tmp/higurashi-browser-navigation-packets");
        std::fs::create_dir_all(dir).unwrap();
        for (name, data) in [
            ("input.txt", self.input.as_bytes()),
            ("native.txt", &output.stdout),
            ("native.log", &output.stderr),
        ] {
            std::fs::write(dir.join(name), data).unwrap();
        }
        std::fs::write(dir.join("rust.txt"), self.expected.join("\n") + "\n").unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        let native: Vec<_> = text.lines().collect();
        assert_eq!(native.len(), self.expected.len());
        for (i, (actual, expected)) in native.iter().zip(&self.expected).enumerate() {
            assert_eq!(*actual, expected, "navigation observation{i}");
        }
        println!(
            "Higurashi navigation:{} scenarios/{} exact native observations; constructor/sort/input/contact/scroll/parent/cue/retirement composition",
            self.scenarios,
            native.len()
        );
    }
}
#[test]
#[ignore = "actual updated7b184/7bbdc/contact/sort/deleting-destructor bodies; no GPU/audio/saves"]
fn higurashi_browser_navigation_matches_native_entries_input_scroll_and_contacts() {
    let mut c = Compare::default();
    for count in [1, 4, 12, 20, 32, 64] {
        for selected in [0, count / 2, count - 1] {
            for current in [selected, (selected + 1) % count] {
                for density in [0, 1, 2] {
                    let markers = (0..count)
                        .filter(|i| density == 2 || (density == 1 && i % 3 == 0))
                        .map(|i| i as u32)
                        .collect();
                    c.begin(count, 0, count, selected, current, markers, false);
                    c.frames(6, 1.0);
                    for delta in [0.0, 0.25, 1.0, 4.0, 20.0] {
                        c.tick(delta, 2, (1 << 25) | (1 << 24), 1 << 6, &[]);
                        c.tick(delta, 3, 0, 1 << 6, &[]);
                        c.frames(5, delta);
                    }
                    for bit in [4, 6, 7, 5, 11, 10] {
                        c.tick(1.0, 1, 0, 1 << bit, &[]);
                        c.frames(5, 1.0);
                    }
                    c.tick(1.0, 1, 1 << 12, 0, &[]);
                    c.frames(5, 1.0);
                    c.tick(1.0, 1, 1 << 24, 0, &[]);
                    c.tick(1.0, 1, 1 << 15, 0, &[]);
                    c.frames(10, 1.0);
                    c.tick(1.0, 1, 1 << 25, 0, &[]);
                    c.frames(10, 1.0);
                    assert!(c.model.as_ref().unwrap().complete());
                    c.finish();
                }
            }
        }
    }
    // Original comparator orders row then kind; markers retain their source
    // index, even when row.index metadata and marker publication are unsorted.
    for (first, end, selected) in [(0, 64, 0), (10, 45, 22), (31, 32, 31)] {
        c.begin(64, first, end, selected, 50, (0..64).rev().collect(), true);
        c.frames(7, 1.0);
        for bit in [11, 10, 4, 6, 7, 5] {
            c.tick(1.0, 1, 0, 1 << bit, &[]);
            c.frames(5, 1.0);
        }
        c.finish();
    }
    // Contact edges, shared close feedback and row pressing execute in the
    // native InputArea owner. Gesture/scroll cases follow below.
    for (x, y) in [
        (1200, 40),
        (1200, 19),
        (1175, 40),
        (1232, 40),
        (600, 159),
        (600, 160),
        (600, 639),
        (600, 640),
        (600, 108),
        (600, 653),
        (10, 320),
    ] {
        c.begin(24, 0, 24, 12, 15, (0..24).collect(), false);
        c.frames(7, 1.0);
        c.contact(PointerKind::Down, 4, x, y);
        c.contact(PointerKind::Up, 4, x, y);
        c.frames(12, 1.0);
        c.finish();
    }
    for (dx, dy) in [
        (100, 0),
        (-100, 0),
        (0, 100),
        (0, -100),
        (50, 30),
        (-50, 30),
        (10, 1),
        (120, 300),
        (0, 500),
    ] {
        c.begin(32, 0, 32, 16, 17, (0..32).collect(), false);
        c.frames(7, 1.0);
        c.contact(PointerKind::Down, 7, 600, 360);
        c.contact(PointerKind::Move, 7, 600 + dx, 360 + dy);
        c.contact(PointerKind::Up, 7, 600 + dx, 360 + dy);
        c.frames(25, 1.0);
        c.finish();
    }
    for end in [0, 1, 3, 5, 7, 20] {
        c.begin(24, 0, 24, 12, 15, (0..24).collect(), false);
        c.frames(end, 0.25);
        c.finish();
    }
    c.verify();
}
