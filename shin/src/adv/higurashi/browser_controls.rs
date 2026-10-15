//! Updated79a08 contact/navigation decisions and76f94 browser scroll clocks.
//! Worker publication, close/jump/navigation child modes and audio transport
//! are supplied by the browser owner. No persistent state is mutated here.
use std::collections::BTreeMap;

use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::time::Ticks;

use super::{browser_glyphs::PositionedRow, browser_history::Item, system_menu::Cue};
use crate::{
    adv::input_area::{InputArea, InputAreaTarget, MouseRelease, PointerFrame, desktop_gesture_vector},
    layer::higurashi_properties::HigurashiTweener,
};

#[derive(Clone, Copy, Debug)]
pub struct RowMetadata {
    pub kind: u8,
    pub voice_count: u8,
    pub top: f32,
    pub height: f32,
}
impl From<&PositionedRow> for RowMetadata {
    fn from(row: &PositionedRow) -> Self {
        Self {
            kind: match &row.row.item {
                Item::Message { .. } => 0,
                Item::Choice { .. } => 1,
                Item::Checkpoint { .. } => 2,
                Item::CharacterChoice { .. } => 3,
            },
            voice_count: row.voice_count,
            top: row.top,
            height: row.height,
        }
    }
}
/// The async worker publishes a measured index interval and document bounds.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub first: usize,
    pub end: usize,
    pub minimum: f32,
    pub extent: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scrollbar {
    pub position: i32,
    pub visible: i32,
    pub first: i32,
    pub end: i32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Cue(Cue),
    Close,
    Navigation,
    Jump(usize),
    Voice(usize),
    StopVoice,
    /// Original voice owner advances by one tick, independent of frame delta.
    VoiceTick,
    MusicMultiplier(f32),
}
pub trait Services {
    fn event(&mut self, event: Event);
    fn voice_is_playing(&self) -> bool;
}
pub struct RowFlashes {
    pub jump: HigurashiTweener,
    pub voice: HigurashiTweener,
}
impl Default for RowFlashes {
    fn default() -> Self {
        Self {
            jump: HigurashiTweener::new(0.0),
            voice: HigurashiTweener::new(0.0),
        }
    }
}
pub struct BrowserControls {
    area: InputArea,
    state: State,
}
struct State {
    rows: Vec<RowMetadata>,
    viewport: Viewport,
    selected: usize,
    region: i32,
    pointed_row: i32,
    velocity: f32,
    scroll: HigurashiTweener,
    close_flash: HigurashiTweener,
    flashes: BTreeMap<usize, RowFlashes>,
    navigation: u32,
    blocked: bool,
    blink: i32,
    scrollbar: Scrollbar,
}

fn control(tween: &mut HigurashiTweener, value: f32, time: f32, flags: u32) {
    tween
        .control(value, Ticks::from_f32(time), flags)
        .expect("Native browser tween flags");
}
fn clear(tween: &mut HigurashiTweener) {
    tween.fast_forward_to(tween.value());
}
fn press_flash(tween: &mut HigurashiTweener) {
    clear(tween);
    control(tween, 0.7, 0.2, 0x40);
}
fn release_flash(tween: &mut HigurashiTweener) {
    press_flash(tween);
    control(tween, 0.0, 0.2, 0x40);
}
fn clear_flash(tween: &mut HigurashiTweener) {
    clear(tween);
    control(tween, 0.0, 0.2, 0x40);
}
fn angle(delta: Vec2) -> f32 {
    // Native constant differs from std::f32::consts::PI.
    delta.y.atan2(delta.x) / f32::from_bits(0x40490fd8) * 180.0
}
fn horizontal(angle: f32) -> bool {
    angle > 150.0 || angle < -150.0 || (angle > -30.0 && angle < 30.0)
}

