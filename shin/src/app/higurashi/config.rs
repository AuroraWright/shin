//! Updated4dd80/4e1a8/51ad0 Config list, widgets and private task callbacks.
//! Concrete assets, Question, SaveGame and return owners live in the app adapter.
use crate::adv::{
    higurashi::{browser_controls::Scrollbar, input::Buttons},
    input_area::InputAreaTarget,
};
use crate::layer::higurashi_properties::HigurashiTweener;
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::{format::save::higurashi::Settings, time::Ticks};
pub(super) mod owner;
pub(super) mod view;
#[cfg(test)]
mod view_tests;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Control {
    Slider {
        subtype: u32,
        value: i32,
    },
    Buttons {
        labels: Vec<u32>,
        value: i32,
        cursor: i32,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RowState {
    pub y: i32,
    pub title_band: u32,
    pub focused: bool,
    pub kind: Control,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Heading {
    pub band: u32,
    pub y: i32,
    pub bottom: i32,
}
#[derive(Clone, Debug)]
pub(super) struct ViewState {
    pub ready: bool,
    pub progress: f32,
    pub scroll: f32,
    pub content_height: i32,
    pub frame: u32,
    pub close_flash: f32,
    pub reset_flash: f32,
    pub scrollbar: Scrollbar,
    pub rows: Vec<RowState>,
    pub headings: Vec<Heading>,
    pub dividers: Vec<i32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Cue(&'static str),
    DisableInput,
    ResetQuestion,
    Save { flag: u32 },
    ReturnSource(u32),
    SetPlatformFlag(bool),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    First,
    Open,
    Input,
    ResetWait,
    CloseWait,
    Close,
}
pub(super) struct Task {
    pub kind: Kind,
    pub done: bool,
}
struct Row {
    state: RowState,
    field: u8,
    pointed: i32,
    dragging: bool,
    drag_value: i32,
    drag_distance: f32,
}
pub(super) struct Model {
    pub settings: Settings,
    initial: Settings,
    pub flags: u32,
    pub append: bool,
    pub progress: f32,
    pub selected: usize,
    pub scroll: HigurashiTweener,
    pub close_flash: HigurashiTweener,
    pub reset_flash: HigurashiTweener,
    pub hit: i32,
    pub chrome: i32,
    pub speed: f32,
    pub frame: u32,
    pub closing: bool,
    pub complete: bool,
    rows: Vec<Row>,
    headings: Vec<Heading>,
    dividers: Vec<i32>,
    height: i32,
    pending: Vec<Kind>,
    actions: Vec<Action>,
}
fn control(t: &mut HigurashiTweener, value: f32, time: f32, flags: u32) {
    t.control(value, Ticks::from_f32(time), flags)
        .expect("Native Config tween flags");
}
fn flash(t: &mut HigurashiTweener, release: bool) {
    t.fast_forward_to(t.value());
    control(
        t,
        f32::from_bits(0x3f333333),
        f32::from_bits(0x3e4ccccd),
        0x40,
    );
    if release {
        control(t, 0., f32::from_bits(0x3e4ccccd), 0x40);
    }
}
fn clear_flash(t: &mut HigurashiTweener) {
    t.fast_forward_to(t.value());
    control(t, 0., f32::from_bits(0x3e4ccccd), 0x40);
}
fn value(s: &Settings, field: u8) -> i32 {
    let c = &s.common;
    match field {
        0 => c.v0_bgmvol.into(),
        1 => c.v1_sfxvol.into(),
        2 => c.v2_voicevol.into(),
        3 => c.v3_sysvol.into(),
        4 => c.v4_voicefocus.into(),
        5 => c.v5_voicepanapot.into(),
        9 => c.v9_msgspeed.into(),
        10 => c.v10_skipspeed.into(),
        11 => c.v11_disallowskipunread.into(),
        13 => c.v13_msgwinalpha.into(),
        14 => c.v14_showroutenavi.into(),
        16 => c.v16_showtoucheffect.into(),
        17 => c.v17_showscenetitle.into(),
        18 => c.v18_showsongtitle.into(),
        24 => s.extra_flag.into(),
        25 => s.extra_percentage.into(),
        _ => unreachable!("Native Config setting lane"),
    }
}
fn set(s: &mut Settings, field: u8, v: i32) {
    let c = &mut s.common;
    match field {
        0 => c.v0_bgmvol = v as u8,
        1 => c.v1_sfxvol = v as u8,
        2 => c.v2_voicevol = v as u8,
        3 => c.v3_sysvol = v as u8,
        4 => c.v4_voicefocus = v != 0,
        5 => c.v5_voicepanapot = v != 0,
        9 => c.v9_msgspeed = v as u8,
        10 => c.v10_skipspeed = v as u8,
        11 => c.v11_disallowskipunread = v != 0,
        13 => c.v13_msgwinalpha = v as u8,
        14 => c.v14_showroutenavi = v != 0,
        16 => c.v16_showtoucheffect = v != 0,
        17 => c.v17_showscenetitle = v != 0,
        18 => c.v18_showsongtitle = v != 0,
        24 => s.extra_flag = v != 0,
        25 => s.extra_percentage = v as u8,
        _ => unreachable!("Native Config setting lane"),
    }
}
impl Model {
    pub fn new(settings: Settings, flags: u32, append: bool) -> Result<Self> {
        ensure!(
            flags <= 3,
            "Config constructor flags outside the checked native0..3 boundary"
        );
        let mut this = Self {
            initial: settings.clone(),
            settings,
            flags,
            append,
            progress: 0.,
            selected: 0,
            scroll: HigurashiTweener::new(0.),
            close_flash: HigurashiTweener::new(0.),
            reset_flash: HigurashiTweener::new(0.),
            hit: -1,
            chrome: -1,
            speed: 0.,
            frame: 0,
            closing: false,
            complete: false,
            rows: vec![],
            headings: vec![],
            dividers: vec![],
            height: 0,
            pending: vec![],
            actions: vec![],
        };
        this.rebuild();
        Ok(this)
    }
    fn rebuild(&mut self) {
        self.rows.clear();
        self.headings.clear();
        self.dividers.clear();
        let mut y = 0;
        let mut section = |this: &mut Self, band: u32, specs: &[(u32, u8, i32)]| {
            let start = y;
            for &(title, field, subtype) in specs {
                let v = value(&this.settings, field);
                let kind = if subtype >= 0 {
                    Control::Slider {
                        subtype: subtype as u32,
                        value: v,
                    }
                } else {
                    Control::Buttons {
                        labels: if subtype == -1 {
                            vec![1, 0]
                        } else if subtype == -2 {
                            vec![5, 4]
                        } else {
                            vec![2, 3]
                        },
                        value: v,
                        cursor: -1,
                    }
                };
                this.rows.push(Row {
                    state: RowState {
                        y,
                        title_band: title,
                        focused: false,
                        kind,
                    },
                    field,
                    pointed: -1,
                    dragging: false,
                    drag_value: 0,
                    drag_distance: 0.,
                });
                this.dividers.push(y + 100);
                y += 106;
            }
            this.dividers.pop();
            this.headings.push(Heading {
                band,
                y: start,
                bottom: y - 6,
            });
            y += 54;
        };
        let mut sound = vec![(0, 3, 0), (1, 0, 0), (2, 1, 0), (3, 2, 0), (4, 4, -1)];
        if self.flags <= 1 {
            sound.push((18, 5, -1));
        }
        section(self, 0, &sound);
        let mut text = vec![(8, 9, 2), (11, 11, -1)];
        if self.flags <= 1 {
            text.push((15, 13, 1));
        }
        section(self, 1, &text);
        let mut other = vec![];
        if self.flags <= 1 {
            other.push((12, 14, -2));
        }
        other.push((10, 10, 2));
        if self.flags <= 1 {
            other.push((16, 18, -2));
        }
        other.extend([(19, 17, -2), (17, 16, -2)]);
        section(self, 2, &other);
        if self.append && (self.flags | 2) == 2 {
            section(self, 3, &[(6, 24, -3), (7, 25, 3)]);
        }
        self.height = self.headings.last().unwrap().bottom;
        self.selected = self.selected.min(self.rows.len() - 1);
        self.focus(self.selected, true);
    }
    fn focus(&mut self, index: usize, yes: bool) {
        let row = &mut self.rows[index];
        if yes && !row.state.focused {
            if let Control::Buttons { value, cursor, .. } = &mut row.state.kind {
                *cursor = *value;
            }
        }
        row.state.focused = yes;
    }
    pub fn select(&mut self, index: usize) -> Result<()> {
        ensure!(
            index < self.rows.len(),
            "Config row outside checked native list"
        );
        if index != self.selected {
            self.focus(self.selected, false);
            self.selected = index;
            self.focus(index, true);
        }
        let top = self.rows[index].state.y as f32;
        let cur = self.scroll.value();
        let next = if top - cur < 0. {
            top.max(0.)
        } else if top - cur + 100. > 770. {
            (top + 100. - 770.).min((self.height - 770) as f32)
        } else {
            cur
        };
        control(&mut self.scroll, next, 4., 0);
        Ok(())
    }
    fn cue(&mut self, s: &'static str) {
        self.actions.push(Action::Cue(s));
    }
    fn setting(&mut self, index: usize, v: i32) {
        let field = self.rows[index].field;
        set(&mut self.settings, field, v);
        if field == 16 {
            self.actions.push(Action::SetPlatformFlag(v != 0));
        }
    }
    fn row_controller(&mut self, index: usize, repeat: u32) -> bool {
        let mut update = None;
        let row = &mut self.rows[index];
        match &mut row.state.kind {
            Control::Slider { value, .. } => {
                let old = *value;
                let next = if repeat & (1 << 7) != 0 {
                    old.max(5) - 5
                } else if repeat & (1 << 5) != 0 {
                    old.min(95) + 5
                } else {
                    old
                };
                if next != old {
                    *value = next;
                    update = Some(next);
                }
            }
            Control::Buttons {
                labels,
                value,
                cursor,
            } => {
                let old = *value;
                let next = if repeat & (1 << 7) != 0 {
                    (old - 1).max(0)
                } else if repeat & (1 << 5) != 0 {
                    (old + 1).min(labels.len() as i32 - 1)
                } else {
                    old
                };
                if next != old {
                    *value = next;
                    *cursor = next;
                    update = Some(next);
                }
            }
        }
        if let Some(v) = update {
            self.setting(index, v);
            self.cue("move");
            true
        } else {
            false
        }
    }
    pub fn poll_controls(&mut self, permission: bool, captured: bool, buttons: Buttons) -> bool {
        if !permission {
            self.actions.push(Action::DisableInput);
        } else if !captured && self.scroll.is_idle() {
            let old = self.selected;
            let next = if buttons.repeat & (1 << 4) != 0 {
                old.saturating_sub(1)
            } else if buttons.repeat & (1 << 6) != 0 {
                (old + 1).min(self.rows.len() - 1)
            } else {
                old
            };
            if next != old {
                self.cue("move");
                self.speed = 0.;
                self.select(next).unwrap();
            } else if buttons.pressed & (1 << 15) != 0 {
                self.cue("decide");
                self.speed = 0.;
                self.reset();
            } else if buttons.pressed & (1 << 25) != 0 {
                self.cue("cancel");
                self.close();
            } else if self.row_controller(old, buttons.repeat) {
                self.speed = 0.;
            }
        }
        !self.closing
    }
    fn reset(&mut self) {
        flash(&mut self.reset_flash, true);
        self.pending.push(Kind::ResetWait);
    }
    fn close(&mut self) {
        self.closing = true;
        flash(&mut self.close_flash, true);
        self.pending.push(Kind::CloseWait);
    }
    pub fn reset_answer(&mut self, stage: u32, answer: i32) {
        if stage == 0 && answer == 0 {
            self.settings = Settings::default();
            self.rebuild();
            self.actions.push(Action::SetPlatformFlag(
                self.settings.common.v16_showtoucheffect,
            ));
        }
    }
    pub fn save_complete(&mut self) {
        self.begin_close();
    }
    fn begin_close(&mut self) {
        self.actions.push(Action::ReturnSource(0));
        self.pending.push(Kind::Close);
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
    pub fn take_pending_tasks(&mut self) -> Vec<Task> {
        self.pending
            .drain(..)
            .map(|kind| Task { kind, done: false })
            .collect()
    }
    fn same_settings(&self) -> bool {
        self.settings.common.serialized_eq(&self.initial.common)
            && self.settings.extra_flag == self.initial.extra_flag
            && (self.settings.extra_percentage & 127) == (self.initial.extra_percentage & 127)
    }
    fn keep_visible(&mut self, scroll: f32) {
        let mut index = self.selected;
        while index + 1 < self.rows.len() && scroll > self.rows[index].state.y as f32 {
            index += 1;
        }
        while index > 0 && scroll + 770. < (self.rows[index].state.y + 100) as f32 {
            index -= 1;
        }
        if index != self.selected {
            self.focus(self.selected, false);
            self.selected = index;
            self.focus(index, true);
        }
        self.scroll.fast_forward_to(scroll);
    }
    pub fn advance_clocks(&mut self, delta: Ticks, ready: bool, scrollbar: &mut Scrollbar) {
        if self.speed != 0. {
            let next = (self.scroll.value() - self.speed)
                .min((self.height - 770) as f32)
                .max(0.);
            self.keep_visible(next);
            self.speed *= f32::from_bits(0x3f733333).powf(delta.as_f32());
            if self.speed.abs() < f32::from_bits(0x3dcccccd) {
                self.speed = 0.;
            }
        }
        if ready {
            *scrollbar = Scrollbar {
                position: self.scroll.value() as i32,
                visible: 770,
                first: 0,
                end: self.height,
            };
            self.scroll.update(Ticks::from_f32(1.));
        }
        self.close_flash.update(delta);
        self.reset_flash.update(delta);
        self.frame = self.frame.wrapping_add(1);
    }
    pub fn view_state(&self, ready: bool, scrollbar: Scrollbar) -> ViewState {
        ViewState {
            ready,
            progress: self.progress,
            scroll: self.scroll.value(),
            content_height: self.height,
            frame: self.frame,
            close_flash: self.close_flash.value(),
            reset_flash: self.reset_flash.value(),
            scrollbar,
            rows: self.rows.iter().map(|r| r.state.clone()).collect(),
            headings: self.headings.clone(),
            dividers: self.dividers.clone(),
        }
    }
    pub fn contact(&mut self, x: f32, y: f32) -> bool {
        self.hit = -1;
        if (30.0..114.0).contains(&y) {
            if (1764.0..1848.0).contains(&x) {
                self.chrome = 1;
                return true;
            }
            if (1446.0..1752.0).contains(&x) {
                self.chrome = 2;
                return true;
            }
        }
        self.chrome = if (210.0..980.0).contains(&y) { -1 } else { 0 };
        if self.chrome < 0 {
            if x <= 150. || x >= 1770. {
                return false;
            }
            let local = (self.scroll.value() - 210.) + y;
            if let Some(index) = self
                .rows
                .iter()
                .position(|r| local >= r.state.y as f32 && local < (r.state.y + 100) as f32)
            {
                self.hit = index as i32;
                let row = &mut self.rows[index];
                match &row.state.kind {
                    Control::Slider { .. } => row.dragging = false,
                    Control::Buttons { labels, .. } => {
                        for i in (0..labels.len()).rev() {
                            let left = 1734. - 252. * (labels.len() - i) as f32 + 12.;
                            if x >= left && x < left + 240. {
                                row.pointed = i as i32;
                                break;
                            }
                        }
                    }
                }
            }
        }
        true
    }
    pub fn reset_contact(&mut self) {
        if self.hit >= 0 {
            self.rows[self.hit as usize].pointed = -1;
        }
        self.chrome = -1;
        self.hit = -1;
    }
    pub fn press_contact(&mut self, p: Vec2) {
        if self.chrome == 1 {
            flash(&mut self.close_flash, false);
        } else if self.chrome == 2 {
            flash(&mut self.reset_flash, false);
        } else if self.chrome < 0 && self.hit >= 0 {
            let i = self.hit as usize;
            self.select(i).unwrap();
            let y = (self.scroll.value() - 210.) + p.y;
            let row = &mut self.rows[i];
            match &mut row.state.kind {
                Control::Buttons { cursor, .. } => {
                    if row.pointed >= 0 {
                        *cursor = row.pointed;
                    }
                }
                Control::Slider { value, .. } => {
                    let center = *value as f32 * 6. + 1092.;
                    if y >= row.state.y as f32
                        && y < (row.state.y + 100) as f32
                        && p.x >= center - 48.
                        && p.x < center + 48.
                    {
                        row.drag_value = *value;
                        row.drag_distance = 0.;
                        row.dragging = true;
                    }
                }
            }
        }
    }
    pub fn release_contact(&mut self, _p: Vec2) {
        if self.chrome == 1 {
            self.cue("decide");
            self.close();
        } else if self.chrome == 2 {
            self.cue("decide");
            self.reset();
        } else if self.chrome < 0 && self.hit >= 0 {
            let i = self.hit as usize;
            self.select(i).unwrap();
            let pointed = self.rows[i].pointed;
            let mut v = None;
            if let Control::Buttons { value, cursor, .. } = &mut self.rows[i].state.kind {
                if pointed >= 0 {
                    *value = pointed;
                    *cursor = *value;
                    v = Some(*value);
                }
            }
            if let Some(v) = v {
                self.setting(i, v);
                self.cue("decide");
            }
        }
    }
    fn row_drag(&mut self, i: usize, previous: Vec2, current: Vec2) -> bool {
        let row = &mut self.rows[i];
        let Control::Slider { value, .. } = &mut row.state.kind else {
            if let Control::Buttons { value, cursor, .. } = &mut row.state.kind {
                *cursor = *value;
            }
            row.pointed = -1;
            return false;
        };
        let dx = current.x - previous.x;
        if !row.dragging {
            let center = *value as f32 * 6. + 1092.;
            if previous.y < row.state.y as f32
                || previous.y >= (row.state.y + 100) as f32
                || previous.x < center - 48.
                || previous.x >= center + 48.
            {
                return false;
            }
            let angle =
                ((current.y - previous.y).atan2(dx) / f32::from_bits(0x40490fd8) * 180.).abs();
            if (40.0..=140.0).contains(&angle) {
                return false;
            }
            row.drag_value = *value;
            row.drag_distance = 0.;
            row.dragging = true;
        }
        row.drag_distance += dx;
        let next = (row.drag_value + (row.drag_distance * f32::from_bits(0x3e2aaaab)) as i32)
            .clamp(0, 100);
        if *value != next {
            *value = next;
            self.setting(i, next);
        }
        true
    }
    pub fn drag_contact(&mut self, previous: Vec2, current: Vec2) {
        if self.chrome >= 0 {
            clear_flash(&mut self.close_flash);
            clear_flash(&mut self.reset_flash);
            self.chrome = 0;
            return;
        }
        if self.hit >= 0 {
            let i = self.hit as usize;
            let off = self.scroll.value() - 210.;
            if self.row_drag(
                i,
                Vec2::new(previous.x, previous.y + off),
                Vec2::new(current.x, current.y + off),
            ) {
                self.select(i).unwrap();
                return;
            }
            self.rows[i].pointed = -1;
            self.hit = -1;
        }
        let next = (self.scroll.value() - (current.y - previous.y))
            .min((self.height - 770) as f32)
            .max(0.);
        self.keep_visible(next);
    }
    pub fn inertia(&mut self, speed: f32) {
        if self.chrome < 0 && self.hit < 0 {
            self.speed = speed;
        }
    }
    pub fn cancel_contact(&mut self) {
        if self.chrome >= 0 {
            clear_flash(&mut self.close_flash);
            clear_flash(&mut self.reset_flash);
            self.chrome = 0;
        }
    }
}
impl InputAreaTarget for Model {
    fn probe(&mut self, p: Vec2) -> bool {
        self.contact(p.x, p.y)
    }
    fn reset(&mut self) {
        self.reset_contact();
    }
    fn press(&mut self, p: Vec2) {
        self.press_contact(p);
    }
    fn release(&mut self, p: Vec2) {
        self.release_contact(p);
    }
    fn long_press(&mut self, _: Vec2) {
        // The Config owner retains InputArea's default-disabled long press.
    }
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        self.drag_contact(previous, current);
    }
    fn gesture(&mut self, speed: Vec2) {
        self.inertia(speed.y);
    }
    fn cancel(&mut self) {
        self.cancel_contact();
    }
}
pub(super) trait Services {
    fn input(&mut self, model: &mut Model, permission: bool);
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
        let mut spawned = vec![];
        match self.kind {
            Kind::First => {
                if status == 2 {
                    model.closing = false;
                    spawned.push(Task {
                        kind: Kind::Open,
                        done: false,
                    });
                    self.done = true;
                }
            }
            Kind::Open => {
                model.progress += delta.as_f32() * f32::from_bits(0x3dcccccd);
                if model.progress >= 1. {
                    model.progress = 1.;
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
            Kind::ResetWait => {
                if model.reset_flash.is_idle() {
                    model.actions.push(Action::ResetQuestion);
                    self.done = true;
                }
            }
            Kind::CloseWait => {
                if model.close_flash.is_idle() {
                    if model.same_settings() {
                        model.begin_close();
                    } else {
                        model.actions.push(Action::Save { flag: 1 });
                    }
                    self.done = true;
                }
            }
            Kind::Close => {
                model.progress += delta.as_f32() * f32::from_bits(0xbdcccccd);
                if model.progress <= 0. {
                    model.progress = 0.;
                    model.actions.push(Action::ReturnSource(1));
                    model.complete = true;
                    self.done = true;
                }
            }
        }
        spawned.extend(model.take_pending_tasks());
        for action in model.take_actions() {
            services.event(action)?;
        }
        Ok(spawned)
    }
}

#[cfg(test)]
mod model_tests;
