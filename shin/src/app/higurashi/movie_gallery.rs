//! Updated MovieMode9356c/93d08/94754 and its private Viewer95824.
//! Pure retained tasks only: decoded media, First publication/capture, actual
//! InputAreas and complete outer Globals remain with fallible host services.
use crate::{
    adv::higurashi::{
        browser_controls::Scrollbar,
        input::Buttons,
        mode_stack::{Mode, ModePush, ModeStack},
        reconstruction_mode::OverlayClock,
    },
    layer::higurashi_properties::HigurashiTweener,
};
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::{format::scenario::higurashi::MovieInfoItem, time::Ticks};

#[cfg(test)]
mod model_tests;
pub(super) mod opening;
pub(super) mod owner;
pub(super) mod view;

pub(super) const ATLAS_PATH: &str = "/moviemode.txa";
pub(super) const MASK_PATH: &str = "/default.msk";
pub(super) const ROSTER: [usize; 12] = [9, 6, 0, 1, 7, 8, 2, 3, 4, 5, 10, 11];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InputTarget {
    Grid,
    Viewer(u64),
}
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Action {
    Cue(&'static str),
    DisableInput(InputTarget),
    Achievement(u32),
    StopBgm(u32),
    ReturnTitle {
        selected: i32,
        mode: i32,
    },
    /// Actualbcc84 type0 filename/open57f00 then92ca4(source1). The host
    /// resolves the real metadata and initializes the standalone Player here.
    OpenPlayer {
        viewer: u64,
        movie_id: u16,
        source_category: u32,
    },
    StartPlayer(u64),
    UpdatePlayer(u64),
    PausePlayer(u64),
    StopPlayer(u64),
    ReleasePlayer(u64),
    PlayerGain {
        viewer: u64,
        value: f32,
    },
    PlayerColor {
        viewer: u64,
        value: [f32; 4],
    },
    PlatformByte,
    JoinFirst,
    #[cfg_attr(not(test), expect(dead_code, reason = "Native model retirement action, exercised by model tests."))]
    ReleaseAtlas,
}
#[derive(Clone, Debug)]
pub(super) struct RowState {
    #[allow(dead_code, reason = "Preserve source roster identity in native row snapshots.")]
    pub catalog_index: usize,
    pub movie_id: u16,
    pub append: bool,
    pub locked: bool,
    pub x: f32,
    pub y: f32,
    pub focus: f32,
}
#[derive(Clone, Debug)]
pub(super) struct ViewState {
    pub ready: bool,
    #[allow(dead_code, reason = "Preserve the native append flag in model and view snapshots.")]
    pub append: bool,
    #[allow(dead_code, reason = "Preserve the native input frame in view snapshots.")]
    pub frame: u32,
    pub rows: Vec<RowState>,
    pub document_height: i32,
    pub selected: usize,
    pub scroll: f32,
    pub close_flash: f32,
    pub scrollbar: Scrollbar,
    pub completion: String,
}
pub(super) struct Model {
    pub append: bool,
    pub rows: Vec<RowState>,
    pub document_height: i32,
    pub x: i32,
    pub y: i32,
    pub closing: bool,
    pub frame: u32,
    pub region: i32,
    pub hover: i32,
    pub contact: i32,
    pub gesture: f32,
    pub scroll: HigurashiTweener,
    pub close_flash: HigurashiTweener,
    pub scrollbar: Scrollbar,
    pub completion: String,
    initial: Vec<Action>,
    pending: Vec<Task>,
    next_viewer: u64,
}
fn control(c: &mut HigurashiTweener, value: f32, time: f32, flags: u32) {
    c.control(value, Ticks::from_f32(time), flags)
        .expect("Original Movie tween flags");
}
fn clear(c: &mut HigurashiTweener) {
    c.fast_forward_to(c.value());
}
fn flash(c: &mut HigurashiTweener, release: bool) {
    clear(c);
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
    clear(c);
    control(c, 0., f32::from_bits(0x3e4ccccd), 0x40);
}
impl Model {
    /// Owns only frozen row metadata/unlock snapshots. No RuntimeGameData,
    /// global progress/history/date or scene owner is cloned/projected here.
    pub fn new(catalog: &[MovieInfoItem], unlocked: &[u32], append: bool) -> Result<Self> {
        ensure!(
            catalog.len() <= u16::MAX as usize,
            "Movie metadata ID boundary"
        );
        let mut rows = Vec::new();
        for id in ROSTER {
            if catalog.get(id).is_none() {
                continue;
            }
            let extra = id > 11;
            if extra && !append {
                continue;
            }
            let acquired = unlocked
                .get(id / 32)
                .is_some_and(|w| w & (1 << (id % 32)) != 0);
            let i = rows.len();
            rows.push(RowState {
                catalog_index: id,
                movie_id: id as u16,
                append: extra,
                locked: !acquired,
                x: ((i % 4) * 420) as f32,
                y: ((i / 4) * 252) as f32,
                focus: 0.5,
            });
        }
        let base = rows.iter().filter(|r| !r.append).count();
        let acquired = rows.iter().filter(|r| !r.append && !r.locked).count();
        // Native empty-constructor observation is retained. Actions requiring
        // a selected row are checked below instead of reproducing its OOB use.
        let ratio = if base == 0 { 0 } else { acquired * 1000 / base };
        let percent = if ratio >= 1000 {
            "100".to_owned()
        } else {
            format!("{}.{}", ratio / 10, ratio % 10)
        };
        let initial = if acquired >= base {
            vec![Action::Achievement(36)]
        } else {
            vec![]
        };
        Ok(Self {
            append,
            document_height: ((rows.len().div_ceil(4) as i32 * 252) - 36).max(0),
            rows,
            x: 0,
            y: 0,
            closing: false,
            frame: 0,
            region: -1,
            hover: -1,
            contact: -1,
            gesture: 0.,
            scroll: HigurashiTweener::new(0.),
            close_flash: HigurashiTweener::new(0.),
            scrollbar: Scrollbar::default(),
            completion: format!("達成率:{percent}％"),
            initial,
            pending: vec![],
            next_viewer: 1,
        })
    }
    pub fn publish_initial(&mut self, services: &mut impl Services) -> Result<()> {
        while let Some(a) = self.initial.first().cloned() {
            services.action(a)?;
            self.initial.remove(0);
        }
        Ok(())
    }
    pub fn selected(&self) -> usize {
        (self.x + self.y * 4) as usize
    }
    pub fn take_pending_tasks(&mut self) -> Vec<Task> {
        std::mem::take(&mut self.pending)
    }
    pub fn view_state(&self, atlas_present: bool) -> ViewState {
        ViewState {
            ready: atlas_present,
            append: self.append,
            frame: self.frame,
            rows: self.rows.clone(),
            document_height: self.document_height,
            selected: self.selected(),
            scroll: self.scroll.value(),
            close_flash: self.close_flash.value(),
            scrollbar: self.scrollbar,
            completion: self.completion.clone(),
        }
    }
    fn reveal(&mut self) {
        let top = (self.y * 252) as f32;
        let current = self.scroll.value();
        let relative = top - current;
        let target = if relative < 0. {
            top.max(0.)
        } else if relative + 216. > 720. {
            ((top + 216.) + -720.).min(self.document_height as f32)
        } else {
            current
        };
        control(&mut self.scroll, target, 4., 0);
    }
    fn confirm(&mut self, services: &mut impl Services) -> Result<()> {
        let row = self
            .rows
            .get(self.selected())
            .ok_or_else(|| anyhow::anyhow!("Movie selected row outside catalog"))?;
        if row.locked {
            services.action(Action::Cue("error"))?;
        } else {
            let movie_id = row.movie_id;
            let id = self.next_viewer;
            services.action(Action::Cue("decide"))?;
            let viewer = Viewer::new(id, movie_id);
            services.action(Action::OpenPlayer {
                viewer: id,
                movie_id,
                source_category: 1,
            })?;
            self.next_viewer += 1;
            self.pending.push(Task::Viewer(viewer));
        }
        Ok(())
    }
    fn close(&mut self) {
        self.closing = true;
        flash(&mut self.close_flash, true);
        self.pending.push(Task::Close { done: false });
    }
    pub fn controller(
        &mut self,
        permission: bool,
        captured: bool,
        buttons: Buttons,
        services: &mut impl Services,
    ) -> Result<bool> {
        if !permission {
            services.action(Action::DisableInput(InputTarget::Grid))?;
        } else if !captured {
            if buttons.pressed & (1 << 25) != 0 {
                services.action(Action::Cue("cancel"))?;
                self.close();
            } else if buttons.pressed & (1 << 24) != 0 {
                self.confirm(services)?;
            } else {
                let r = buttons.repeat;
                let dx = if r & (1 << 7) != 0 {
                    -1
                } else {
                    i32::from(r & (1 << 5) != 0)
                };
                let dy = if r & (1 << 4) != 0 {
                    -1
                } else {
                    i32::from(r & (1 << 6) != 0)
                };
                if dx | dy != 0 {
                    ensure!(
                        !self.rows.is_empty(),
                        "Movie empty catalog direction boundary"
                    );
                    let mut y = (self.y + dy).clamp(0, self.rows.len().div_ceil(4) as i32 - 1);
                    let mut x = (self.x + dx).clamp(0, 3);
                    if y * 4 + x >= self.rows.len() as i32 {
                        let last = self.rows.len() as i32 - 1;
                        x = last % 4;
                        y = last / 4;
                    }
                    if x != self.x || y != self.y {
                        services.action(Action::Cue("move"))?;
                        self.gesture = 0.;
                        self.x = x;
                        self.y = y;
                        self.reveal();
                    }
                }
            }
        }
        Ok(!self.closing)
    }
    /// Parent93d08 tail: scope first, pre-scroll scrollbar, frame/focus only
    /// when atlas is present, then scroll and close clocks. Gesture is NOT inertia.
    pub fn advance_clocks(&mut self, delta: Ticks, atlas_present: bool) {
        if atlas_present {
            if self.document_height >= 720 {
                self.scrollbar = Scrollbar {
                    position: self.scroll.value() as i32,
                    visible: 720,
                    first: 0,
                    end: self.document_height,
                };
            }
            self.frame = self.frame.wrapping_add(1);
            let selected = self.selected();
            let step = delta.as_f32() * 0.125;
            for (i, row) in self.rows.iter_mut().enumerate() {
                row.focus = if i == selected {
                    (row.focus + step).min(1.)
                } else {
                    (row.focus - step).max(0.5)
                };
            }
        }
        self.scroll.update(delta);
        self.close_flash.update(delta);
    }
    pub fn probe(&mut self, point: Vec2) -> bool {
        self.hover = -1;
        self.region = if point.y >= 30. && point.y < 114. && point.x >= 1764. && point.x < 1848. {
            1
        } else if point.y >= 162. && point.y < 980. {
            -1
        } else {
            0
        };
        if self.region < 0 {
            let y = (self.scroll.value() + -210.) + point.y;
            for row in &self.rows {
                let left = row.x + 138.;
                if y >= row.y && y < row.y + 216. && point.x >= left && point.x < left + 384. {
                    self.hover = (row.x / 420.) as i32 + (row.y / 252.) as i32 * 4;
                    break;
                }
            }
        }
        true
    }
    pub fn reset(&mut self) {
        self.region = -1;
        self.hover = -1;
        self.contact = -1;
    }
    pub fn press(&mut self) {
        if self.region == 1 {
            flash(&mut self.close_flash, false);
        } else if self.region < 0 && self.hover >= 0 && (self.hover as usize) < self.rows.len() {
            self.x = self.hover % 4;
            self.y = self.hover / 4;
            clear(&mut self.scroll);
            self.reveal();
        }
    }
    pub fn release(&mut self, services: &mut impl Services) -> Result<()> {
        if self.region == 1 {
            services.action(Action::Cue("decide"))?;
            self.close();
        } else if self.region < 0 && self.hover >= 0 {
            if (self.hover as usize) < self.rows.len() {
                self.x = self.hover % 4;
                self.y = self.hover / 4;
                clear(&mut self.scroll);
                self.reveal();
            }
            self.confirm(services)?;
        }
        Ok(())
    }
    pub fn long_press(&mut self) {}
    pub fn drag(&mut self, previous: Vec2, current: Vec2) {
        if self.region >= 0 {
            clear_flash(&mut self.close_flash);
            self.region = 0;
        } else {
            let value = (self.scroll.value() - (current.y - previous.y))
                .min((self.document_height - 720) as f32)
                .max(0.);
            while value > (self.y * 252) as f32 {
                self.y += 1;
            }
            while value + 720. < ((self.y * 252) as f32 + 216.) {
                self.y -= 1;
            }
            self.scroll.fast_forward_to(value);
        }
    }
    pub fn gesture(&mut self, velocity: Vec2) {
        if self.region < 0 {
            self.gesture = velocity.y;
        }
    }
    pub fn cancel(&mut self) {
        if self.region >= 0 {
            clear_flash(&mut self.close_flash);
            self.region = 0;
        }
    }
}

/// No defaults: a caller must supply actual Player/First/input service owners.
/// Every fallible action stops successful-path polling before later clocks.
pub(super) trait Services {
    fn first_status(&mut self) -> Result<u32>;
    fn mask_published(&self) -> bool;
    fn input_grid(&mut self, model: &mut Model, permission: bool) -> Result<bool>;
    fn input_viewer(&mut self, viewer: &mut Viewer, permission: bool) -> Result<bool>;
    fn player_status(&mut self, viewer: u64) -> Result<u32>;
    fn action(&mut self, action: Action) -> Result<()>;
}
pub(super) enum Task {
    First { phase: i32, overlay: OverlayClock },
    Input { done: bool },
    Viewer(Viewer),
    Close { done: bool },
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
            Self::Viewer(v) => v.scope.is_empty(),
        }
    }
    fn retire_native(&mut self, services: &mut impl Services) -> Result<()> {
        match self {
            Self::First { .. } => services.action(Action::JoinFirst),
            Self::Viewer(v) => v.retire(services),
            _ => Ok(()),
        }
    }
}
pub(super) struct Poll<'a, S> {
    pub model: &'a mut Model,
    pub services: &'a mut S,
    fault: Option<anyhow::Error>,
}
impl<S: Services> Mode<Poll<'_, S>> for Task {
    fn completion(&mut self) -> u32 {
        u32::from(self.complete())
    }
    fn retire(&mut self, p: &mut Poll<'_, S>) {
        if p.fault.is_none() {
            if let Err(e) = self.retire_native(p.services) {
                p.fault = Some(e);
            }
        }
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        p: &mut Poll<'_, S>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        if let Some(e) = p.fault.take() {
            return Err(e);
        }
        match self {
            Self::First { phase, overlay } => {
                if *phase == 0 && p.services.first_status()? == 2 {
                    overlay.masked(
                        30,
                        f32::from_bits(0x3e99999a),
                        0,
                        p.services.mask_published(),
                    );
                    p.services.action(Action::StopBgm(60))?;
                    *phase = 1;
                } else if *phase == 1 && !overlay.busy() {
                    p.model.closing = false;
                    pushes.push(Self::Input { done: false });
                    *phase = -1;
                }
                overlay.advance(delta);
            }
            Self::Input { done } => {
                *done = !p.services.input_grid(p.model, permission)?;
            }
            Self::Viewer(v) => v.advance(delta, permission, p.services)?,
            Self::Close { done } => {
                if p.model.close_flash.is_idle() {
                    p.services.action(Action::StopBgm(30))?;
                    p.services.action(Action::ReturnTitle {
                        selected: 17,
                        mode: -1,
                    })?;
                    *done = true;
                }
            }
        }
        for task in p.model.take_pending_tasks() {
            pushes.push(task);
        }
        Ok(0)
    }
}
#[cfg(test)]
pub(super) fn advance(
    scope: &mut ModeStack<Task>,
    model: &mut Model,
    delta: Ticks,
    atlas: bool,
    services: &mut impl Services,
) -> Result<()> {
    let mut p = Poll {
        model,
        services,
        fault: None,
    };
    scope.poll(delta, 1, &mut p)?;
    if let Some(e) = p.fault {
        return Err(e);
    }
    p.model.advance_clocks(delta, atlas);
    Ok(())
}
/// Concrete owner must join workers/resources even after a desktop fault. This
/// fallible explicit boundary does not emulate native exception rollback.
#[cfg(test)]
pub(super) fn retire(
    scope: &mut ModeStack<Task>,
    model: &mut Model,
    services: &mut impl Services,
) -> Result<()> {
    services.action(Action::ReleaseAtlas)?;
    let mut p = Poll {
        model,
        services,
        fault: None,
    };
    scope.clear_with(&mut p);
    if let Some(e) = p.fault {
        return Err(e);
    }
    Ok(())
}