impl BrowserControls {
    pub fn new(
        rows: Vec<RowMetadata>,
        viewport: Viewport,
        selected: usize,
        scroll: f32,
    ) -> Result<Self> {
        ensure!(
            viewport.first < viewport.end
                && viewport.end <= rows.len()
                && (viewport.first..viewport.end).contains(&selected),
            "Invalid browser measured interval"
        );
        ensure!(
            viewport.minimum.is_finite() && viewport.extent.is_finite() && scroll.is_finite(),
            "Invalid browser document bounds"
        );
        ensure!(
            rows.iter().all(|r| r.kind <= 3
                && r.top.is_finite()
                && r.height.is_finite()
                && r.height > 0.0),
            "Invalid browser row metadata"
        );
        Ok(Self {
            area: InputArea::new(1280, 720),
            state: State {
                rows,
                viewport,
                selected,
                region: -1,
                pointed_row: -1,
                velocity: 0.0,
                scroll: HigurashiTweener::new(scroll),
                close_flash: HigurashiTweener::new(0.0),
                flashes: BTreeMap::new(),
                navigation: 0,
                blocked: false,
                blink: 0,
                scrollbar: Scrollbar::default(),
            },
        })
    }
    /// Publish a growing worker interval without resetting input/scroll/flash
    /// owners. Unmeasured row slots carry finite placeholders until published.
    pub fn publish_measurement(
        &mut self,
        rows: Vec<RowMetadata>,
        viewport: Viewport,
    ) -> Result<()> {
        ensure!(
            rows.len() == self.state.rows.len()
                && viewport.first <= self.state.viewport.first
                && viewport.end >= self.state.viewport.end
                && viewport.end <= rows.len()
                && viewport.minimum <= self.state.viewport.minimum
                && viewport.extent >= self.state.viewport.extent,
            "Browser measurement must extend its retained range"
        );
        ensure!(
            viewport.minimum.is_finite()
                && viewport.extent.is_finite()
                && rows.iter().all(|r| r.kind <= 3
                    && r.top.is_finite()
                    && r.height.is_finite()
                    && r.height > 0.0),
            "Invalid browser measurement metadata"
        );
        self.state.rows = rows;
        self.state.viewport = viewport;
        Ok(())
    }
    /// Mirror the glyph cache's retained rows; flashes retire with their entry.
    pub fn set_cached_rows(&mut self, indices: impl IntoIterator<Item = usize>) {
        let indices: std::collections::BTreeSet<_> = indices.into_iter().collect();
        self.state.flashes.retain(|i, _| indices.contains(i));
        for i in indices {
            if i < self.state.rows.len() {
                self.state.flashes.entry(i).or_default();
            }
        }
    }
    pub fn selected(&self) -> usize {
        self.state.selected
    }
    pub fn scroll(&self) -> f32 {
        self.state.scroll.value()
    }
    pub fn close_flash(&self) -> f32 {
        self.state.close_flash.value()
    }
    pub fn close_flash_waiting(&self) -> bool {
        self.state.close_flash.is_waiting()
    }
    pub fn scroll_waiting(&self) -> bool {
        self.state.scroll.is_waiting()
    }
    pub fn blocked(&self) -> bool {
        self.state.blocked
    }
    pub(super) fn selected_metadata(&self) -> RowMetadata {
        self.state.rows[self.state.selected]
    }
    pub(super) fn viewport(&self) -> Viewport {
        self.state.viewport
    }
    pub fn flashes(&self, row: usize) -> Option<&RowFlashes> {
        self.state.flashes.get(&row)
    }
    pub fn scrollbar(&self) -> Scrollbar {
        self.state.scrollbar
    }
    pub fn blink(&self) -> i32 {
        self.state.blink
    }
    pub fn navigation(&self) -> u32 {
        self.state.navigation
    }
    /// Opening/closing and navigation child owners publish these fields.
    pub fn set_blocked(&mut self, blocked: bool) {
        self.state.blocked = blocked;
    }
    pub fn navigation_returned(&mut self) {
        self.state.navigation = 0;
    }
    pub(super) fn row_count(&self) -> usize {
        self.state.rows.len()
    }
    pub(super) fn navigation_opened(&mut self) {
        self.state.navigation = 1;
    }
    pub(super) fn navigation_region(&self) -> i32 {
        self.state.region
    }
    pub(super) fn set_navigation_region(&mut self, region: i32) {
        self.state.region = region;
    }
    pub(super) fn navigation_select(&mut self, row: usize) {
        self.state.selected = row;
        clear(&mut self.state.scroll);
        self.state.ensure_visible();
    }
    pub(super) fn navigation_close_press(&mut self) {
        press_flash(&mut self.state.close_flash);
    }
    pub(super) fn navigation_close_clear(&mut self) {
        clear_flash(&mut self.state.close_flash);
    }
    pub(super) fn navigation_close(&mut self, services: &mut impl Services) {
        self.state.close(services);
    }
    pub(super) fn navigation_voice(&mut self, services: &mut impl Services) {
        self.state.voice(services);
    }
    pub(super) fn navigation_jump(&mut self, services: &mut impl Services) {
        self.state.jump(services);
    }
    pub fn set_scroll(&mut self, scroll: f32) {
        self.state.scroll.fast_forward_to(scroll);
    }
    pub fn cancel_input(&mut self, services: &mut impl Services) {
        self.area.disable(&mut Contact { state: &mut self.state, services });
    }

