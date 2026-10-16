//! Ger Config51550/549a0: one original scrolling list, with deferred reset/save.
use shin_core::format::save::Settings;

pub mod artwork;
pub mod dialog;
pub const ATLAS: &str = "/config.txa";
pub const TEXTURES: &[&str] = &[
    "bg",
    "top",
    "button_shadow",
    "caption",
    "close",
    "default",
    "left",
    "right",
    "slider",
    "focus",
    "separator",
    "category",
    "help_config",
    "key",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Slider(u8),
    Buttons([u8; 2]),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub y: i32,
    pub title: u8,
    pub field: u8,
    pub control: Control,
}
use Control::{Buttons, Slider};
pub const ROWS: [Row; 13] = [
    Row {
        y: 80,
        title: 0,
        field: 3,
        control: Slider(0),
    },
    Row {
        y: 166,
        title: 1,
        field: 0,
        control: Slider(0),
    },
    Row {
        y: 252,
        title: 2,
        field: 1,
        control: Slider(0),
    },
    Row {
        y: 338,
        title: 3,
        field: 2,
        control: Slider(0),
    },
    Row {
        y: 424,
        title: 4,
        field: 4,
        control: Buttons([1, 0]),
    },
    Row {
        y: 584,
        title: 8,
        field: 9,
        control: Slider(2),
    },
    Row {
        y: 670,
        title: 11,
        field: 11,
        control: Buttons([1, 0]),
    },
    Row {
        y: 756,
        title: 15,
        field: 13,
        control: Slider(1),
    },
    Row {
        y: 916,
        title: 10,
        field: 10,
        control: Slider(2),
    },
    Row {
        y: 1002,
        title: 16,
        field: 15,
        control: Buttons([5, 4]),
    },
    Row {
        y: 1088,
        title: 19,
        field: 14,
        control: Buttons([5, 4]),
    },
    Row {
        y: 1174,
        title: 17,
        field: 17,
        control: Buttons([5, 4]),
    },
    Row {
        y: 1260,
        title: 12,
        field: 18,
        control: Buttons([1, 0]),
    },
];
pub const HEIGHT: f32 = 1340.;
pub const VISIBLE: f32 = 796.;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Opening,
    Active,
    ResetFlash,
    ResetDialog,
    NoticeDialog,
    CloseFlash,
    WaitingSave,
    Closing,
    Closed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    ResetConfirmation,
    AutoSleepNotice,
    Save,
    Close,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub reset: bool,
    pub close: bool,
    pub axis: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    None,
    Content,
    Close,
    Reset,
    Row(usize, Option<u8>),
    Heading(usize),
}
#[derive(Clone, Copy, Debug, Default)]
struct Tween {
    from: f32,
    to: f32,
    time: f32,
}
impl Tween {
    fn value(self) -> f32 {
        if self.time >= 4. {
            self.to
        } else {
            (self.to - self.from).mul_add(self.time / 4., self.from)
        }
    }
    fn set(&mut self, v: f32) {
        self.from = self.value();
        self.to = v;
        self.time = 0.;
    }
    fn snap(&mut self, v: f32) {
        self.from = v;
        self.to = v;
        self.time = 4.;
    }
    fn busy(self) -> bool {
        self.time < 4.
    }
}
pub struct ConfigController {
    pub settings: Settings,
    initial: Settings,
    pub selected: usize,
    pub phase: Phase,
    pub progress: f32,
    pub frame: u32,
    pub elapsed: f32,
    scroll: Tween,
    velocity: f32,
    target: Target,
    drag: Option<(usize, u8, f32)>,
    requests: Vec<Request>,
    sounds: Vec<&'static str>,
}
impl ConfigController {
    pub fn new(settings: Settings) -> Self {
        Self {
            initial: settings.clone(),
            settings,
            selected: 0,
            phase: Phase::Opening,
            progress: 0.,
            frame: 0,
            elapsed: 0.,
            scroll: Tween {
                time: 4.,
                ..Default::default()
            },
            velocity: 0.,
            target: Target::None,
            drag: None,
            requests: vec![],
            sounds: vec![],
        }
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn active(&self) -> bool {
        self.phase == Phase::Active && !self.scroll.busy()
    }
    pub fn value(&self, index: usize) -> u8 {
        value(&self.settings, ROWS[index].field)
    }
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    pub fn flash(&self, reset: bool) -> f32 {
        if matches!(
            (self.phase, reset),
            (Phase::ResetFlash, true) | (Phase::CloseFlash, false)
        ) {
            if self.elapsed < 3.5 {
                self.elapsed * 0.2
            } else {
                (7. - self.elapsed).max(0.) * 0.2
            }
        } else {
            0.
        }
    }
    pub fn select(&mut self, i: usize) {
        self.selected = i.min(12);
        let y = ROWS[self.selected].y as f32;
        let cur = self.scroll();
        let next = if y - cur < 80. {
            (y - 80.).max(0.)
        } else if y - cur + 80. > 796. {
            (y + 80. - 796.).min(HEIGHT - VISIBLE)
        } else {
            cur
        };
        self.scroll.set(next);
    }
    pub fn input(&mut self, i: Input) {
        if !self.active() {
            return;
        }
        let old = self.selected;
        let next = if i.up {
            old.saturating_sub(1)
        } else if i.down {
            (old + 1).min(12)
        } else {
            old
        };
        if next != old {
            self.sounds.push("move");
            self.velocity = 0.;
            self.select(next);
        } else if i.reset {
            self.reset();
        } else if i.close {
            self.close();
        } else if i.left || i.right {
            let v = self.value(old);
            let next = match ROWS[old].control {
                Slider(_) => {
                    if i.left {
                        v.max(5) - 5
                    } else {
                        v.min(95) + 5
                    }
                }
                Buttons(_) => {
                    if i.left {
                        v.saturating_sub(1)
                    } else {
                        (v + 1).min(1)
                    }
                }
            };
            if next != v {
                self.edit(old, next);
                self.sounds.push("move");
                self.velocity = 0.;
            }
        } else if i.axis.is_finite() && i.axis != 0. {
            self.set_scroll(
                i.axis
                    .mul_add(32., self.scroll())
                    .clamp(0., HEIGHT - VISIBLE),
            );
        }
    }
    fn edit(&mut self, i: usize, v: u8) {
        let field = ROWS[i].field;
        let previous = self.value(i);
        set(&mut self.settings, field, v);
        if field == 18 && v != previous && v != 0 {
            self.phase = Phase::NoticeDialog;
            self.requests.push(Request::AutoSleepNotice);
        }
    }
    pub fn reset(&mut self) {
        if !self.active() {
            return;
        }
        self.phase = Phase::ResetFlash;
        self.elapsed = 0.;
        self.sounds.push("decide");
        self.cancel_pointer();
    }
    pub fn close(&mut self) {
        if !self.active() {
            return;
        }
        self.phase = Phase::CloseFlash;
        self.elapsed = 0.;
        self.sounds.push("cancel");
        self.cancel_pointer();
    }
    pub fn reset_response(&mut self, final_flag: bool, answer: i32) {
        if self.phase != Phase::ResetDialog {
            return;
        }
        if !final_flag && answer == 0 {
            self.settings = Settings::default();
            self.selected = self.selected.min(12);
        }
        if final_flag {
            self.phase = Phase::Active;
        }
    }
    pub fn notice_finished(&mut self) {
        if self.phase == Phase::NoticeDialog {
            self.phase = Phase::Active;
        }
    }
    pub fn save_finished(&mut self) {
        if self.phase == Phase::WaitingSave {
            self.phase = Phase::Closing;
            self.elapsed = 0.;
        }
    }
    pub fn save_failed(&mut self) {
        if self.phase == Phase::WaitingSave {
            self.phase = Phase::Active;
        }
    }
    pub fn update(&mut self, dt: f32) {
        let dt = if dt.is_finite() { dt.max(0.) } else { 0. };
        match self.phase {
            Phase::Opening => {
                self.progress = dt.mul_add(0.1, self.progress).min(1.);
                if self.progress == 1. {
                    self.phase = Phase::Active;
                }
            }
            Phase::ResetFlash if self.elapsed >= 7. => {
                self.phase = Phase::ResetDialog;
                self.requests.push(Request::ResetConfirmation);
            }
            Phase::CloseFlash if self.elapsed >= 7. => {
                if self.settings.serialized_eq(&self.initial) {
                    self.phase = Phase::Closing;
                    self.elapsed = 0.;
                } else {
                    self.phase = Phase::WaitingSave;
                    self.requests.push(Request::Save);
                }
            }
            Phase::Closing => {
                self.progress = dt.mul_add(-0.1, self.progress).max(0.);
                if self.progress == 0. {
                    self.phase = Phase::Closed;
                    self.requests.push(Request::Close);
                }
            }
            _ => {}
        }
        self.elapsed += dt;
        self.scroll.time = (self.scroll.time + 1.).min(4.);
        if self.velocity != 0. && self.phase == Phase::Active {
            self.set_scroll((self.scroll() - self.velocity).clamp(0., HEIGHT - VISIBLE));
            self.velocity *= 0.95f32.powf(dt);
            if self.velocity.abs() < 0.1 {
                self.velocity = 0.;
            }
        }
        self.frame = self.frame.wrapping_add(1);
    }
    fn set_scroll(&mut self, s: f32) {
        let mut i = self.selected;
        while i + 1 < 13 && s + 80. > ROWS[i].y as f32 {
            i += 1;
        }
        while i > 0 && s + 796. < (ROWS[i].y + 80) as f32 {
            i -= 1;
        }
        self.selected = i;
        self.scroll.snap(s);
    }
    pub fn probe(&mut self, x: f32, y: f32) -> bool {
        self.target = Target::None;
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        if (46. ..154.).contains(&y) {
            if (1766. ..1874.).contains(&x) {
                self.target = Target::Close;
                return true;
            }
            if (1404. ..1744.).contains(&x) {
                self.target = Target::Reset;
                return true;
            }
        }
        if !(160. ..992.).contains(&y) {
            return true;
        }
        if !(160. ..1760.).contains(&x) || y < 178. || y >= 974. {
            return false;
        }
        for (i, (top, bottom)) in [(0., 504.), (504., 836.), (836., 1340.)]
            .into_iter()
            .enumerate()
        {
            let header = if top - self.scroll() >= 0. {
                top - self.scroll()
            } else {
                (bottom - self.scroll() - 80.).min(0.)
            };
            if y >= 178. + header && y < 258. + header {
                self.target = Target::Heading(i);
                return true;
            }
        }
        let local = y + self.scroll() - 178.;
        for (i, r) in ROWS.iter().enumerate() {
            if local >= r.y as f32 && local < (r.y + 80) as f32 {
                let choice = if matches!(r.control, Buttons(_)) {
                    (0..2)
                        .rev()
                        .find(|&c| {
                            x >= 160. + 1094. + c as f32 * 236.
                                && x < 160. + 1318. + c as f32 * 236.
                        })
                        .map(|c| c as u8)
                } else {
                    None
                };
                self.target = Target::Row(i, choice);
                return true;
            }
        }
        self.target = Target::Content;
        true
    }
    pub fn press(&mut self, x: f32, y: f32) {
        if self.phase != Phase::Active {
            return;
        }
        if let Target::Row(i, _) = self.target {
            self.select(i);
            if matches!(ROWS[i].control, Slider(_)) {
                let v = self.value(i);
                let local_x = x - 160.;
                let local_y = y + self.scroll() - 178.;
                let left = (v as f32).mul_add(6., 842.);
                if local_y >= ROWS[i].y as f32
                    && local_y < (ROWS[i].y + 80) as f32
                    && local_x >= left
                    && local_x < left + 48.
                {
                    self.drag = Some((i, v, 0.));
                }
            }
        }
    }
    pub fn release(&mut self) {
        if self.phase != Phase::Active {
            return;
        }
        match self.target {
            Target::Close => {
                self.close();
                if self.sounds.last() == Some(&"cancel") {
                    *self.sounds.last_mut().unwrap() = "decide";
                }
            }
            Target::Reset => self.reset(),
            Target::Row(i, Some(v)) => {
                self.select(i);
                self.edit(i, v);
                self.sounds.push("decide");
            }
            Target::Heading(i) => {
                self.scroll
                    .set([0.0_f32, 504., 836.][i].min(HEIGHT - VISIBLE));
                self.sounds.push("decide");
            }
            _ => {}
        }
        self.drag = None;
    }
    pub fn drag(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        if let Some((i, start, distance)) = self.drag {
            let distance = distance + dx;
            let v = (start as i32 + (distance * (1. / 6.)) as i32).clamp(0, 100) as u8;
            self.drag = Some((i, start, distance));
            self.edit(i, v);
        } else {
            self.set_scroll((self.scroll() - dy).clamp(0., HEIGHT - VISIBLE));
        }
    }
    pub fn gesture(&mut self, y: f32) {
        if y.is_finite() {
            self.velocity = y;
        }
    }
    pub fn cancel_pointer(&mut self) {
        self.target = Target::None;
        self.drag = None;
    }
}
fn value(s: &Settings, f: u8) -> u8 {
    match f {
        0 => s.v0_bgmvol,
        1 => s.v1_sfxvol,
        2 => s.v2_voicevol,
        3 => s.v3_sysvol,
        4 => s.v4_voicefocus.into(),
        9 => s.v9_msgspeed,
        10 => s.v10_skipspeed,
        11 => s.v11_disallowskipunread.into(),
        13 => s.v13_msgwinalpha,
        14 => s.v14_showroutenavi.into(),
        15 => s.v15.into(),
        17 => s.v17_showscenetitle.into(),
        18 => s.v18_showsongtitle.into(),
        _ => unreachable!(),
    }
}
fn set(s: &mut Settings, f: u8, v: u8) {
    match f {
        0 => s.v0_bgmvol = v,
        1 => s.v1_sfxvol = v,
        2 => s.v2_voicevol = v,
        3 => s.v3_sysvol = v,
        4 => s.v4_voicefocus = v != 0,
        9 => s.v9_msgspeed = v,
        10 => s.v10_skipspeed = v,
        11 => s.v11_disallowskipunread = v != 0,
        13 => s.v13_msgwinalpha = v,
        14 => s.v14_showroutenavi = v != 0,
        15 => s.v15 = v != 0,
        17 => s.v17_showscenetitle = v != 0,
        18 => s.v18_showsongtitle = v != 0,
        _ => unreachable!(),
    }
}
#[cfg(test)]
#[path = "config/tests.rs"]
mod tests;
