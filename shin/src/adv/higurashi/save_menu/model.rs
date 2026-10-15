//! Updated adb3c/b102c/b14d0/b30d8/b343c controls, clocks and callback stages.
use crate::{
    adv::{
        higurashi::{
            browser_controls::Scrollbar,
            input::Buttons,
            mode_stack::{Mode as OwnedMode, ModePush, ModeStack},
        },
        input_area::InputAreaTarget,
    },
    layer::higurashi_properties::HigurashiTweener,
};
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::time::Ticks;

pub(crate) const ROWS: usize = 100;
pub(crate) const VIEW_HEIGHT: i32 = 818;
pub(crate) const DOCUMENT_HEIGHT: i32 = 20618;
const MAX_SCROLL: f32 = 19800.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum Mode {
    Save = 0,
    Load = 1,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SlotMetadata {
    pub valid: bool,
    pub scenario_id: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuestionKind {
    Save,
    Overwrite,
    Load,
    Delete,
    MissingAppend,
}
impl QuestionKind {
    pub fn prompt(self) -> &'static str {
        match self {
            Self::Save => "セーブを行います。よろしいですか？",
            Self::Overwrite => "セーブを行います。@r内容は上書きされますが、よろしいですか？",
            Self::Load => "ロードします。よろしいですか？",
            Self::Delete => "このセーブデータを削除します。よろしいですか？",
            Self::MissingAppend => {
                "追加コンテンツがインストールされていないため、@rこのセーブデータをロードすることはできません。"
            }
        }
    }
    pub fn labels(self) -> &'static str {
        if self == Self::MissingAppend {
            "確認"
        } else {
            "はい|いいえ"
        }
    }
    pub fn selected(self) -> i32 {
        i32::from(self != Self::MissingAppend)
    }
    pub fn cancellable(self) -> bool {
        true
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Cue(&'static str),
    DisableInput,
    OpenQuestion(QuestionKind),
    SaveSelected(usize),
    DeleteSelected(usize),
    LoadSelected(usize),
    ReturnSource(u32),
    QueueAdv { flags: u32 },
}
#[derive(Clone, Debug)]
pub(crate) struct ViewState {
    pub mode: Mode,
    pub selected: i32,
    pub progress: f32,
    pub scroll: f32,
    pub close_flash: f32,
    pub row_flash: [f32; ROWS],
    pub scrollbar: Scrollbar,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    First,
    Opening,
    Input,
    Confirm,
    Close,
    Exit,
}
pub(crate) struct Task {
    pub kind: Kind,
    pub done: bool,
}
pub(crate) struct Model {
    pub mode: Mode,
    pub selected: i32,
    pub append_present: bool,
    pub progress: f32,
    pub scroll: HigurashiTweener,
    pub close_flash: HigurashiTweener,
    pub row_flash: [HigurashiTweener; ROWS],
    pub closing: bool,
    pub complete: bool,
    pub loaded: bool,
    pub request: u32,
    pub hit: i32,
    pub pointed_row: i32,
    pub speed: f32,
    pub slots: [SlotMetadata; ROWS],
    question: Option<QuestionKind>,
    pending: Vec<Kind>,
    actions: Vec<Action>,
}
fn control(clock: &mut HigurashiTweener, value: f32, ticks: f32, flags: u32) {
    clock
        .control(value, Ticks::from_f32(ticks), flags)
        .expect("Native SaveMenu tween flags");
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
        control(clock, 0.0, f32::from_bits(0x3e4ccccd), 0x40);
    }
}
fn clear_flash(clock: &mut HigurashiTweener) {
    clock.fast_forward_to(clock.value());
    control(clock, 0.0, f32::from_bits(0x3e4ccccd), 0x40);
}
impl Model {
    pub fn new(
        mode: Mode,
        remembered: u8,
        append_present: bool,
        slots: [SlotMetadata; ROWS],
    ) -> Result<Self> {
        ensure!(
            remembered < 100,
            "Manual save selection is outside the 100 native rows"
        );
        Ok(Self {
            mode,
            selected: i32::from(remembered),
            append_present,
            progress: 0.0,
            scroll: HigurashiTweener::new(
                (f32::from(remembered) * 206.0 - 297.0).clamp(0.0, MAX_SCROLL),
            ),
            close_flash: HigurashiTweener::new(0.0),
            row_flash: std::array::from_fn(|_| HigurashiTweener::new(0.0)),
            closing: false,
            complete: false,
            loaded: false,
            request: 0,
            hit: -1,
            pointed_row: -1,
            speed: 0.0,
            slots,
            question: None,
            pending: vec![],
            actions: vec![],
        })
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
    pub(crate) fn take_pending_tasks(&mut self) -> Vec<Task> {
        self.pending.drain(..).map(|kind| Task { kind, done: false }).collect()
    }
    pub(crate) fn view_state(&self, scrollbar: Scrollbar) -> ViewState {
        ViewState { mode:self.mode, selected:self.selected, progress:self.progress,
            scroll:self.scroll.value(), close_flash:self.close_flash.value(),
            row_flash:std::array::from_fn(|i|self.row_flash[i].value()), scrollbar }
    }
    pub fn set_slots(&mut self, slots: [SlotMetadata; ROWS]) {
        self.slots = slots;
    }
    fn reveal_selected(&mut self) {
        let top = (self.selected * 206 + 12) as f32;
        let current = self.scroll.value();
        let next = if top - current < 0.0 {
            top.max(0.0)
        } else if top - current + 200.0 > 818.0 {
            (top + 200.0 - 818.0).min(MAX_SCROLL)
        } else {
            current
        };
        control(&mut self.scroll, next, 4.0, 0);
    }
    pub fn poll_controls(&mut self, permission: bool, captured: bool, buttons: Buttons) -> bool {
        if !permission {
            self.actions.push(Action::DisableInput);
        } else if !captured && self.scroll.is_idle() {
            let r = buttons.repeat;
            let delta = if r & (1 << 4) != 0 {
                -1
            } else if r & (1 << 6) != 0 {
                1
            } else if r & (1 << 7) != 0 {
                -4
            } else if r & (1 << 5) != 0 {
                4
            } else if r & (1 << 10) != 0 {
                -10
            } else if r & (1 << 11) != 0 {
                10
            } else {
                0
            };
            let next = (self.selected + delta).clamp(0, 99);
            if next != self.selected {
                self.selected = next;
                self.actions.push(Action::Cue("move"));
                self.reveal_selected();
                self.speed = 0.0;
            } else if buttons.pressed & (1 << 24) != 0 {
                self.speed = 0.0;
                self.confirm();
            } else if buttons.pressed & (1 << 15) != 0 {
                if self.mode == Mode::Save {
                    self.speed = 0.0;
                    if self.slots[self.selected as usize].valid {
                        self.actions.push(Action::Cue("decide"));
                        self.open_question(QuestionKind::Delete);
                    } else {
                        self.actions.push(Action::Cue("error"));
                    }
                }
            } else if buttons.pressed & (1 << 25) != 0 {
                self.actions.push(Action::Cue("cancel"));
                self.close();
            }
        }
        !self.closing
    }
    fn confirm(&mut self) {
        flash(&mut self.row_flash[self.selected as usize], true);
        if self.mode == Mode::Save || self.slots[self.selected as usize].valid {
            self.actions.push(Action::Cue("decide"));
            self.pending.push(Kind::Confirm);
        } else {
            self.actions.push(Action::Cue("error"));
        }
    }
    fn open_question(&mut self, kind: QuestionKind) {
        self.question = Some(kind);
        self.request = match kind {
            QuestionKind::Save | QuestionKind::Overwrite => 1,
            QuestionKind::Load | QuestionKind::MissingAppend => 2,
            QuestionKind::Delete => 3,
        };
        self.actions.push(Action::OpenQuestion(kind));
    }
    fn close(&mut self) {
        flash(&mut self.close_flash, true);
        self.pending.push(Kind::Close);
    }
    fn begin_exit(&mut self) {
        self.closing = true;
        if !self.loaded {
            self.actions.push(Action::ReturnSource(0));
        }
        self.pending.push(Kind::Exit);
    }
    /// Native Save/Delete act on the initial callback; Load acts after the
    /// Question has faded out and calls its final callback.
    pub fn question_callback(&mut self, stage: u32, answer: i32) {
        let Some(kind) = self.question else { return };
        match kind {
            QuestionKind::Load => {
                if stage == 0 {
                    self.request = 0;
                } else if answer == 0 {
                    self.actions
                        .push(Action::LoadSelected(self.selected as usize));
                    self.loaded = true;
                    self.begin_exit();
                }
            }
            QuestionKind::MissingAppend => {
                if stage == 0 {
                    self.request = 0;
                }
            }
            QuestionKind::Save | QuestionKind::Overwrite | QuestionKind::Delete => {
                if stage == 0 {
                    if answer == 0 {
                        self.actions.push(if kind == QuestionKind::Delete {
                            Action::DeleteSelected(self.selected as usize)
                        } else {
                            Action::SaveSelected(self.selected as usize)
                        });
                    } else {
                        self.request = 0;
                    }
                }
            }
        }
        if stage != 0 {
            self.question = None;
        }
    }
    pub fn write_complete(&mut self) {
        self.request = 0;
    }
    fn keep_selection_visible(&mut self, scroll: f32) {
        let bottom = scroll + 818.0;
        loop {
            let top = (self.selected * 206 + 12) as f32;
            if scroll > top {
                self.selected += 1;
            } else if bottom < top + 200.0 {
                self.selected -= 1;
            } else {
                break;
            }
        }
        self.scroll.fast_forward_to(scroll);
    }
    pub(crate) fn advance_clocks(&mut self, delta: Ticks, atlas_present: bool, scrollbar: &mut Scrollbar) {
        if self.speed != 0.0 {
            let next = (self.scroll.value() - self.speed).clamp(0.0, MAX_SCROLL);
            self.keep_selection_visible(next);
            self.speed *= f32::from_bits(0x3f733333).powf(delta.as_f32());
            if self.speed.abs() < f32::from_bits(0x3dcccccd) {
                self.speed = 0.0;
            }
        }
        if atlas_present {
            *scrollbar = Scrollbar {
                position: self.scroll.value() as i32,
                visible: VIEW_HEIGHT,
                first: 0,
                end: DOCUMENT_HEIGHT,
            };
            self.scroll.update(Ticks::from_f32(1.0));
        }
        self.close_flash.update(delta);
        for flash in &mut self.row_flash {
            flash.update(delta);
        }
    }
    pub fn contact(&mut self, x: f32, y: f32) -> bool {
        self.pointed_row = -1;
        self.speed = 0.0;
        if (1764.0..1848.0).contains(&x) && (30.0..114.0).contains(&y) {
            self.hit = 1;
            return true;
        }
        self.hit = if (162.0..980.0).contains(&y) { -1 } else { 0 };
        if self.hit < 0 {
            if x <= 150.0 || x >= 1770.0 {
                return false;
            }
            let at = (self.scroll.value() + y - 162.0) as i32;
            let row = at / 206;
            if at - row * 206 <= 199 {
                self.pointed_row = row;
            }
        }
        true
    }
    fn safe_pointed(&self) -> bool {
        (0..100).contains(&self.pointed_row)
    }
    pub fn press_contact(&mut self) {
        if self.hit == 1 {
            flash(&mut self.close_flash, false);
        } else if self.hit < 0 && self.safe_pointed() {
            self.selected = self.pointed_row;
            self.reveal_selected();
        }
    }
    pub fn release_contact(&mut self) {
        if self.hit == 1 {
            self.actions.push(Action::Cue("decide"));
            self.close();
        } else if self.hit < 0 && self.safe_pointed() {
            self.selected = self.pointed_row;
            self.reveal_selected();
            self.confirm();
        }
    }
    pub fn long_press_contact(&mut self) {
        // b1a24 tests the old selection's validity before moving to the contact.
        if self.hit < 0
            && self.safe_pointed()
            && self.mode == Mode::Save
            && self.slots[self.selected as usize].valid
        {
            self.selected = self.pointed_row;
            self.reveal_selected();
            self.actions.push(Action::Cue("decide"));
            self.open_question(QuestionKind::Delete);
        }
    }
    pub fn drag_contact(&mut self, previous: f32, current: f32) {
        if self.hit >= 0 {
            clear_flash(&mut self.close_flash);
            self.hit = 0;
        } else {
            let scroll = (self.scroll.value() - (current - previous)).clamp(0.0, MAX_SCROLL);
            self.keep_selection_visible(scroll);
            self.pointed_row = -1;
        }
    }
    pub fn inertia(&mut self, speed: f32) {
        if self.hit < 0 && self.pointed_row < 0 {
            self.speed = speed;
        }
    }
    pub fn cancel_contact(&mut self) {
        if self.hit >= 0 {
            clear_flash(&mut self.close_flash);
            self.hit = 0;
        }
    }
    pub fn reset_contact(&mut self) {
        self.hit = -1;
        self.pointed_row = -1;
    }
}
impl InputAreaTarget for Model {
    fn probe(&mut self, p: Vec2) -> bool {
        self.contact(p.x, p.y)
    }
    fn reset(&mut self) {
        self.reset_contact();
    }
    fn press(&mut self, _: Vec2) {
        self.press_contact();
    }
    fn release(&mut self, _: Vec2) {
        self.release_contact();
    }
    fn long_press(&mut self, _: Vec2) {
        self.long_press_contact();
    }
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        self.drag_contact(previous.y, current.y);
    }
    fn gesture(&mut self, speed: Vec2) {
        self.inertia(speed.y);
    }
    fn cancel(&mut self) {
        self.cancel_contact();
    }
}
pub(crate) trait Services {
    fn input(&mut self, model: &mut Model, permission: bool);
    fn event(&mut self, action: Action) -> Result<()>;
    fn retire_first(&mut self);
}
struct Frame<'a> {
    model: &'a mut Model,
    status: u32,
    services: &'a mut dyn Services,
}
impl Task {
    /// Shared task body for the concrete mixed private/Question/SaveGame scope.
    pub(crate) fn poll_native(
        &mut self, model: &mut Model, status: u32, delta: Ticks,
        permission: bool, services: &mut dyn Services,
    ) -> Result<Vec<Task>> {
        let mut spawned = Vec::new();
        match self.kind {
            Kind::First => {
                if status == 2 {
                    model.closing = false;
                    spawned.push(Task {
                        kind: Kind::Opening,
                        done: false,
                    });
                    self.done = true;
                }
            }
            Kind::Opening => {
                model.progress += delta.as_f32() * f32::from_bits(0x3dcccccd);
                if model.progress >= 1.0 {
                    model.progress = 1.0;
                    spawned.push(Task {
                        kind: Kind::Input,
                        done: false,
                    });
                    self.done = true;
                }
            }
            Kind::Input => {
                services.input(model, permission);
                self.done = model.closing;
            }
            Kind::Confirm => {
                if model.row_flash[model.selected as usize].is_idle() {
                    let kind = if model.mode == Mode::Save {
                        if model.slots[model.selected as usize].valid {
                            QuestionKind::Overwrite
                        } else {
                            QuestionKind::Save
                        }
                    } else if !model.append_present
                        && model.slots[model.selected as usize].scenario_id >= 10000
                    {
                        QuestionKind::MissingAppend
                    } else {
                        QuestionKind::Load
                    };
                    model.open_question(kind);
                    self.done = true;
                }
            }
            Kind::Close => {
                if model.close_flash.is_idle() {
                    model.begin_exit();
                    self.done = true;
                }
            }
            Kind::Exit => {
                model.progress += delta.as_f32() * f32::from_bits(0xbdcccccd);
                if model.progress <= 0.0 {
                    model.progress = 0.0;
                    if model.loaded {
                        model.actions.push(Action::QueueAdv { flags: 0 });
                    } else {
                        model.actions.push(Action::ReturnSource(1));
                        model.complete = true;
                    }
                    self.done = true;
                }
            }
        }
        for kind in model.pending.drain(..) {
            spawned.push(Task { kind, done: false });
        }
        for action in model.take_actions() {
            services.event(action)?;
        }
        Ok(spawned)
    }
}
impl OwnedMode<Frame<'_>> for Task {
    fn completion(&mut self) -> u32 { u32::from(self.done) }
    fn retire(&mut self, frame: &mut Frame<'_>) {
        if self.kind == Kind::First { frame.services.retire_first(); }
    }
    fn poll(&mut self, delta: Ticks, permission: bool, frame: &mut Frame<'_>, pushes: &mut ModePush<'_, Self>) -> Result<u32> {
        for task in self.poll_native(frame.model, frame.status, delta, permission, frame.services)? {
            pushes.push(task);
        }
        Ok(0)
    }
}
pub(crate) struct Motion {
    pub model: Model,
    pub scrollbar: Scrollbar,
    tasks: ModeStack<Task>,
}
impl Motion {
    pub fn new(model: Model) -> Self {
        let mut tasks = ModeStack::default();
        tasks.push(Task {
            kind: Kind::First,
            done: false,
        });
        Self {
            model,
            scrollbar: Scrollbar {
                position: 0,
                visible: 1,
                first: 0,
                end: 1,
            },
            tasks,
        }
    }
    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks.iter()
    }
    pub fn view_state(&self) -> ViewState {
        ViewState {
            mode: self.model.mode,
            selected: self.model.selected,
            progress: self.model.progress,
            scroll: self.model.scroll.value(),
            close_flash: self.model.close_flash.value(),
            row_flash: std::array::from_fn(|i| self.model.row_flash[i].value()),
            scrollbar: self.scrollbar,
        }
    }
    pub fn advance(
        &mut self,
        status: u32,
        atlas_present: bool,
        permission: bool,
        delta: Ticks,
        services: &mut dyn Services,
    ) -> Result<()> {
        for kind in self.model.pending.drain(..) {
            self.tasks.push(Task { kind, done: false });
        }
        self.tasks.poll(
            delta,
            u32::from(permission),
            &mut Frame {
                model: &mut self.model,
                status,
                services,
            },
        )?;
        self.model
            .advance_clocks(delta, atlas_present, &mut self.scrollbar);
        Ok(())
    }
    pub fn retire(&mut self, services: &mut dyn Services) {
        self.tasks.clear_with(&mut Frame {
            model: &mut self.model,
            status: 0,
            services,
        });
    }
}