    /// Original input task receives a byte permission and clears it. The return
    /// value keeps that task alive until the closing owner publishes blocked.
    /// Its parent browser always returns zero permission to the lower ADV mode.
    pub fn input(
        &mut self,
        permission: u8,
        pressed: u32,
        repeated: u32,
        frame: &PointerFrame<'_>,
        services: &mut impl Services,
    ) -> bool {
        self.input_with_mouse_release(permission, pressed, repeated, frame, services, None)
    }
    pub(crate) fn input_with_mouse_release(
        &mut self,
        permission: u8,
        pressed: u32,
        repeated: u32,
        frame: &PointerFrame<'_>,
        services: &mut impl Services,
        mouse_release: Option<MouseRelease>,
    ) -> bool {
        let state = &mut self.state;
        if permission == 0 {
            self.area.disable(&mut Contact { state, services });
        } else if !self.area.update_with_mouse_release(
            &mut Contact { state, services },
            frame,
            mouse_release,
        )
            && !state.scroll.is_waiting()
        {
            state.buttons(pressed, repeated, services);
        }
        !state.blocked
    }
    /// Called after child mode/input polling. Inertia uses frame delta, while
    /// native scroll and voice clocks each advance by exactly one tick.
    pub fn tick(&mut self, delta: Ticks, duck_music: bool, services: &mut impl Services) {
        let state = &mut self.state;
        if state.velocity != 0.0 {
            state.move_scroll(state.scroll.value() - state.velocity);
            state.velocity *= 0.95f32.powf(delta.as_f32());
            if state.velocity.abs() < 0.1 {
                state.velocity = 0.0;
            }
        }
        let position = state.scroll.value() as i32;
        let end = state.viewport.extent.ceil() as i32;
        state.scrollbar = Scrollbar {
            position,
            visible: if position.wrapping_add(818) > end {
                end.wrapping_sub(position)
            } else {
                818
            },
            first: state.viewport.minimum.floor() as i32,
            end,
        };
        state.scroll.update(Ticks::from_f32(1.0));
        services.event(Event::VoiceTick);
        state.close_flash.update(delta);
        for flash in state.flashes.values_mut() {
            flash.jump.update(delta);
            flash.voice.update(delta);
        }
        services.event(Event::MusicMultiplier(
            if duck_music && services.voice_is_playing() {
                0.5
            } else {
                1.0
            },
        ));
        state.blink = state.blink.wrapping_add(1) % 50;
    }
}

