//! Updated BGMMode2f2e0/2fe2c/30c44. Pure retained model; the future concrete
//! owner must keep the complete outer Globals, worker, capture and audio owners.
//! Atlas publication and executor completion are deliberately separate inputs.
use crate::{
    adv::{
        higurashi::{
            browser_controls::Scrollbar, input::Buttons, reconstruction_mode::OverlayClock,
        },
        input_area::InputAreaTarget,
    },
    layer::higurashi_properties::HigurashiTweener,
};
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::{format::scenario::info::MusicBoxInfoItem, time::Ticks};

#[cfg(test)]
mod model_tests;

/// 321d4 loads/publishes these in order. The outer owner must capture the
/// complete composed source before submitting that First worker.
pub(super) const ATLAS_PATH: &str = "/bgmmode.txa";
pub(super) const MASK_PATH: &str = "/default.msk";
pub(super) const INPUT_EXTENT: (u16, u16) = (1920, 1080);
pub(super) mod opening;
pub(super) mod owner;
pub(super) mod view;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Action {
    Cue(&'static str),
    DisableInput,
    PlayBgm {
        id: u16,
        once: u16,
        fade: u32,
        gain: f32,
    },
    StopBgm(u32),
    /// Original ccb10 request after polling a previously nonnegative marker.
    PlatformByte,
    Achievement(u32),
    /// e2a40 builds the queued Title factory; the platform owns its invocation.
    ReturnTitle {
        selected: i32,
        mode: i32,
    },
}
pub(super) struct Row {
    pub catalog_index: usize,
    pub bgm: u16,
    pub once: u16,
    pub name: u16,
    pub unlocked: bool,
    pub append: bool,
    pub flash: HigurashiTweener,
}
pub(super) struct Model {
    pub append: bool,
    pub rows: Vec<Row>,
    pub document_height: i32,
    pub selected: i32,
    pub playing: i32,
    pub closing: bool,
    pub elapsed: f32,
    pub region: i32,
    pub hover: i32,
    pub contact: i32,
    pub velocity: f32,
    pub scroll: HigurashiTweener,
    pub close_flash: HigurashiTweener,
    pub scrollbar: Scrollbar,
    pub completion_text: String,
    actions: Vec<Action>,
    pending: Vec<Task>,
}
fn control(t: &mut HigurashiTweener, value: f32, time: f32, flags: u32) {
    t.control(value, Ticks::from_f32(time), flags)
        .expect("Original Music control flags");
}
fn clear(t: &mut HigurashiTweener) {
    t.fast_forward_to(t.value());
}
fn flash(t: &mut HigurashiTweener, release: bool) {
    clear(t);
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
    clear(t);
    control(t, 0., f32::from_bits(0x3e4ccccd), 0x40);
}
impl Model {
    /// 2f2e0 snapshots packed global+88 unlocks. No global vector, runtime row,
    /// history, date or source GameData is replaced or truncated by this model.
    pub fn new(items: &[MusicBoxInfoItem], unlocked: &[u32], append: bool) -> Result<Self> {
        let mut rows = Vec::new();
        let (mut count, mut acquired) = (0u32, 0u32);
        for (catalog_index, item) in items.iter().enumerate() {
            let bgm = item.bgm_id.into_i32() as u16;
            let extra = bgm > 158;
            if extra && !append {
                continue;
            }
            let available = unlocked
                .get(usize::from(bgm) / 32)
                .is_some_and(|w| w & (1 << (bgm % 32)) != 0);
            if !extra {
                count += 1;
                acquired += u32::from(available);
            }
            rows.push(Row {
                catalog_index,
                bgm,
                once: item.once_flag,
                name: if available { item.name_index } else { 0 },
                unlocked: available,
                append: extra,
                flash: HigurashiTweener::new(0.),
            });
        }
        // The shipped catalog is nonempty and has non-append rows. Explicit
        // desktop malformed-catalog rejection avoids native division/OOB UB.
        ensure!(
            !rows.is_empty() && count != 0,
            "Music catalog has no native base entries"
        );
        let tenths = acquired * 1000 / count;
        let percent = if tenths >= 1000 {
            "100".to_owned()
        } else {
            format!("{}.{}", tenths / 10, tenths % 10)
        };
        let actions = if tenths >= 1000 {
            vec![Action::Achievement(34)]
        } else {
            vec![]
        };
        Ok(Self {
            append,
            document_height: (rows.len().div_ceil(2) * 92) as i32,
            rows,
            selected: 0,
            playing: -1,
            closing: false,
            elapsed: 0.,
            region: -1,
            hover: -1,
            contact: -1,
            velocity: 0.,
            scroll: HigurashiTweener::new(0.),
            close_flash: HigurashiTweener::new(0.),
            scrollbar: Scrollbar::default(),
            completion_text: format!("達成率:{percent}％"),
            actions,
            pending: vec![],
        })
    }
    pub fn take_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.actions)
    }
    pub fn take_pending_tasks(&mut self) -> Vec<Task> {
        std::mem::take(&mut self.pending)
    }
    fn reveal(&mut self) {
        let top = ((self.selected / 2) * 92) as f32;
        let current = self.scroll.value();
        let relative = top - current;
        let target = if relative < 0. {
            top.max(0.)
        } else if relative + 92. > 818. {
            // Original30e70 clamps against docHeight, not docHeight-818.
            ((top + 92.) + -818.).min(self.document_height as f32)
        } else {
            current
        };
        control(&mut self.scroll, target, 4., 0);
    }
    fn move_scroll(&mut self, proposed: f32, only_if_changed: bool) {
        let value = proposed.min((self.document_height - 818) as f32).max(0.);
        if !only_if_changed || value != self.scroll.value() {
            // Native corrects the selected pair even when its column stays
            // unchanged. The supplied158-entry catalog has complete pairs.
            while value > ((self.selected / 2) * 92) as f32 {
                self.selected += 2;
            }
            while value + 818. < (((self.selected / 2) * 92) as f32 + 92.) {
                self.selected -= 2;
            }
            self.scroll.fast_forward_to(value);
        }
    }
    fn play(&mut self) {
        let row = &mut self.rows[self.selected as usize];
        flash(&mut row.flash, true);
        if row.unlocked {
            self.actions.push(Action::PlayBgm {
                id: row.bgm,
                once: row.once,
                fade: 0,
                gain: 1.,
            });
            self.playing = self.selected;
            self.velocity = 0.;
        } else {
            self.actions.push(Action::Cue("error"));
        }
    }
    fn close(&mut self) {
        self.closing = true;
        flash(&mut self.close_flash, true);
        self.velocity = 0.;
        self.pending.push(Task::Close { done: false });
    }
    /// InputArea processing is performed by the concrete service before this
    /// controller decision. False permission disables contacts and reads no keys.
    pub fn controller(&mut self, permission: bool, captured: bool, buttons: Buttons) -> bool {
        if !permission {
            self.actions.push(Action::DisableInput);
        } else if !captured && self.scroll.is_idle() {
            let p = buttons.pressed;
            let r = buttons.repeat;
            if p & (1 << 25) != 0 {
                self.actions.push(Action::Cue("cancel"));
                self.close();
            } else if p & (1 << 24) != 0 {
                self.play();
            } else if p & (1 << 15) != 0 {
                if self.playing >= 0 {
                    self.actions.push(Action::StopBgm(30));
                    self.playing = -1;
                }
            } else {
                let dx = if r & (1 << 7) != 0 {
                    -1
                } else {
                    i32::from(r & (1 << 5) != 0)
                };
                let dy = if r & (1 << 4) != 0 {
                    -1
                } else if r & (1 << 6) != 0 {
                    1
                } else if r & (1 << 10) != 0 {
                    -8
                } else if r & (1 << 11) != 0 {
                    8
                } else {
                    0
                };
                if dx | dy != 0 {
                    let column = (self.selected % 2 + dx).clamp(0, 1);
                    let row =
                        (self.selected / 2 + dy).clamp(0, self.rows.len().div_ceil(2) as i32 - 1);
                    let selected = (row * 2 + column).min(self.rows.len() as i32 - 1);
                    if selected != self.selected {
                        self.actions.push(Action::Cue("move"));
                        self.velocity = 0.;
                        self.selected = selected;
                        self.reveal();
                    }
                }
            }
        }
        !self.closing
    }
    /// Parent2fe2c tail: atlas controls scrollbar visibility independently of
    /// executor status. Sample scrollbar BEFORE advancing the scroll clock.
    pub fn advance_clocks(&mut self, delta: Ticks, atlas: bool, bgm_status: u32) {
        if self.velocity != 0. {
            self.move_scroll(self.scroll.value() - self.velocity, true);
            self.velocity *= f32::from_bits(0x3f733333).powf(delta.as_f32());
            if self.velocity.abs() < f32::from_bits(0x3dcccccd) {
                self.velocity = 0.;
            }
        }
        if atlas && self.document_height >= 818 {
            self.scrollbar = Scrollbar {
                position: self.scroll.value() as i32,
                visible: 818,
                first: 0,
                end: self.document_height,
            };
        }
        self.scroll.update(delta);
        if self.playing >= 0 {
            if bgm_status & 2 == 0 {
                self.playing = -1;
            }
            self.actions.push(Action::PlatformByte);
        }
        self.close_flash.update(delta);
        for row in &mut self.rows {
            row.flash.update(delta);
        }
        self.elapsed += delta.as_f32() * f32::from_bits(0x3c888889);
    }
}
impl InputAreaTarget for Model {
    fn probe(&mut self, p: Vec2) -> bool {
        self.hover = -1;
        self.velocity = 0.;
        self.region = if p.y >= 30. && p.y < 114. && p.x >= 1764. && p.x < 1848. {
            1
        } else if p.y >= 162. && p.y < 980. {
            -1
        } else {
            0
        };
        if self.region < 0 {
            let column = if p.x >= 150. && p.x < 950. {
                0
            } else if p.x >= 970. && p.x < 1770. {
                1
            } else {
                -1
            };
            if column >= 0 {
                //31200: float scroll-162+y then signed truncation/division.
                let y = ((self.scroll.value() + -162.) + p.y) as i32;
                let row = y / 92;
                let remainder = y % 92;
                let index = row * 2 + column;
                if (2..=89).contains(&remainder) && index >= 0 && (index as usize) < self.rows.len()
                {
                    self.hover = index;
                }
            }
        }
        true
    }
    fn reset(&mut self) {
        self.region = -1;
        self.hover = -1;
        self.contact = -1;
    }
    fn press(&mut self, _: Vec2) {
        if self.region == 1 {
            flash(&mut self.close_flash, false);
        } else if self.region < 0 && self.hover >= 0 && (self.hover as usize) < self.rows.len() {
            self.selected = self.hover;
            clear(&mut self.scroll);
            self.reveal();
        }
    }
    fn release(&mut self, _: Vec2) {
        if self.region == 1 {
            self.actions.push(Action::Cue("decide"));
            self.close();
        } else if self.region < 0 && self.hover >= 0 {
            if (self.hover as usize) < self.rows.len() {
                self.selected = self.hover;
                clear(&mut self.scroll);
                self.reveal();
            }
            self.play();
        }
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        if self.region >= 0 {
            clear_flash(&mut self.close_flash);
            self.region = 0;
        } else if self.hover >= 0 {
            self.move_scroll(self.scroll.value() - (current.y - previous.y), false);
        } else {
            self.hover = -1;
            self.contact = -1;
        }
    }
    fn gesture(&mut self, velocity: Vec2) {
        if self.region < 0 && self.hover >= 0 {
            self.velocity = velocity.y;
        }
    }
    fn cancel(&mut self) {
        if self.region >= 0 {
            clear_flash(&mut self.close_flash);
            self.region = 0;
        }
    }
}

