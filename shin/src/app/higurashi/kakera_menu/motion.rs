//! Updated Kakera First/Start/ChangePage/Decide/Exit and632a8 frame order.
use super::Model;
use crate::app::higurashi::runtime_game_data::RuntimeGameData;
use crate::{
    adv::{
        higurashi::{input::Buttons, reconstruction_mode::OverlayClock},
        input_area::InputAreaTarget,
    },
    layer::higurashi_properties::HigurashiTweener,
};
use glam::Vec2;
use shin_core::time::Ticks;

pub const EXIT_PROMPT: &str = "カケラ紡ぎを終了して、タイトル画面に戻ります。@rよろしいですか？";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Cue(&'static str),
    PlayBgm(i32),
    StopBgm,
    ExitQuestion,
}
pub(in crate::app::higurashi) struct GameplayRequest {
    pub data: RuntimeGameData,
    pub flags: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    First(i32),
    Input,
    PageOut(i32),
    PageIn,
    DecideShake(usize),
    DecideFade(usize),
    ExitShake,
    Question,
    ExitFade,
}
struct Task {
    id: u64,
    kind: Kind,
    done: bool,
}
pub(super) trait Services {
    fn input(&mut self, state: &mut Motion, permission: bool);
    fn question(&mut self, state: &mut Motion, permission: bool, delta: Ticks) -> bool;
    fn next_seed(&mut self) -> u32;
}
pub(in crate::app::higurashi) struct Motion {
    pub model: Model,
    pub overlay: OverlayClock,
    /// Back flash,six card flashes,page slide,blackout,locked notice.
    pub clocks: [HigurashiTweener; 10],
    pub weights: [f32; 6],
    pub counter: u32,
    pub closing: bool,
    pub pointer_kind: i32,
    pub pointer_cell: i32,
    tasks: Vec<Task>,
    pub borrowed: Vec<u64>,
    next_id: u64,
    pub(super) actions: Vec<Action>,
    pub request: Option<GameplayRequest>,
}
fn control(clock: &mut HigurashiTweener, value: f32, ticks: f32, flags: u32) {
    clock
        .control(value, Ticks::from_f32(ticks), flags)
        .expect("Native Kakera tween");
}
impl Motion {
    pub fn new(model: Model, captured: bool) -> Self {
        Self {
            model,
            overlay: OverlayClock::captured(captured),
            clocks: std::array::from_fn(|_| HigurashiTweener::new(0.0)),
            weights: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            counter: 0,
            closing: false,
            pointer_kind: -1,
            pointer_cell: -1,
            tasks: vec![Task {
                id: 1,
                kind: Kind::First(0),
                done: false,
            }],
            borrowed: vec![],
            next_id: 2,
            actions: vec![],
            request: None,
        }
    }
    fn append(&mut self, kind: Kind) {
        self.tasks.push(Task {
            id: self.next_id,
            kind,
            done: false,
        });
        self.next_id += 1;
    }
    pub fn first_phase(&self) -> Option<i32> {
        self.tasks.iter().find_map(|task| {
            if let Kind::First(phase) = task.kind {
                Some(phase)
            } else {
                None
            }
        })
    }
    pub fn first_retiring(&self) -> bool {
        self.first_phase().is_some_and(|phase| phase < 0)
    }
    pub(super) fn tasks(&self) -> impl Iterator<Item = (u64, Kind, bool)> + '_ {
        self.tasks.iter().map(|t| (t.id, t.kind, t.done))
    }
    fn confirm(&mut self) {
        if !self.model.cell_available(self.model.page, self.model.selected) {
            self.actions.push(Action::Cue("error")); return;
        }
        self.actions.push(Action::Cue("decide"));
        let cell = self.model.selected;
        self.clocks[1 + cell].fast_forward_to(self.clocks[1 + cell].value());
        control(
            &mut self.clocks[1 + cell],
            f32::from_bits(0x3f333333),
            f32::from_bits(0x3e4ccccd),
            0x40,
        );
        control(
            &mut self.clocks[1 + cell],
            0.0,
            f32::from_bits(0x3e4ccccd),
            0x40,
        );
        self.append(Kind::DecideShake(cell));
    }
    fn exit(&mut self) {
        self.clocks[0].fast_forward_to(self.clocks[0].value());
        control(
            &mut self.clocks[0],
            f32::from_bits(0x3f333333),
            f32::from_bits(0x3e4ccccd),
            0x40,
        );
        control(&mut self.clocks[0], 0.0, f32::from_bits(0x3e4ccccd), 0x40);
        self.append(Kind::ExitShake);
    }
    fn page(&mut self, direction: i32) {
        if !self.model.any_artwork() { return; }
        control(
            &mut self.clocks[7],
            if direction < 0 { 1920.0 } else { -1920.0 },
            10.0,
            1,
        );
        self.append(Kind::PageOut(direction));
    }
    fn fade(&mut self, next: Kind) {
        self.closing = true;
        self.actions.push(Action::StopBgm);
        control(&mut self.clocks[8], 0.0, 10.0, 0x80);
        control(&mut self.clocks[8], 1.0, 60.0, 0);
        self.append(next);
    }
    pub fn exit_answer(&mut self, stage: u32, button: i32) {
        if stage == 1 && button == 0 {
            self.fade(Kind::ExitFade);
        }
    }
    pub fn controls(&mut self, permission: bool, captured: bool, buttons: Buttons) {
        if !permission || captured || !self.clocks[9].is_idle() {
            return;
        }
        if self.clocks[9].value() > 0.0 {
            if buttons.pressed & 0x03000000 != 0 {
                control(&mut self.clocks[9], 0.0, 10.0, 0);
            }
            return;
        }
        if self.model.can_exit() && buttons.pressed & (1 << 25) != 0 {
            self.actions.push(Action::Cue("cancel"));
            self.exit();
        } else if buttons.pressed & (1 << 24) != 0 {
            self.confirm();
        } else if buttons.pressed & (1 << 10) != 0 {
            self.actions.push(Action::Cue("move"));
            self.page(-1);
        } else if buttons.pressed & (1 << 11) != 0 {
            self.actions.push(Action::Cue("move"));
            self.page(1);
        } else if self.model.navigate(buttons.repeat) {
            self.actions.push(Action::Cue("move"));
        }
    }
    pub(super) fn advance(&mut self, status: u32, delta: Ticks, services: &mut impl Services) {
        self.tasks.retain(|t| !t.done);
        let mut current = std::mem::take(&mut self.tasks);
        self.borrowed = current.iter().map(|t| t.id).collect();
        let mut permission = true;
        for task in current.iter_mut().rev() {
            match task.kind {
                Kind::First(mut phase) => {
                    if phase == 0 && status == 2 {
                        self.actions.push(Action::PlayBgm(self.model.bgm_id));
                        self.overlay.clear(30);
                        phase = 1;
                    }
                    if phase == 1 && !self.overlay.busy() {
                        self.closing = false;
                        self.append(Kind::Input);
                        phase = -1;
                        task.done = true;
                    }
                    self.overlay.advance(delta);
                    task.kind = Kind::First(phase);
                    permission = false;
                }
                Kind::Input => {
                    services.input(self, permission);
                    task.done = self.closing;
                    permission = false;
                }
                Kind::PageOut(direction) => {
                    if self.clocks[7].is_idle() {
                        self.model.turn_page(direction);
                        self.clocks[7].fast_forward_to(if direction < 0 {
                            -1920.0
                        } else {
                            1920.0
                        });
                        control(&mut self.clocks[7], 0.0, 10.0, 2);
                        self.append(Kind::PageIn);
                        task.done = true;
                    }
                    permission = false;
                }
                Kind::PageIn => {
                    task.done = self.clocks[7].is_idle();
                    permission = false;
                }
                Kind::DecideShake(cell) => {
                    if self.clocks[1 + cell].is_idle() {
                        let fragment = self.model.pages[self.model.page][cell];
                        if self.model.statuses[fragment] == 0 {
                            control(&mut self.clocks[9], 1.0, 10.0, 0);
                        } else {
                            self.fade(Kind::DecideFade(fragment));
                        }
                        task.done = true;
                    }
                    permission = false;
                }
                Kind::DecideFade(fragment) => {
                    if self.clocks[8].is_idle() {
                        let mut data = self.model.data.clone();
                        data.progress
                            .set_active_index(1)
                            .expect("Fragment progress record");
                        let entry = &mut data.progress.entries[1];
                        entry.scenario_id =
                            (if self.model.alternate { 12000 } else { 2000 }) + fragment as i32;
                        entry.random_seed = services.next_seed();
                        entry.save_position = 0;
                        entry.replay_target = 0;
                        self.model.data = data.clone();
                        self.request = Some(GameplayRequest { data, flags: 0 });
                        task.done = true;
                    }
                    permission = false;
                }
                Kind::ExitShake => {
                    if self.clocks[0].is_idle() {
                        self.actions.push(Action::ExitQuestion);
                        self.append(Kind::Question);
                        task.done = true;
                    }
                    permission = false;
                }
                Kind::Question => {
                    task.done = services.question(self, permission, delta);
                    permission = false;
                }
                Kind::ExitFade => {
                    if self.clocks[8].is_idle() {
                        let mut data = self.model.data.clone();
                        data.progress
                            .set_active_index(0)
                            .expect("Main-story progress record");
                        self.model.data = data.clone();
                        self.request = Some(GameplayRequest { data, flags: 0 });
                        task.done = true;
                    }
                    permission = false;
                }
            }
        }
        current.append(&mut self.tasks);
        self.tasks = current;
        for i in 0..6 {
            let next = self.weights[i]
                + if i == self.model.selected {
                    f32::from_bits(0x3e4ccccd)
                } else {
                    f32::from_bits(0xbdcccccd)
                };
            self.weights[i] = if i == self.model.selected {
                next.min(1.0)
            } else {
                next.max(0.0)
            };
            self.clocks[1 + i].update(delta);
        }
        self.counter = self.counter.wrapping_add(1);
        for i in [8, 0, 7, 9] {
            self.clocks[i].update(delta);
        }
    }
}
impl InputAreaTarget for Motion {
    fn probe(&mut self, p: Vec2) -> bool {
        self.pointer_cell = -1;
        if !self.clocks[9].is_idle() || self.clocks[9].value() > 0.0 {
            return true;
        }
        self.pointer_kind =
            if self.model.can_exit() && p.y >= 30.0 && p.y < 114.0 && p.x >= 1764.0 && p.x < 1848.0
            {
                1
            } else if p.y >= 150.0 && p.y < 992.0 {
                -1
            } else {
                0
            };
        if self.pointer_kind < 0 {
            self.pointer_cell = self.model.cell_at(p.x, p.y).map_or(-1, |i| i as i32);
        }
        true
    }
    fn reset(&mut self) {
        self.pointer_kind = -1;
        self.pointer_cell = -1;
    }
    fn press(&mut self, _: Vec2) {
        if self.pointer_kind == 1 {
            self.clocks[0].fast_forward_to(self.clocks[0].value());
            control(
                &mut self.clocks[0],
                f32::from_bits(0x3f333333),
                f32::from_bits(0x3e4ccccd),
                0x40,
            );
        } else if self.pointer_kind < 0 && self.pointer_cell >= 0 {
            self.model.selected = self.pointer_cell as usize;
        }
    }
    fn release(&mut self, _: Vec2) {
        if self.pointer_kind == 1 {
            self.actions.push(Action::Cue("decide"));
            self.exit();
        } else if self.pointer_kind < 0 && self.pointer_cell >= 0 {
            self.confirm();
        } else if self.pointer_kind < 0 && self.clocks[9].is_idle() && self.clocks[9].value() > 0.0
        {
            control(&mut self.clocks[9], 0.0, 10.0, 0);
        }
    }
    fn cancel(&mut self) {
        if self.pointer_kind >= 0 {
            self.clocks[0].fast_forward_to(self.clocks[0].value());
            control(&mut self.clocks[0], 0.0, f32::from_bits(0x3e4ccccd), 0x40);
            self.pointer_kind = 0;
        } else {
            self.pointer_cell = -1;
        }
    }
    fn drag(&mut self, _: Vec2, _: Vec2) {}
    fn gesture(&mut self, p: Vec2) {
        if self.pointer_kind >= 0
            || !self.clocks[9].is_idle()
            || self.clocks[9].value() != 0.0
            || p.length() <= 20.0
        {
            return;
        }
        let angle = p.y.atan2(p.x) * 180.0 / f32::from_bits(0x40490fd8);
        let direction = if angle > 150.0 || angle < -150.0 {
            1
        } else if angle > -30.0 && angle < 30.0 {
            -1
        } else {
            return;
        };
        self.actions.push(Action::Cue("move"));
        self.page(direction);
    }
    fn long_press(&mut self, _: Vec2) {
        if self.pointer_kind >= 0 {
            self.cancel();
        }
    }
}

#[cfg(test)]
mod tests;
