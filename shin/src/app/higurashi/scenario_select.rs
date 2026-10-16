//! Updated shared ScenarioSelect b3904/b4d68/b606c and queued scene returns.
//! Assets, actual captured texture, Question and scene-slot ownership are adapters.
use super::runtime_game_data::RuntimeGameData;
use crate::adv::higurashi::{input::Buttons, reconstruction_mode::OverlayClock};
use crate::layer::higurashi_properties::HigurashiTweener;
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::{format::save::higurashi::SaveData, time::Ticks};
pub(super) mod opening;
pub(super) mod owner;
#[path = "quiz_scene/random.rs"]
mod random;
pub(super) mod view;

pub(super) const EXIT_PROMPT: &str = "ゲームを終了して、タイトル画面に戻ります。@rよろしいですか？";
pub(super) const EXIT_LABELS: [&str; 2] = ["はい", "いいえ"];
const ROWS: [[(u32, i32); 15]; 6] = [
    [
        (0, 100),
        (2, 101),
        (3, 102),
        (4, 103),
        (5, 104),
        (6, 105),
        (7, 106),
        (8, 107),
        (1, 108),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ],
    [
        (0, 200),
        (9, 201),
        (10, 202),
        (11, 203),
        (15, 204),
        (1, 205),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ],
    [
        (0, 300),
        (16, 301),
        (17, 302),
        (18, 303),
        (19, 304),
        (1, 305),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ],
    [
        (0, 400),
        (20, 401),
        (21, 402),
        (22, 403),
        (23, 404),
        (24, 405),
        (25, 406),
        (26, 407),
        (1, 408),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ],
    [
        (13, 500),
        (14, 501),
        (27, 502),
        (28, 503),
        (29, 10000),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ],
    [
        (30, 10001),
        (31, 10002),
        (32, 10003),
        (33, 10004),
        (34, 10005),
        (35, 10006),
        (36, 10007),
        (39, 10008),
        (40, 10011),
        (42, -1),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
        (0, 0),
    ],
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SceneRequest {
    Adv { data: RuntimeGameData, flags: u32 },
    Title { selected: i32, mode: i32 },
    Tips { selected: i32, alternate: bool },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Cue(&'static str),
    DisableInput,
    PlayBgm { id: i32, fade: u32, loops: u32 },
    StopBgm(u32),
    ExitQuestion,
    Request(SceneRequest),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    First(i32),
    Entrance,
    Input,
    DecideWait(usize),
    AdvFade(i32),
    ExitWait,
    ExitOpacity,
    ReturnFade,
}
pub(super) struct Task {
    pub kind: Kind,
    pub done: bool,
    /// Metadata clock only. The concrete First must own its retained texture.
    pub capture: Option<OverlayClock>,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct RowState {
    pub art_band: u32,
    pub opacity: f32,
    pub highlight: f32,
    pub flash: f32,
}
#[derive(Clone, Debug)]
pub(super) struct ViewState {
    pub ready: bool,
    #[allow(
        dead_code,
        reason = "Preserve native view mode independently of row layout."
    )]
    pub kind: u32,
    pub mask: u32,
    pub selected: i32,
    pub base_y: f32,
    pub rows: Vec<RowState>,
    pub close_flash: f32,
    pub blackout: f32,
}
pub(super) struct Model {
    pub data: RuntimeGameData,
    #[allow(
        dead_code,
        reason = "Preserve the native append flag for scene inspection."
    )]
    pub append: bool,
    pub kind: u32,
    pub bgm: i32,
    pub count: usize,
    pub mask: u32,
    pub selected: i32,
    pub closing: bool,
    pub ready: bool,
    pub contact: i32,
    pub hover: i32,
    pub opacity: [HigurashiTweener; 15],
    pub highlight: [HigurashiTweener; 15],
    pub hot: [HigurashiTweener; 15],
    pub close_flash: HigurashiTweener,
    pub blackout: HigurashiTweener,
    capture_present: bool,
    pending: Vec<Kind>,
    actions: Vec<Action>,
}
fn control(clock: &mut HigurashiTweener, value: f32, time: f32, flags: u32) {
    clock
        .control(value, Ticks::from_f32(time), flags)
        .expect("Original ScenarioSelect tween flags");
}
fn flash(clock: &mut HigurashiTweener, release: bool) {
    clock.fast_forward_to(clock.value());
    control(
        clock,
        f32::from_bits(0x3f333333),
        f32::from_bits(0x3e4ccccd),
        0x40,
    );
    if release {
        control(clock, 0., f32::from_bits(0x3e4ccccd), 0x40);
    }
}
fn clear_flash(clock: &mut HigurashiTweener) {
    clock.fast_forward_to(clock.value());
    control(clock, 0., f32::from_bits(0x3e4ccccd), 0x40);
}
fn shuffle(count: usize, seed: u32) -> Vec<usize> {
    let mut values = (0..count).collect::<Vec<_>>();
    let mut rng = random::Random::new(seed);
    for i in 0..count.saturating_sub(1) {
        let j = i + rng.uniform((count - i) as u32) as usize;
        values.swap(i, j);
    }
    values
}
fn art_band_available(band: u32, (width, height): (u32, u32)) -> bool {
    (band / 15 + 1) * 720 <= width && (band % 15 + 1) * 120 <= height
}
impl Model {
    /// The source owns both records and every runtime-only vector. Fresh Origin
    /// construction must supply an explicitly initialized desktop data owner;
    /// original b8274's unwritten date/dormant scalars have no safe Rust value.
    pub fn new(
        kind: u32,
        selected: i32,
        append: bool,
        data: &RuntimeGameData,
        settings: &SaveData,
        origin_bgm: Option<i32>,
        capture_present: bool,
    ) -> Result<Self> {
        ensure!(kind <= 5, "ScenarioSelect kind outside original0..5 table");
        let count = match kind {
            0 | 3 => 9,
            1 | 2 => 6,
            4 => 4 + usize::from(append),
            5 => 10,
            _ => unreachable!(),
        };
        ensure!(
            selected >= 0 && (selected as usize) < count,
            "ScenarioSelect selection outside native rows"
        );
        let mut mask = if kind == 0 { 3 } else { 1 };
        let mut unlock = |ids: &[usize], first_bit: usize| {
            for (i, id) in ids.iter().enumerate() {
                if settings.persist_data.get(*id) as u16 != 0 {
                    mask |= 1 << (first_bit + i);
                }
            }
        };
        match kind {
            0 => {
                unlock(&[3, 4, 5, 6, 7, 8], 2);
                if settings.persist_data.get(1) & (1 << 4) != 0 {
                    mask |= 1 << 8;
                }
            }
            1 => {
                unlock(&[9, 10, 11, 12], 1);
                if settings.persist_data.get(1) & (1 << 5) != 0 {
                    mask |= 1 << 5;
                }
            }
            2 => {
                unlock(&[13, 14, 15, 16], 1);
                if settings.persist_data.get(1) & (1 << 6) != 0 {
                    mask |= 1 << 5;
                }
            }
            3 => {
                unlock(&[17, 18, 19, 20, 21, 22, 23], 1);
                if settings.persist_data.get(1) & (1 << 7) != 0 {
                    mask |= 1 << 8;
                }
            }
            4 => {
                unlock(&[162, 163, 164, 165], 0);
                if append {
                    mask |= 1 << 4;
                }
            }
            5 => {
                unlock(&[170, 171, 172, 173, 174, 175, 176, 177], 1);
                mask |= 1 << 9;
            }
            _ => unreachable!(),
        }
        let bgm = match kind {
            0 => 134,
            1 => 135,
            2 => 132,
            3 => 82,
            4 => 131,
            5 => origin_bgm.unwrap_or(-1),
            _ => unreachable!(),
        };
        Ok(Self {
            data: data.clone(),
            append,
            kind,
            bgm,
            count,
            mask,
            selected,
            closing: false,
            ready: false,
            contact: -1,
            hover: -1,
            opacity: std::array::from_fn(|_| HigurashiTweener::new(0.)),
            highlight: std::array::from_fn(|_| HigurashiTweener::new(0.)),
            hot: std::array::from_fn(|_| HigurashiTweener::new(0.)),
            close_flash: HigurashiTweener::new(0.),
            blackout: HigurashiTweener::new(0.),
            capture_present,
            pending: vec![Kind::First(0)],
            actions: vec![],
        })
    }
    pub fn base_y(&self) -> f32 {
        (1080. - (self.count as f32) * 60.) * 0.5
    }
    /// Artwork is a separate permission from persisted unlocks. A base atlas
    /// supplies columns0..1; missing labels must never launch invisible rows.
    pub fn constrain_artwork(&mut self, size: (u32, u32)) {
        for i in 0..self.count {
            if !art_band_available(ROWS[self.kind as usize][i].0, size) {
                self.mask &= !(1 << i);
            }
        }
    }
    pub fn publish_resources(&mut self) {
        self.ready = true;
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
    pub fn take_pending_tasks(&mut self) -> Vec<Task> {
        std::mem::take(&mut self.pending)
            .into_iter()
            .map(|kind| Task {
                kind,
                done: false,
                capture: matches!(kind, Kind::First(_))
                    .then(|| OverlayClock::captured(self.capture_present)),
            })
            .collect()
    }
    pub fn view_state(&self) -> ViewState {
        ViewState {
            ready: self.ready,
            kind: self.kind,
            mask: self.mask,
            selected: self.selected,
            base_y: self.base_y(),
            rows: (0..self.count)
                .map(|i| RowState {
                    art_band: ROWS[self.kind as usize][i].0,
                    opacity: self.opacity[i].value(),
                    highlight: self.highlight[i].value(),
                    flash: self.hot[i].value(),
                })
                .collect(),
            close_flash: self.close_flash.value(),
            blackout: self.blackout.value(),
        }
    }
    pub fn advance_clocks(&mut self, delta: Ticks) {
        self.blackout.update(delta);
        for i in 0..self.count {
            self.opacity[i].update(delta);
            self.highlight[i].update(delta);
            self.hot[i].update(delta);
        }
        self.close_flash.update(delta);
    }
    fn start(&mut self, seed: u32) {
        self.closing = false;
        for (i, row) in shuffle(self.count, seed).into_iter().enumerate() {
            control(&mut self.opacity[row], 0., i as f32 * 4., 0x80);
            control(&mut self.opacity[row], 1., 30., 0);
        }
        control(
            &mut self.highlight[self.selected as usize],
            0.,
            self.count as f32 * 4.,
            0x80,
        );
        control(&mut self.highlight[self.selected as usize], 1., 4., 0);
        self.pending.push(Kind::Entrance);
    }
    fn change_selection(&mut self, next: i32) {
        if next == self.selected {
            return;
        }
        if self.selected >= 0 {
            let c = &mut self.highlight[self.selected as usize];
            c.fast_forward_to(c.value());
            control(c, 0., 32., 0x40);
        }
        let c = &mut self.highlight[next as usize];
        c.fast_forward_to(c.value());
        control(c, 1., 64., 0x40);
        self.selected = next;
    }
    fn decide(&mut self, row: usize, seed: &mut dyn FnMut() -> u32) {
        flash(&mut self.hot[row], true);
        if self.mask & (1 << row) == 0 {
            self.actions.push(Action::Cue("error"));
            return;
        }
        self.actions.push(Action::Cue("decide"));
        self.closing = true;
        let mut delay = 0.;
        for item in shuffle(self.count, seed()) {
            if item == row {
                continue;
            }
            control(&mut self.opacity[item], 0., delay, 0x80);
            control(&mut self.opacity[item], 0., 30., 0);
            delay += 4.;
        }
        control(&mut self.highlight[row], 0., 30., 0);
        control(&mut self.highlight[row], 0., 10., 0x80);
        self.pending.push(Kind::DecideWait(row));
    }
    fn exit(&mut self) {
        flash(&mut self.close_flash, true);
        self.pending.push(Kind::ExitWait);
    }
    pub fn controls(
        &mut self,
        permission: bool,
        captured: bool,
        buttons: Buttons,
        seed: &mut dyn FnMut() -> u32,
    ) -> bool {
        if !permission {
            self.actions.push(Action::DisableInput);
        } else if !captured {
            let b = buttons.repeat;
            let next = if b & (1 << 4) != 0 {
                self.selected - 2
            } else if b & (1 << 6) != 0 {
                self.selected + 2
            } else if b & (1 << 7) != 0 {
                if self.selected & 1 != 0 {
                    self.selected
                } else if self.selected > 0 {
                    self.selected - 1
                } else {
                    self.selected + 1
                }
            } else if self.selected & 1 != 0 && b & (1 << 5) != 0 {
                if self.selected < self.count as i32 - 1 {
                    self.selected + 1
                } else {
                    self.selected - 1
                }
            } else {
                self.selected
            };
            let next = next.clamp(0, self.count as i32 - 1);
            if next != self.selected {
                self.actions.push(Action::Cue("move"));
                self.change_selection(next);
            } else if buttons.pressed & (1 << 25) != 0 {
                self.actions.push(Action::Cue("cancel"));
                self.exit();
            } else if buttons.pressed & (1 << 24) != 0 {
                self.decide(self.selected as usize, seed);
            }
        }
        !self.closing
    }
    pub fn question_callback(&mut self, stage: u32, answer: i32) {
        if stage != 0 || answer != 0 {
            return;
        }
        self.closing = true;
        control(&mut self.highlight[self.selected as usize], 0., 30., 0);
        self.pending.push(Kind::ExitOpacity);
    }
    pub fn probe_contact(&mut self, p: Vec2) -> bool {
        self.hover = -1;
        self.contact = if p.y >= 30. && p.y < 114. && p.x >= 1764. && p.x < 1848. {
            1
        } else if p.y >= 165. && p.y < 984. {
            -1
        } else {
            0
        };
        if self.contact < 0 {
            for i in 0..self.count {
                let x = if i & 1 == 0 { 990. } else { 330. };
                let y = self.base_y() + i as f32 * 60.;
                if p.x >= x && p.x < x + 660. && p.y >= y && p.y < y + 120. {
                    self.hover = i as i32;
                    break;
                }
            }
        }
        true
    }
    pub fn reset_contact(&mut self) {
        self.contact = -1;
        self.hover = -1;
    }
    pub fn press_contact(&mut self) {
        if self.contact == 1 {
            flash(&mut self.close_flash, false);
        } else if self.contact < 0 && self.hover >= 0 {
            self.change_selection(self.hover);
        }
    }
    pub fn release_contact(&mut self, seed: &mut dyn FnMut() -> u32) {
        if self.contact == 1 {
            self.actions.push(Action::Cue("decide"));
            self.exit();
        } else if self.contact < 0 && self.hover >= 0 {
            self.change_selection(self.hover);
            self.decide(self.hover as usize, seed);
        }
    }
    pub fn drag_contact(&mut self) {
        if self.contact >= 0 {
            clear_flash(&mut self.close_flash);
            self.contact = 0;
        } else {
            self.hover = -1;
        }
    }
    pub fn cancel_contact(&mut self) {
        if self.contact >= 0 {
            clear_flash(&mut self.close_flash);
            self.contact = 0;
        }
    }
}
pub(super) trait Services {
    fn next_seed(&mut self) -> u32;
    fn input(&mut self, model: &mut Model, permission: bool) -> Result<()>;
    fn event(&mut self, action: Action) -> Result<()>;
}
impl Task {
    pub fn poll_native(
        &mut self,
        model: &mut Model,
        status: u32,
        delta: Ticks,
        permission: bool,
        services: &mut dyn Services,
    ) -> Result<Vec<Task>> {
        match self.kind {
            Kind::First(mut phase) => {
                let overlay = self.capture.as_mut().expect("First metadata clock");
                if phase == 0 && status == 2 {
                    if model.bgm >= 0 {
                        model.actions.push(Action::PlayBgm {
                            id: model.bgm,
                            fade: 60,
                            loops: 0,
                        });
                    } else {
                        model.actions.push(Action::StopBgm(60));
                    }
                    overlay.clear(30);
                    phase = 1;
                }
                if phase == 1 && !overlay.busy() {
                    model.start(services.next_seed());
                    phase = -1;
                    self.done = true;
                }
                overlay.advance(delta);
                self.kind = Kind::First(phase);
            }
            Kind::Entrance => {
                if model.opacity[..model.count]
                    .iter()
                    .all(HigurashiTweener::is_idle)
                {
                    model.pending.push(Kind::Input);
                    self.done = true;
                }
            }
            Kind::Input => {
                services.input(model, permission)?;
                self.done = model.closing;
            }
            Kind::DecideWait(row) => {
                if model.opacity[..model.count]
                    .iter()
                    .all(HigurashiTweener::is_idle)
                {
                    let id = ROWS[model.kind as usize][row].1;
                    if id < 0 {
                        model.actions.push(Action::Request(SceneRequest::Tips {
                            selected: 0,
                            alternate: true,
                        }));
                    } else {
                        model.actions.push(Action::StopBgm(60));
                        control(&mut model.blackout, 1., 60., 0);
                        model.pending.push(Kind::AdvFade(id));
                    }
                    self.done = true;
                }
            }
            Kind::AdvFade(id) => {
                if model.blackout.is_idle() {
                    model.data.progress.set_active_index(0)?;
                    let p = &mut model.data.progress.entries[0];
                    p.scenario_id = id;
                    p.random_seed = services.next_seed();
                    p.save_position = 0;
                    p.replay_target = 0;
                    model.actions.push(Action::Request(SceneRequest::Adv {
                        data: model.data.clone(),
                        flags: 0,
                    }));
                    self.done = true;
                }
            }
            Kind::ExitWait => {
                if model.close_flash.is_idle() {
                    if model.kind == 5 {
                        model.closing = true;
                        model.actions.push(Action::StopBgm(30));
                        model.actions.push(Action::Request(SceneRequest::Title {
                            selected: 8,
                            mode: -1,
                        }));
                    } else {
                        model.actions.push(Action::ExitQuestion);
                    }
                    self.done = true;
                }
            }
            Kind::ExitOpacity => {
                if model.opacity[..model.count]
                    .iter()
                    .all(HigurashiTweener::is_idle)
                {
                    model.actions.push(Action::StopBgm(60));
                    control(&mut model.blackout, 1., 60., 0);
                    model.pending.push(Kind::ReturnFade);
                    self.done = true;
                }
            }
            Kind::ReturnFade => {
                if model.blackout.is_idle() {
                    model.actions.push(Action::Request(SceneRequest::Adv {
                        data: model.data.clone(),
                        flags: 0,
                    }));
                    self.done = true;
                }
            }
        }
        let spawned = model.take_pending_tasks();
        for action in model.take_actions() {
            services.event(action)?;
        }
        Ok(spawned)
    }
}
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod owner_tests;