impl State {
    fn ensure_visible(&mut self) {
        let row = self.rows[self.selected];
        let mut target = self.scroll.value();
        let relative = row.top - target;
        if relative < 0.0 {
            target = row.top.max(self.viewport.minimum);
        } else if relative + row.height > 818.0 {
            target = ((row.top + row.height) - 818.0)
                .min(self.viewport.extent - 818.0)
                .max(self.viewport.minimum);
        }
        control(&mut self.scroll, target, 4.0, 0);
    }
    fn move_scroll(&mut self, target: f32) {
        let scroll = target.clamp(
            self.viewport.minimum,
            (self.viewport.extent - 818.0).max(self.viewport.minimum),
        );
        let mut direction = -1;
        loop {
            let row = self.rows[self.selected];
            if row.top < scroll {
                if direction == 1 || self.selected + 1 >= self.viewport.end {
                    break;
                }
                direction = 0;
                self.selected += 1;
            } else if direction != 0 && scroll + 818.0 < row.top + row.height {
                if self.selected <= self.viewport.first {
                    break;
                }
                direction = 1;
                self.selected -= 1;
            } else {
                break;
            }
        }
        self.scroll.fast_forward_to(scroll);
    }
    fn close(&mut self, services: &mut impl Services) {
        release_flash(&mut self.close_flash);
        services.event(Event::Close);
    }
    fn navigation(&mut self, services: &mut impl Services) {
        self.navigation = 1;
        services.event(Event::Navigation);
    }
    fn jump(&mut self, services: &mut impl Services) {
        services.event(Event::Cue(Cue::Confirm));
        if let Some(f) = self.flashes.get_mut(&self.selected) {
            release_flash(&mut f.jump);
        }
        services.event(Event::Jump(self.selected));
    }
    fn voice(&mut self, services: &mut impl Services) {
        let row = self.rows[self.selected];
        if row.kind != 0 || row.voice_count == 0 {
            return;
        }
        if let Some(f) = self.flashes.get_mut(&self.selected) {
            release_flash(&mut f.voice);
        }
        services.event(Event::Voice(self.selected));
    }
    fn buttons(&mut self, pressed: u32, repeated: u32, services: &mut impl Services) {
        let first = self.viewport.first;
        let end = self.viewport.end;
        let previous = self.selected;
        let proposed = if repeated & (1 << 4) != 0 {
            previous.saturating_sub(1).max(first)
        } else if repeated & (1 << 6) != 0 {
            (previous + 1).min(end - 1)
        } else if repeated & (1 << 7) != 0 {
            previous.saturating_sub(4).max(first)
        } else if repeated & (1 << 5) != 0 {
            (previous + 4).min(end - 1)
        } else if repeated & (1 << 11) != 0 {
            ((previous + 1)..end)
                .find(|i| self.rows[*i].kind != 0)
                .unwrap_or(end - 1)
        } else if repeated & (1 << 10) != 0 {
            (first..previous)
                .rev()
                .find(|i| self.rows[*i].kind != 0)
                .unwrap_or(first)
        } else {
            previous
        };
        if proposed != previous {
            services.event(Event::Cue(Cue::Move));
            self.velocity = 0.0;
            self.selected = proposed;
            self.ensure_visible();
        } else if pressed & (1 << 25) != 0 {
            services.event(Event::Cue(Cue::Cancel));
            services.event(Event::StopVoice);
            self.close(services);
        } else if pressed & (1 << 12) != 0 {
            services.event(Event::Cue(Cue::Confirm));
            self.velocity = 0.0;
            self.navigation(services);
        } else if pressed & (1 << 24) != 0 {
            self.velocity = 0.0;
            self.voice(services);
        } else if pressed & (1 << 15) != 0 {
            self.velocity = 0.0;
            self.jump(services);
        }
    }
    fn document_y(&self, y: f32) -> f32 {
        (self.scroll.value() as i32).wrapping_sub(162) as f32 + y
    }
    fn inside_pointed_row(&self, position: Vec2) -> bool {
        let row = self.rows[self.selected];
        let y = self.document_y(position.y);
        y >= row.top && y < row.top + row.height && position.x >= 150.0 && position.x < 1770.0
    }
}

