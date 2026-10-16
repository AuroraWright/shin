//! Original Ger CGMode494C0/4A4B0/4B6F0; the factory4E730 replaces the outer Scene.
//! Asset, GPU and sound services live in the app host. CPU state is standalone-testable.
#[path = "cg_gallery/artwork.rs"]
pub mod artwork;
#[path = "cg_gallery/motion.rs"]
pub mod motion;
#[cfg(test)]
#[path = "cg_gallery/tests.rs"]
mod tests;
#[path = "cg_gallery/viewer.rs"]
pub mod viewer;
use motion::{Motion, flash};
pub const ATLAS: &str = "/cgmode.txa";
pub const TEXTURES: &[&str] = &[
    "bg",
    "logo",
    "check",
    "num",
    "top",
    "caption",
    "close",
    "button_shadow",
    "numbg",
    "help_cgmode",
    "key",
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub pictures: Vec<u16>,
    pub unlocked: Vec<bool>,
}
impl Entry {
    pub fn locked(&self) -> bool {
        !self.unlocked.iter().any(|v| *v)
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub confirm: bool,
    pub cancel: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub previous: bool,
    pub next: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    OpenViewer(usize),
    ReturnTitle { requested_id: i32 },
    PlayBgm { id: i32, fade: u32 },
    StopBgm(u32),
    Achievement(u32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    First(u8),
    Input,
    Page {
        phase: u8,
        destination: usize,
        direction: usize,
    },
    ViewerWait {
        cell: usize,
        index: usize,
    },
    ExitWait,
}
#[derive(Clone, Copy, Debug)]
pub struct Task {
    pub kind: TaskKind,
    pub done: bool,
}
#[derive(Clone, Debug)]
pub struct Controller {
    pub entries: Vec<Entry>,
    pub page: usize,
    pub x: usize,
    pub y: usize,
    pub closing: bool,
    pub ready: bool,
    pub frame: u32,
    pub spin: f32,
    pub contact: i32,
    pub hover: i32,
    pub positions: [Motion; 12],
    pub focus: [f32; 12],
    pub hot: [Motion; 12],
    pub close_flash: Motion,
    pub tasks: Vec<Task>,
    pub capture_elapsed: f32,
    pub disabled: u32,
    worker_ready: bool,
    viewer_owned: bool,
    sounds: Vec<&'static str>,
    actions: Vec<Action>,
}
fn base_x(i: usize) -> f32 {
    (117 + (i & 3) * 420 + (i >> 2) * 36) as f32
}
const PAGE_DELAYS: [[u32; 12]; 2] = [
    [0, 1, 2, 3, 1, 2, 3, 4, 2, 3, 4, 5],
    [3, 2, 1, 0, 4, 3, 2, 1, 5, 4, 3, 2],
];
impl Controller {
    pub fn new(entries: Vec<Entry>) -> Result<Self, &'static str> {
        if entries.is_empty()
            || entries.iter().any(|e| {
                e.pictures.is_empty()
                    || e.pictures.len() > 0x7fff
                    || e.pictures.len() != e.unlocked.len()
            })
        {
            return Err("CG catalog requires nonempty groups and matching unlock flags");
        }
        let complete = entries.iter().all(|e| e.unlocked.iter().all(|u| *u));
        Ok(Self {
            entries,
            page: 0,
            x: 0,
            y: 0,
            closing: false,
            ready: false,
            frame: 0,
            spin: 0.,
            contact: -1,
            hover: -1,
            positions: std::array::from_fn(|i| Motion::new(base_x(i))),
            focus: [0.5; 12],
            hot: std::array::from_fn(|_| Motion::new(0.)),
            close_flash: Motion::new(0.),
            tasks: vec![Task {
                kind: TaskKind::First(0),
                done: false,
            }],
            capture_elapsed: 0.,
            disabled: 0,
            worker_ready: false,
            viewer_owned: false,
            sounds: vec![],
            actions: if complete {
                vec![Action::Achievement(13)]
            } else {
                vec![]
            },
        })
    }
    pub fn assets_ready(&mut self) {
        self.ready = true;
        self.worker_ready = true;
    }
    pub fn pages(&self) -> usize {
        self.entries.len().div_ceil(12)
    }
    pub fn cell(&self) -> usize {
        self.x + 4 * self.y
    }
    pub fn selected(&self) -> usize {
        self.page * 12 + self.cell()
    }
    pub fn accepts_input(&self) -> bool {
        self.tasks
            .iter()
            .any(|t| matches!(t.kind, TaskKind::Input) && !t.done)
            && !self.closing
            && !self.viewer_owned
            && !self
                .tasks
                .iter()
                .any(|t| !t.done && !matches!(t.kind, TaskKind::Input))
    }
    /// Desktop resource-error recovery after a failed fresh Title load.
    /// This is an explicit host boundary, not native C++ unwind behavior.
    pub fn recover_return(&mut self) {
        self.closing = false;
        self.viewer_owned = false;
        self.actions.clear();
        self.close_flash.snap(0.);
        self.tasks = vec![Task {
            kind: TaskKind::Input,
            done: false,
        }];
        self.actions.push(Action::PlayBgm { id: 11, fade: 0 });
    }
    pub fn set_viewer_owned(&mut self, owned: bool) {
        self.viewer_owned = owned;
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    fn push(&mut self, kind: TaskKind) {
        self.tasks.push(Task { kind, done: false });
    }
    fn animate_page(&mut self, direction: usize, incoming: bool) {
        let offset = if direction == 0 { 1920. } else { -1920. };
        for i in 0..12 {
            let x = base_x(i);
            self.positions[i].snap(if incoming { x + offset } else { x });
            self.positions[i].target(
                if incoming { x + offset } else { x },
                PAGE_DELAYS[direction][i] as f32 * 2.,
                0,
                false,
            );
            self.positions[i].enqueue(
                if incoming { x } else { x - offset },
                16.,
                if incoming { 2 } else { 1 },
                false,
            );
        }
    }
    fn change_page(&mut self, step: i32) {
        self.sounds.push("move");
        let destination = (self.page as i32 + step).rem_euclid(self.pages() as i32) as usize;
        let direction = usize::from(step < 1);
        self.animate_page(direction, false);
        self.push(TaskKind::Page {
            phase: 0,
            destination,
            direction,
        });
    }
    pub fn decide(&mut self) {
        let cell = self.cell();
        flash(&mut self.hot[cell], true);
        let index = self.selected();
        if self.entries[index].locked() {
            self.sounds.push("error");
        } else {
            self.sounds.push("decide");
            self.push(TaskKind::ViewerWait { cell, index });
        }
    }
    fn exit(&mut self) {
        self.closing = true;
        flash(&mut self.close_flash, true);
        self.push(TaskKind::ExitWait);
    }
    /// Original4B6F0 priority B29, A28, L10, R11, vertical24/25, horizontal26/27.
    pub fn controls(&mut self, permission: bool, captured: bool, input: Input) -> bool {
        if !permission {
            self.disabled += 1;
        } else if !captured {
            if input.cancel {
                self.sounds.push("cancel");
                self.exit();
            } else if input.confirm {
                self.decide();
            } else if input.previous || input.next {
                self.change_page(if input.previous { -1 } else { 1 });
            } else {
                let old = (self.x, self.y);
                let dy = if input.up { -1 } else { i32::from(input.down) };
                let dx = if input.left {
                    -1
                } else {
                    i32::from(input.right)
                };
                if dy != 0 {
                    loop {
                        self.y = (self.y as i32 + dy).rem_euclid(3) as usize;
                        if self.selected() < self.entries.len() {
                            break;
                        }
                    }
                }
                if dx != 0 {
                    loop {
                        self.x = (self.x as i32 + dx).rem_euclid(4) as usize;
                        if self.selected() < self.entries.len() {
                            break;
                        }
                    }
                }
                if old != (self.x, self.y) {
                    self.sounds.push("move");
                }
            }
        }
        !self.closing
    }
    /// Original TaskGroup reverse fixed snapshot, then row/flash clocks and FMA spin.
    pub fn update(&mut self, dt: f32, permission: bool, captured: bool, input: Input) {
        if !dt.is_finite() || dt < 0. {
            return;
        }
        self.tasks.retain(|t| !t.done);
        let count = self.tasks.len();
        let mut permission = permission && !self.viewer_owned;
        for i in (0..count).rev() {
            let mut kind = self.tasks[i].kind;
            let done = match kind {
                TaskKind::First(mut phase) => {
                    if phase == 0 && self.worker_ready {
                        self.capture_elapsed = 0.;
                        self.actions.push(Action::StopBgm(60));
                        phase = 1;
                    }
                    if phase == 1 && self.capture_elapsed >= 60. {
                        self.closing = false;
                        self.actions.push(Action::PlayBgm { id: 11, fade: 0 });
                        self.push(TaskKind::Input);
                        phase = 2;
                    }
                    if phase == 1 {
                        self.capture_elapsed = (self.capture_elapsed + dt).min(60.);
                    }
                    kind = TaskKind::First(phase);
                    phase == 2
                }
                TaskKind::Input => {
                    let keep = self.controls(permission, captured, input);
                    permission = false;
                    !keep
                }
                TaskKind::Page {
                    mut phase,
                    destination,
                    direction,
                } => {
                    permission = false;
                    if phase == 0 && self.positions.iter().all(|p| !p.busy()) {
                        self.page = destination;
                        if self.selected() >= self.entries.len() {
                            let last = self.entries.len() - 1;
                            self.x = last % 4;
                            self.y = (last / 4) % 3;
                        }
                        self.animate_page(direction, true);
                        phase = 1;
                    }
                    let done = phase == 1 && self.positions.iter().all(|p| !p.busy());
                    kind = TaskKind::Page {
                        phase,
                        destination,
                        direction,
                    };
                    done
                }
                TaskKind::ViewerWait { cell, index } => {
                    permission = false;
                    if !self.hot[cell].busy() {
                        self.actions.push(Action::OpenViewer(index));
                        true
                    } else {
                        false
                    }
                }
                TaskKind::ExitWait => {
                    permission = false;
                    if !self.close_flash.busy() {
                        self.actions.push(Action::StopBgm(30));
                        self.actions.push(Action::ReturnTitle { requested_id: 8 });
                        true
                    } else {
                        false
                    }
                }
            };
            self.tasks[i] = Task { kind, done };
        }
        if self.ready {
            self.frame = self.frame.wrapping_add(1);
            let selected = self.cell();
            for i in 0..12 {
                self.positions[i].update(dt);
                self.focus[i] = dt
                    .mul_add(if selected == i { 0.125 } else { -0.125 }, self.focus[i])
                    .clamp(0.5, 1.);
                self.hot[i].update(dt);
            }
        }
        self.close_flash.update(dt);
        self.spin = dt.mul_add(f32::from_bits(0x3c888889), self.spin);
    }
    pub fn probe(&mut self, x: f32, y: f32) -> bool {
        self.hover = -1;
        self.contact = if (46.0..154.0).contains(&y) && (1766.0..1874.0).contains(&x) {
            1
        } else if (162.0..980.0).contains(&y) {
            -1
        } else {
            0
        };
        if self.contact < 0 {
            for i in 0..12 {
                let left = base_x(i);
                let top = 222. + (i >> 2) as f32 * 252.;
                if (left..left + 384.).contains(&x)
                    && (top..top + 216.).contains(&y)
                    && self.page * 12 + i < self.entries.len()
                {
                    self.hover = i as i32;
                    break;
                }
            }
        }
        true
    }
    pub fn reset(&mut self) {
        self.contact = -1;
        self.hover = -1;
    }
    pub fn press(&mut self) {
        if self.contact == 1 {
            flash(&mut self.close_flash, false);
        } else if self.contact < 0 && self.hover >= 0 {
            let i = self.hover as usize;
            self.x = i & 3;
            self.y = i >> 2;
            flash(&mut self.hot[i], false);
        }
    }
    pub fn release(&mut self) {
        if self.contact == 1 {
            self.sounds.push("decide");
            self.exit();
        } else if self.contact < 0 && self.hover >= 0 {
            let i = self.hover as usize;
            self.x = i & 3;
            self.y = i >> 2;
            self.decide();
        }
    }
    pub fn cancel(&mut self) {
        if self.contact >= 0 {
            self.close_flash.target(0., 0.2, 0, true);
            self.contact = 0;
        } else if self.hover >= 0 {
            self.hot[self.hover as usize].target(0., 0.2, 0, true);
        }
    }
    pub fn drag(&mut self) {
        if self.contact >= 0 {
            self.close_flash.target(0., 0.2, 0, true);
            self.contact = 0;
        } else if self.hover >= 0 {
            self.hot[self.hover as usize].target(0., 0.2, 0, true);
            self.hover = -1;
        }
    }
    pub fn gesture(&mut self, x: f32, y: f32) {
        if self.contact >= 0 {
            return;
        }
        let norm = x.mul_add(x, y * y).sqrt();
        if norm <= 20. || !norm.is_finite() {
            return;
        }
        let angle = y.atan2(x) / f32::from_bits(0x40490fd8) * 180.;
        if angle > 150. || angle < -150. {
            self.change_page(1);
        } else if angle < 30. && angle > -30. {
            self.change_page(-1);
        }
    }
}