/// Values are native private-callable owners, not platform scenes. A concrete
/// owner polls them through ModeStack and draws its retained forward snapshot.
pub(super) enum Task {
    First { phase: i32, overlay: OverlayClock },
    Input { done: bool },
    Close { done: bool },
}
pub(super) trait Services {
    fn job_status(&mut self) -> Result<u32>;
    fn mask_published(&self) -> bool;
    /// Process real InputArea and invoke Model::controller; return its result.
    fn input(&mut self, model: &mut Model, permission: bool) -> bool;
    fn event(&mut self, action: Action);
}
impl Task {
    pub fn first(capture_present: bool) -> Self {
        Self::First {
            phase: 0,
            overlay: OverlayClock::captured(capture_present),
        }
    }
    pub fn complete(&self) -> bool {
        match self {
            Self::First { phase, .. } => *phase < 0,
            Self::Input { done } | Self::Close { done } => *done,
        }
    }
    /// Returns append-only tasks for the same native scope. Newly pushed tasks
    /// start on the following scope poll; overlay advancement follows phase tests.
    pub fn poll_native(
        &mut self,
        model: &mut Model,
        delta: Ticks,
        permission: bool,
        services: &mut impl Services,
    ) -> Result<Vec<Task>> {
        let mut pushes = vec![];
        match self {
            Self::First { phase, overlay } => {
                if *phase == 0 && services.job_status()? == 2 {
                    overlay.masked(30, f32::from_bits(0x3e99999a), 0, services.mask_published());
                    services.event(Action::StopBgm(60));
                    *phase = 1;
                } else if *phase == 1 && !overlay.busy() {
                    model.closing = false;
                    pushes.push(Self::Input { done: false });
                    *phase = -1;
                }
                overlay.advance(delta);
            }
            Self::Input { done } => {
                *done = !services.input(model, permission);
            }
            Self::Close { done } => {
                if model.close_flash.is_idle() {
                    services.event(Action::StopBgm(30));
                    services.event(Action::ReturnTitle {
                        selected: 16,
                        mode: -1,
                    });
                    *done = true;
                }
            }
        }
        pushes.extend(model.take_pending_tasks());
        for event in model.take_actions() {
            services.event(event);
        }
        Ok(pushes)
    }
}