pub(super) struct Viewer {
    pub id: u64,
    #[allow(dead_code, reason = "Preserve source movie identity for retained viewer inspection.")]
    pub movie_id: u16,
    pub closing: bool,
    pub opacity: HigurashiTweener,
    pub backdrop: HigurashiTweener,
    pub scope: ModeStack<ViewerTask>,
    pending: Vec<ViewerTask>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ViewerTask {
    Ready { done: bool },
    Input { done: bool },
    Exit { done: bool },
    Backdrop { done: bool },
}
impl ViewerTask {
    pub fn complete(self) -> bool {
        match self {
            Self::Ready { done }
            | Self::Input { done }
            | Self::Exit { done }
            | Self::Backdrop { done } => done,
        }
    }
}
impl Viewer {
    fn new(id: u64, movie_id: u16) -> Self {
        let mut backdrop = HigurashiTweener::new(0.);
        control(&mut backdrop, 1., 30., 0);
        let mut scope = ModeStack::default();
        scope.push(ViewerTask::Ready { done: false });
        Self {
            id,
            movie_id,
            closing: false,
            opacity: HigurashiTweener::new(1.),
            backdrop,
            scope,
            pending: vec![],
        }
    }
    fn close(&mut self) {
        self.closing = true;
        control(&mut self.opacity, 0., 30., 0);
        self.pending.push(ViewerTask::Exit { done: false });
    }
    pub fn controller(
        &mut self,
        permission: bool,
        captured: bool,
        pressed: u32,
        services: &mut impl Services,
    ) -> Result<bool> {
        if services.player_status(self.id)? == 2 {
            self.close();
        } else if !permission {
            services.action(Action::DisableInput(InputTarget::Viewer(self.id)))?;
        } else if !captured && pressed & ((1 << 24) | (1 << 25)) != 0 {
            services.action(Action::Cue("cancel"))?;
            self.close();
        }
        Ok(!self.closing)
    }
    pub fn pointer_release(&mut self, services: &mut impl Services) -> Result<()> {
        services.action(Action::Cue("decide"))?;
        self.close();
        Ok(())
    }
    #[cfg(test)]
    pub fn append_pending(&mut self) {
        for task in std::mem::take(&mut self.pending) {
            self.scope.push(task);
        }
    }
    fn advance(
        &mut self,
        delta: Ticks,
        permission: bool,
        services: &mut impl Services,
    ) -> Result<()> {
        let mut scope = std::mem::take(&mut self.scope);
        let result = scope.poll(
            delta,
            u32::from(permission),
            &mut ViewerPoll {
                viewer: self,
                services: &mut *services,
            },
        );
        self.scope = scope;
        result?;
        services.action(Action::UpdatePlayer(self.id))?;
        self.opacity.update(delta);
        self.backdrop.update(delta);
        services.action(Action::PlatformByte)?;
        Ok(())
    }
    /// Viewer95e90 calls gain/color only when the external Player playing
    /// getter succeeds. Actual standalone draw90d34 stays with the host.
    pub fn prepare_player_draw(
        &self,
        playing: bool,
        volume: u8,
        services: &mut impl Services,
    ) -> Result<()> {
        if playing {
            let opacity = self.opacity.value();
            let gain = (f32::from(volume) * f32::from_bits(0x3c23d70a)) * opacity;
            services.action(Action::PlayerGain {
                viewer: self.id,
                value: gain,
            })?;
            services.action(Action::PlayerColor {
                viewer: self.id,
                value: [1., 1., 1., opacity],
            })?;
        }
        Ok(())
    }
    fn retire(&mut self, services: &mut impl Services) -> Result<()> {
        services.action(Action::StopPlayer(self.id))?;
        services.action(Action::ReleasePlayer(self.id))?;
        self.scope = ModeStack::default();
        Ok(())
    }
}
struct ViewerPoll<'a, S> {
    viewer: &'a mut Viewer,
    services: &'a mut S,
}
impl<S: Services> Mode<ViewerPoll<'_, S>> for ViewerTask {
    fn completion(&mut self) -> u32 {
        u32::from(self.complete())
    }
    fn poll(
        &mut self,
        _delta: Ticks,
        permission: bool,
        p: &mut ViewerPoll<'_, S>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        match self {
            Self::Ready { done } => {
                if p.viewer.backdrop.is_idle() && p.services.player_status(p.viewer.id)? != 0 {
                    p.services.action(Action::StartPlayer(p.viewer.id))?;
                    pushes.push(Self::Input { done: false });
                    *done = true;
                }
            }
            Self::Input { done } => {
                *done = !p.services.input_viewer(p.viewer, permission)?;
            }
            Self::Exit { done } => {
                if p.viewer.opacity.is_idle() || p.services.player_status(p.viewer.id)? != 1 {
                    p.services.action(Action::PausePlayer(p.viewer.id))?;
                    control(&mut p.viewer.backdrop, 0., 30., 0);
                    pushes.push(Self::Backdrop { done: false });
                    *done = true;
                }
            }
            Self::Backdrop { done } => {
                *done = p.viewer.backdrop.is_idle();
            }
        }
        for task in std::mem::take(&mut p.viewer.pending) {
            pushes.push(task);
        }
        Ok(0)
    }
}