struct Contact<'a, S> {
    state: &'a mut State,
    services: &'a mut S,
}
impl<S: Services> InputAreaTarget for Contact<'_, S> {
    fn probe(&mut self, p: Vec2) -> bool {
        let s = &mut self.state;
        s.pointed_row = -1;
        s.velocity = 0.0;
        s.region = if p.y >= 30.0 && p.y < 114.0 && p.x >= 1764.0 && p.x < 1848.0 {
            1
        } else if p.y >= 162.0 && p.y < 980.0 {
            -1
        } else {
            0
        };
        if s.region == -1 && p.x >= 150.0 && p.x < 1770.0 {
            let y = s.document_y(p.y);
            let mut i = s.selected as i32;
            loop {
                let Some(row) = s.rows.get(i as usize) else {
                    break;
                };
                if y < row.top {
                    i -= 1;
                } else if y > row.top + row.height {
                    i += 1;
                } else {
                    if y >= row.top + 18.0 && y < row.top + row.height - 18.0 {
                        s.pointed_row = i;
                    }
                    break;
                }
            }
        }
        true
    }
    fn reset(&mut self) {
        self.state.region = -1;
        self.state.pointed_row = -1;
    }
    fn press(&mut self, p: Vec2) {
        let s = &mut self.state;
        if s.region == 1 {
            press_flash(&mut s.close_flash);
        } else if s.region < 0 && s.pointed_row >= 0 {
            s.selected = s.pointed_row as usize;
            s.ensure_visible();
            if s.inside_pointed_row(p) && p.x - 150.0 < 136.0 {
                let row = s.rows[s.selected];
                if row.kind == 0 && row.voice_count != 0 {
                    if let Some(f) = s.flashes.get_mut(&s.selected) {
                        press_flash(&mut f.voice);
                    }
                }
            }
        }
    }
    fn release(&mut self, p: Vec2) {
        let s = &mut self.state;
        if s.region == 1 {
            self.services.event(Event::Cue(Cue::Confirm));
            s.close(self.services);
        } else if s.region < 0 && s.pointed_row >= 0 {
            s.selected = s.pointed_row as usize;
            s.ensure_visible();
            if s.inside_pointed_row(p) {
                if p.x - 150.0 < 136.0 {
                    s.voice(self.services);
                } else {
                    s.jump(self.services);
                }
            }
        }
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        let s = &mut self.state;
        if s.region >= 0 {
            clear_flash(&mut s.close_flash);
            s.region = 0;
        } else {
            let delta = current - previous;
            if horizontal(angle(delta)) {
                return;
            }
            s.move_scroll(s.scroll.value() - delta.y);
            if let Some(f) = s.flashes.get_mut(&(s.pointed_row as usize)) {
                clear_flash(&mut f.voice);
            }
            s.pointed_row = -1;
        }
    }
    fn gesture(&mut self, velocity: Vec2) {
        let s = &mut self.state;
        if s.region < 0 {
            let distance = (velocity.x * velocity.x + velocity.y * velocity.y).sqrt();
            if distance > 20.0 && (angle(velocity) > 150.0 || angle(velocity) < -150.0) {
                self.services.event(Event::Cue(Cue::Confirm));
                s.navigation(self.services);
            } else {
                s.velocity = velocity.y;
            }
            s.pointed_row = -1;
        }
    }
    fn mouse_gesture(&mut self, displacement: Vec2, velocity: Vec2) {
        let previous = self.state.navigation;
        let candidate = desktop_gesture_vector(displacement, velocity);
        let candidate_angle = angle(candidate);
        if candidate.length() >= 64.0
            && (candidate_angle > 150.0 || candidate_angle < -150.0)
        {
            self.gesture(candidate);
        } else if velocity.length() > 20.0 {
            // Native InputArea dispatches gesture only above this threshold.
            // Keep measured vertical inertia; displacement is only a left-swipe fallback.
            self.gesture(velocity);
        }
        if previous == 0 && self.state.navigation != 0 {
            tracing::debug!(phase = "desktop_open", "HIGURASHI_CHAPTER_LIST");
        }
    }
    fn cancel(&mut self) {
        if self.state.region >= 0 {
            clear_flash(&mut self.state.close_flash);
            self.state.region = 0;
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod desktop_tests;
