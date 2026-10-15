//! Updated CGMode grid (3b69c/3c794/3d8c8). The concrete owner supplies
//! InputArea, scoped First capture/resources and the separately owned Viewer.
//! This source-free gallery does not project or replace any GameData owner.
use crate::{
    adv::higurashi::{input::Buttons, reconstruction_mode::OverlayClock},
    layer::higurashi_properties::HigurashiTweener,
};
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::{
    format::{save::higurashi::SaveData, scenario::higurashi::PictureBoxInfoItem},
    time::Ticks,
};
pub(super) mod opening;
pub(super) mod owner;
pub(super) mod view;
pub(super) mod viewer;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    pub name: String,
    pub pictures: Vec<u16>,
    pub unlocked: Vec<bool>,
}
impl Entry {
    pub fn locked(&self) -> bool {
        !self.unlocked.iter().any(|x| *x)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Cue(&'static str),
    DisableInput,
    Achievement(u32),
    PlayBgm {
        id: i32,
        loops: u32,
        fade: u32,
    },
    StopBgm(u32),
    /// Actual416b0 constructs Viewer(parent,index); host must retain its shared
    /// scope/controller result owner, not simulate an immediately completed child.
    OpenViewer(usize),
    ReturnTitle {
        selected: i32,
        mode: i32,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    First(i32),
    Input,
    Page {
        phase: i32,
        destination: usize,
        direction: usize,
    },
    ViewerWait {
        cell: usize,
        index: usize,
    },
    ExitWait,
}
pub(super) struct Task {
    pub kind: Kind,
    pub done: bool,
    pub capture: Option<OverlayClock>,
}
#[derive(Clone, Debug)]
pub(super) struct CellState {
    pub x: f32,
    pub focus: f32,
    pub flash: f32,
}
#[derive(Clone, Debug)]
pub(super) struct ViewState {
    pub ready: bool,
    pub entries: Vec<Entry>,
    pub page: usize,
    pub pages: usize,
    pub cell: usize,
    pub spin: f32,
    pub cells: [CellState; 12],
    pub close_flash: f32,
    #[allow(dead_code, reason = "Native view metadata retained for completion-caption inspection.")]
    pub completion: String,
}
pub(super) struct Model {
    pub entries: Vec<Entry>,
    pub completion: String,
    pub pages: usize,
    pub page: usize,
    pub x: usize,
    pub y: usize,
    pub closing: bool,
    pub ready: bool,
    pub frame: u32,
    pub spin: f32,
    pub contact: i32,
    pub hover: i32,
    pub gesture: i32,
    pub positions: [HigurashiTweener; 12],
    pub focus: [f32; 12],
    pub hot: [HigurashiTweener; 12],
    pub close_flash: HigurashiTweener,
    capture_present: bool,
    pending: Vec<Kind>,
    actions: Vec<Action>,
}
const PAGE_DELAYS: [[i32; 12]; 2] = [
    [0, 1, 2, 3, 1, 2, 3, 4, 2, 3, 4, 5],
    [3, 2, 1, 0, 4, 3, 2, 1, 5, 4, 3, 2],
];
fn base_x(i: usize) -> f32 {
    (117 + (i & 3) * 420 + (i >> 2) * 36) as f32
}
fn control(c: &mut HigurashiTweener, value: f32, time: f32, flags: u32) {
    c.control(value, Ticks::from_f32(time), flags)
        .expect("Original CG tween flags");
}
fn flash(c: &mut HigurashiTweener, release: bool) {
    c.fast_forward_to(c.value());
    control(
        c,
        f32::from_bits(0x3f333333),
        f32::from_bits(0x3e4ccccd),
        0x40,
    );
    if release {
        control(c, 0., f32::from_bits(0x3e4ccccd), 0x40);
    }
}
fn clear_flash(c: &mut HigurashiTweener) {
    c.fast_forward_to(c.value());
    control(c, 0., f32::from_bits(0x3e4ccccd), 0x40);
}
impl Model {
    pub fn new(
        catalog: &[PictureBoxInfoItem],
        settings: &SaveData,
        capture_present: bool,
    ) -> Result<Self> {
        ensure!(
            !catalog.is_empty(),
            "CG catalog is empty (native grid requires a valid selected entry)"
        );
        let mut entries = Vec::with_capacity(catalog.len());
        let mut total = 0usize;
        let mut unlocked = 0usize;
        for item in catalog {
            let pictures = item.picture_ids.0.iter().copied().collect::<Vec<_>>();
            ensure!(
                !pictures.is_empty() && pictures.len() <= 35,
                "CG native catalog record capacity"
            );
            let flags = pictures
                .iter()
                .map(|id| {
                    let id = *id as usize;
                    settings
                        .save_vectors
                        .unlocked_cgs
                        .get(id >> 5)
                        .is_some_and(|w| w & (1 << (id & 31)) != 0)
                })
                .collect::<Vec<_>>();
            total += pictures.len();
            unlocked += flags.iter().filter(|x| **x).count();
            entries.push(Entry {
                name: item.name.to_string(),
                pictures,
                unlocked: flags,
            });
        }
        ensure!(
            total <= i32::MAX as usize / 1000,
            "CG completion numerator exceeds original signed arithmetic"
        );
        let ratio = unlocked * 1000 / total;
        let completion = if ratio >= 1000 {
            "100".into()
        } else {
            format!("{}.{}", ratio / 10, ratio % 10)
        };
        let actions = if unlocked >= total {
            vec![Action::Achievement(35)]
        } else {
            vec![]
        };
        Ok(Self {
            pages: entries.len().div_ceil(12),
            entries,
            completion: format!("達成率:{completion}％"),
            page: 0,
            x: 0,
            y: 0,
            closing: false,
            ready: false,
            frame: 0,
            spin: 0.,
            contact: -1,
            hover: -1,
            gesture: -1,
            positions: std::array::from_fn(|i| HigurashiTweener::new(base_x(i))),
            focus: [0.5; 12],
            hot: std::array::from_fn(|_| HigurashiTweener::new(0.)),
            close_flash: HigurashiTweener::new(0.),
            capture_present,
            pending: vec![Kind::First(0)],
            actions,
        })
    }
    pub fn cell(&self) -> usize {
        self.x + 4 * self.y
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
            entries: self.entries.clone(),
            page: self.page,
            pages: self.pages,
            cell: self.cell(),
            spin: self.spin,
            cells: std::array::from_fn(|i| CellState {
                x: self.positions[i].value(),
                focus: self.focus[i],
                flash: self.hot[i].value(),
            }),
            close_flash: self.close_flash.value(),
            completion: self.completion.clone(),
        }
    }
    /// Must follow the owner's one borrowed mixed-scope poll. Newly appended
    /// tasks first poll next frame; all grid clocks still advance this frame.
    pub fn advance_clocks(&mut self, delta: Ticks) {
        let d = delta.as_f32();
        if self.ready {
            self.frame = self.frame.wrapping_add(1);
            let selected = self.cell();
            for i in 0..12 {
                self.positions[i].update(delta);
                if i == selected {
                    self.focus[i] = (self.focus[i] + d * 0.125).min(1.);
                } else {
                    self.focus[i] = (self.focus[i] - d * 0.125).max(0.5);
                }
                self.hot[i].update(delta);
            }
        }
        self.close_flash.update(delta);
        self.spin += d * f32::from_bits(0x3c888889);
    }
    fn page_animation(&mut self, direction: usize, incoming: bool) {
        for i in 0..12 {
            let x = base_x(i);
            let offset = if direction == 0 { 1920. } else { -1920. };
            self.positions[i].fast_forward_to(if incoming { x + offset } else { x });
            control(
                &mut self.positions[i],
                0.,
                (PAGE_DELAYS[direction][i] * 2) as f32,
                0x80,
            );
            control(
                &mut self.positions[i],
                if incoming { x } else { x - offset },
                16.,
                if incoming { 2 } else { 1 },
            );
        }
    }
    fn change_page(&mut self, step: i32) {
        self.actions.push(Action::Cue("move"));
        let destination = (self.page as i32 + step).rem_euclid(self.pages as i32) as usize;
        let direction = usize::from(step < 1);
        self.page_animation(direction, false);
        self.pending.push(Kind::Page {
            phase: 0,
            destination,
            direction,
        });
    }
    fn decide(&mut self) {
        let cell = self.cell();
        flash(&mut self.hot[cell], true);
        let index = self.page * 12 + cell;
        if self.entries[index].locked() {
            self.actions.push(Action::Cue("error"));
        } else {
            self.actions.push(Action::Cue("decide"));
            self.pending.push(Kind::ViewerWait { cell, index });
        }
    }
    fn exit(&mut self) {
        self.closing = true;
        flash(&mut self.close_flash, true);
        self.pending.push(Kind::ExitWait);
    }
    pub fn controls(&mut self, permission: bool, captured: bool, buttons: Buttons) -> bool {
        if !permission {
            self.actions.push(Action::DisableInput);
        } else if !captured {
            let b = buttons.repeat;
            if buttons.pressed & (1 << 25) != 0 {
                self.actions.push(Action::Cue("cancel"));
                self.exit();
            } else if buttons.pressed & (1 << 24) != 0 {
                self.decide();
            } else if b & 0xc00 != 0 {
                self.change_page(if b & (1 << 10) != 0 { -1 } else { 1 });
            } else {
                let dx = if b & (1 << 7) != 0 {
                    -1
                } else {
                    i32::from(b & (1 << 5) != 0)
                };
                let dy = if b & (1 << 4) != 0 {
                    -1
                } else {
                    i32::from(b & (1 << 6) != 0)
                };
                let (oldx, oldy) = (self.x, self.y);
                if dy != 0 {
                    loop {
                        self.y = (self.y as i32 + dy).rem_euclid(3) as usize;
                        if self.page * 12 + self.x + 4 * self.y < self.entries.len() {
                            break;
                        }
                    }
                }
                if dx != 0 {
                    loop {
                        self.x = (self.x as i32 + dx).rem_euclid(4) as usize;
                        if self.page * 12 + self.x + 4 * self.y < self.entries.len() {
                            break;
                        }
                    }
                }
                if (oldx, oldy) != (self.x, self.y) {
                    self.actions.push(Action::Cue("move"));
                }
            }
        }
        !self.closing
    }
    pub fn probe_contact(&mut self, p: Vec2) -> bool {
        self.hover = -1;
        self.contact = if p.y >= 30. && p.y < 114. && p.x >= 1764. && p.x < 1848. {
            1
        } else if p.y >= 162. && p.y < 980. {
            -1
        } else {
            0
        };
        if self.contact < 0 {
            for i in 0..12 {
                let x = base_x(i);
                let y = 222. + (i >> 2) as f32 * 252.;
                if p.x >= x
                    && p.x < x + 384.
                    && p.y >= y
                    && p.y < y + 216.
                    && self.page * 12 + i < self.entries.len()
                {
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
        self.gesture = -1;
    }
    pub fn press_contact(&mut self) {
        if self.contact == 1 {
            flash(&mut self.close_flash, false);
        } else if self.contact < 0 && self.hover >= 0 {
            self.x = self.hover as usize & 3;
            self.y = self.hover as usize >> 2;
        }
    }
    pub fn release_contact(&mut self) {
        if self.contact == 1 {
            self.actions.push(Action::Cue("decide"));
            self.exit();
        } else if self.contact < 0 && self.hover >= 0 {
            self.x = self.hover as usize & 3;
            self.y = self.hover as usize >> 2;
            self.decide();
        }
    }
    pub fn drag_contact(&mut self) {
        if self.contact >= 0 {
            clear_flash(&mut self.close_flash);
            self.contact = 0;
        } else {
            self.hover = -1;
            self.gesture = -1;
        }
    }
    pub fn cancel_contact(&mut self) {
        if self.contact >= 0 {
            clear_flash(&mut self.close_flash);
            self.contact = 0;
        }
    }
    pub fn gesture_contact(&mut self, velocity: Vec2) {
        if self.contact >= 0 {
            return;
        }
        let angle = velocity.y.atan2(velocity.x) / f32::from_bits(0x40490fd8) * 180.;
        let length = (velocity.x * velocity.x + velocity.y * velocity.y).sqrt();
        if length > 20. {
            if angle > 150. || angle < -150. {
                self.change_page(1);
            } else if angle < 30. && angle > -30. {
                self.change_page(-1);
            }
        }
    }
}
pub(super) trait Services {
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
                let c = self.capture.as_mut().expect("CG First capture metadata");
                if phase == 0 && status == 2 {
                    c.masked(30, f32::from_bits(0x3e99999a), 0, true);
                    model.actions.push(Action::PlayBgm {
                        id: 75,
                        loops: 0,
                        fade: 60,
                    });
                    phase = 1;
                }
                if phase == 1 && !c.busy() {
                    model.closing = false;
                    model.pending.push(Kind::Input);
                    phase = -1;
                    self.done = true;
                }
                c.advance(delta);
                self.kind = Kind::First(phase);
            }
            Kind::Input => {
                services.input(model, permission)?;
                self.done = model.closing;
            }
            Kind::Page {
                mut phase,
                destination,
                direction,
            } => {
                if phase == 0 && model.positions.iter().all(HigurashiTweener::is_idle) {
                    model.page = destination;
                    if model.page * 12 + model.cell() >= model.entries.len() {
                        let last = model.entries.len() - 1;
                        model.x = last % 4;
                        model.y = (last / 4) % 3;
                    }
                    model.page_animation(direction, true);
                    phase = 1;
                }
                if phase == 1 && model.positions.iter().all(HigurashiTweener::is_idle) {
                    phase = -1;
                    self.done = true;
                }
                self.kind = Kind::Page {
                    phase,
                    destination,
                    direction,
                };
            }
            Kind::ViewerWait { cell, index } => {
                if model.hot[cell].is_idle() {
                    model.actions.push(Action::OpenViewer(index));
                    self.done = true;
                }
            }
            Kind::ExitWait => {
                if model.close_flash.is_idle() {
                    model.actions.push(Action::StopBgm(30));
                    model.actions.push(Action::ReturnTitle {
                        selected: 15,
                        mode: -1,
                    });
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
