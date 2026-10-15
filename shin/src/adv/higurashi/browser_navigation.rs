//! Updated7b184/7bbdc chapter-navigation model and7d214..7db58 contacts.
//! List labels/glyph resources live in labels; the browser owns this model.
pub mod labels;
pub mod view;
use super::{
    browser_controls::{BrowserControls, Event, Services},
    system_menu::Cue,
};
use crate::{
    adv::input_area::{InputArea, InputAreaTarget, PointerFrame},
    layer::higurashi_properties::HigurashiTweener,
};
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::time::Ticks;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    ///0=start,1=history marker,97=selected,98=current,99=both,100=end.
    pub kind: i32,
    pub marker: i32,
    pub row: i32,
}
pub struct History {
    pub(super) row_indices: Vec<u32>,
    pub(super) markers: Vec<u32>,
    pub(super) current: usize,
}
impl History {
    pub fn new(row_indices: Vec<u32>, markers: Vec<u32>, current: usize) -> Result<Self> {
        ensure!(
            current < row_indices.len() && row_indices.len() <= i32::MAX as usize / 60,
            "Invalid navigation history"
        );
        ensure!(
            row_indices
                .iter()
                .chain(&markers)
                .all(|i| (*i as usize) < row_indices.len())
                && markers.len() + 4 <= i32::MAX as usize / 60,
            "Invalid navigation indices"
        );
        Ok(Self {
            row_indices,
            markers,
            current,
        })
    }
}
pub struct Navigation {
    area: InputArea,
    state: State,
}
struct State {
    entries: Vec<Entry>,
    phase: i32,
    slide: f32,
    selected: usize,
    pointed: i32,
    dragging: bool,
    velocity: f32,
    scroll: HigurashiTweener,
}
fn control(t: &mut HigurashiTweener, target: f32, time: f32, flags: u32) {
    t.control(target, Ticks::from_f32(time), flags)
        .expect("Native navigation tween");
}
impl Navigation {
    /// Original constructor reads row.index for markers, but vector positions
    /// for selected/current/end anchors. Keep both values available to drawing.
    pub fn new(
        row_indices: &[u32],
        markers: &[u32],
        current: usize,
        browser: &mut BrowserControls,
    ) -> Result<Self> {
        ensure!(
            row_indices.len() == browser.row_count() && current < row_indices.len(),
            "Invalid navigation history"
        );
        ensure!(
            row_indices.len() <= i32::MAX as usize / 60
                && row_indices
                    .iter()
                    .all(|i| (*i as usize) < row_indices.len()),
            "Invalid navigation row index"
        );
        ensure!(
            markers.iter().all(|i| (*i as usize) < row_indices.len()),
            "Invalid navigation marker"
        );
        let selected = browser.selected();
        let mut entries = vec![Entry {
            kind: 0,
            marker: -1,
            row: 0,
        }];
        entries.extend(markers.iter().map(|i| Entry {
            kind: 1,
            marker: *i as i32,
            row: row_indices[*i as usize] as i32,
        }));
        entries.push(Entry {
            kind: if current == selected { 99 } else { 97 },
            marker: -1,
            row: selected as i32,
        });
        if current != selected {
            entries.push(Entry {
                kind: 98,
                marker: -1,
                row: current as i32,
            });
        }
        entries.push(Entry {
            kind: 100,
            marker: -1,
            row: row_indices.len() as i32 - 1,
        });
        entries.sort_by_key(|e| (e.row, e.kind));
        ensure!(
            entries.len() <= i32::MAX as usize / 60,
            "Too many navigation entries"
        );
        let selected = entries.iter().position(|e| e.kind | 2 == 99).unwrap();
        let scroll =
            ((selected as i32 * 60 - 330).clamp(0, (entries.len() as i32 * 60 - 720).max(0)) / 60
                * 60) as f32;
        browser.navigation_opened();
        Ok(Self {
            area: InputArea::new(1280, 720),
            state: State {
                entries,
                phase: 0,
                slide: 0.0,
                selected,
                pointed: -1,
                dragging: false,
                velocity: 0.0,
                scroll: HigurashiTweener::new(scroll),
            },
        })
    }
    pub fn entries(&self) -> &[Entry] {
        &self.state.entries
    }
    pub fn phase(&self) -> i32 {
        self.state.phase
    }
    pub fn slide(&self) -> f32 {
        self.state.slide
    }
    pub fn selected(&self) -> usize {
        self.state.selected
    }
    pub fn scroll(&self) -> f32 {
        self.state.scroll.value()
    }
    pub fn scroll_waiting(&self) -> bool {
        self.state.scroll.is_waiting()
    }
    pub fn complete(&self) -> bool {
        self.state.phase < 0
    }
    /// The original deleting destructor resets browser+1d0, at stack pruning.
    pub fn retire(&mut self, browser: &mut BrowserControls) {
        browser.navigation_returned();
        tracing::debug!(phase = "retired", "HIGURASHI_CHAPTER_LIST");
    }
    pub fn cancel_input(&mut self, browser: &mut BrowserControls, services: &mut impl Services) {
        self.area.disable(&mut Contact { state: &mut self.state, browser, services });
    }
    /// Native8162c requests this only for the accepted stage1/Yes callback.
    pub fn begin_closing(&mut self) {
        self.state.phase += 1;
    }
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: u32,
        pressed: u32,
        repeated: u32,
        frame: &PointerFrame<'_>,
        browser: &mut BrowserControls,
        services: &mut impl Services,
    ) -> u32 {
        self.poll_with_mouse_release(
            delta, permission, pressed, repeated, frame, browser, services, None,
        )
    }
    pub(crate) fn poll_with_mouse_release(
        &mut self,
        delta: Ticks,
        permission: u32,
        pressed: u32,
        repeated: u32,
        frame: &PointerFrame<'_>,
        browser: &mut BrowserControls,
        services: &mut impl Services,
        mouse_release: Option<crate::adv::input_area::MouseRelease>,
    ) -> u32 {
        let s = &mut self.state;
        if s.phase == 0 {
            s.slide += delta.as_f32() * f32::from_bits(0x3e4c_cccd);
            if s.slide >= 1.0 {
                s.slide = 1.0;
                s.phase = 1;
                tracing::debug!(phase = "ready", "HIGURASHI_CHAPTER_LIST");
            }
        }
        if s.phase == 1 {
            let mut target = Contact {
                state: s,
                browser,
                services,
            };
            if permission & 1 == 0 || target.state.scroll.is_waiting() {
                self.area.disable(&mut target);
            } else if !self
                .area
                .update_with_mouse_release(&mut target, frame, mouse_release)
            {
                target.buttons(pressed, repeated);
            }
        } else if s.phase == 2 && !browser.close_flash_waiting() {
            s.slide += delta.as_f32() * f32::from_bits(0xbe4c_cccd);
            if s.slide <= 0.0 {
                s.slide = 0.0;
                s.phase = -1;
            }
        }
        if s.velocity != 0.0 {
            let target = (s.scroll.value() - s.velocity).clamp(0.0, s.maximum());
            s.move_scroll(target, browser);
            s.velocity *= 0.95f32.powf(delta.as_f32());
            if s.velocity.abs() < 0.1 {
                s.velocity = 0.0;
            }
        }
        if s.velocity == 0.0 && !s.dragging && !s.scroll.is_waiting() {
            let position = s.scroll.value() as i32;
            let remainder = position % 60;
            if remainder != 0 {
                control(
                    &mut s.scroll,
                    (position - remainder + if remainder > 29 { 60 } else { 0 }) as f32,
                    4.0,
                    0,
                );
            }
        }
        // Original navigation scroll advances one tick even for delta0/20.
        s.scroll.update(Ticks::from_f32(1.0));
        0
    }
}
impl State {
    fn maximum(&self) -> f32 {
        (self.entries.len() as i32 * 60 - 720).max(0) as f32
    }
    fn select(&mut self, selected: usize, browser: &mut BrowserControls) {
        self.selected = selected;
        let top = selected as f32 * 60.0;
        let relative = top - self.scroll.value();
        let target = if relative < 60.0 {
            (top - 60.0).max(0.0)
        } else if relative + 60.0 > 660.0 {
            (top - 600.0).min(self.maximum())
        } else {
            self.scroll.value()
        };
        control(&mut self.scroll, target, 4.0, 0x100);
        browser.navigation_select(self.entries[selected].row as usize);
    }
    fn move_scroll(&mut self, target: f32, browser: &mut BrowserControls) {
        let mut selected = self.selected;
        while selected as f32 * 60.0 <= target {
            selected += 1;
        }
        while target + 720.0 <= (selected + 1) as f32 * 60.0 {
            selected -= 1;
        }
        self.scroll.fast_forward_to(target);
        if selected != self.selected {
            self.selected = selected;
            browser.navigation_select(self.entries[selected].row as usize);
        }
    }
}
struct Contact<'a, S> {
    state: &'a mut State,
    browser: &'a mut BrowserControls,
    services: &'a mut S,
}
impl<S: Services> Contact<'_, S> {
    fn select(&mut self, selected: usize) {
        self.services.event(Event::Cue(Cue::Move));
        self.state.select(selected, self.browser);
    }
    fn buttons(&mut self, pressed: u32, repeated: u32) {
        let s = &mut self.state;
        let previous = s.selected;
        let proposed = if repeated & (1 << 4) != 0 {
            previous.saturating_sub(1)
        } else if repeated & (1 << 6) != 0 {
            previous + 1
        } else if repeated & (1 << 7) != 0 {
            previous.saturating_sub(4)
        } else if repeated & (1 << 5) != 0 {
            previous + 4
        } else if repeated & (1 << 11) != 0 {
            s.entries.len() - 1
        } else if repeated & (1 << 10) != 0 {
            0
        } else if pressed & (1 << 12) != 0 {
            s.entries.iter().position(|e| e.kind | 2 == 99).unwrap()
        } else {
            previous
        };
        let mut proposed = proposed.min(s.entries.len() - 1);
        let v = self.browser.viewport();
        while s.entries[proposed].row < v.first as i32 {
            proposed += 1;
        }
        // Native compares against end inclusively, unlike ordinary row input.
        while s.entries[proposed].row > v.end as i32 {
            proposed -= 1;
        }
        if proposed != previous {
            s.velocity = 0.0;
            self.select(proposed);
        } else if pressed & (1 << 25) != 0 {
            s.velocity = 0.0;
            self.services.event(Event::Cue(Cue::Cancel));
            self.state.phase += 1;
        } else if pressed & (1 << 24) != 0 {
            s.velocity = 0.0;
            self.browser.navigation_voice(self.services);
        } else if pressed & (1 << 15) != 0 {
            s.velocity = 0.0;
            self.browser.navigation_jump(self.services);
        }
    }
}
impl<S: Services> InputAreaTarget for Contact<'_, S> {
    fn probe(&mut self, p: Vec2) -> bool {
        let s = &mut self.state;
        s.pointed = -1;
        s.velocity = 0.0;
        if p.x >= 1764.0 && p.x < 1848.0 && p.y >= 30.0 && p.y < 114.0 {
            self.browser.set_navigation_region(1);
        } else {
            let content = p.y >= 162.0 && p.y < 980.0;
            self.browser
                .set_navigation_region(if content { -1 } else { 0 });
            if content && p.y >= 240.0 && p.y < 960.0 {
                let coordinate = ((p.y - 240.0) + s.scroll.value()) as i32;
                let index = coordinate / 60;
                if coordinate >= -59 && index < s.entries.len() as i32 {
                    s.pointed = index;
                }
            }
        }
        true
    }
    fn reset(&mut self) {
        self.browser.set_navigation_region(-1);
        self.state.pointed = -1;
        self.state.dragging = false;
    }
    fn press(&mut self, _: Vec2) {
        if self.browser.navigation_region() == 1 {
            self.browser.navigation_close_press();
        } else if self.browser.navigation_region() < 0
            && self.state.pointed >= 0
            && self.state.pointed as usize != self.state.selected
        {
            self.select(self.state.pointed as usize);
        }
    }
    fn release(&mut self, _: Vec2) {
        if self.browser.navigation_region() == 1 {
            self.services.event(Event::Cue(Cue::Confirm));
            self.browser.navigation_close(self.services);
            self.state.phase = 2;
        } else if self.browser.navigation_region() < 0
            && self.state.pointed >= 0
            && self.state.pointed as usize != self.state.selected
        {
            self.select(self.state.pointed as usize);
        }
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        if self.browser.navigation_region() >= 0 {
            self.browser.navigation_close_clear();
            self.browser.set_navigation_region(0);
        } else if self.state.pointed >= 0 {
            let target = (self.state.scroll.value() - (current.y - previous.y))
                .clamp(0.0, self.state.maximum());
            self.state.move_scroll(target, self.browser);
            self.state.dragging = true;
        }
    }
    fn gesture(&mut self, velocity: Vec2) {
        if self.browser.navigation_region() >= 0 {
            return;
        }
        let angle = velocity.y.atan2(velocity.x) / f32::from_bits(0x4049_0fd8) * 180.0;
        let distance = (velocity.x * velocity.x + velocity.y * velocity.y).sqrt();
        if angle > -30.0 && angle < 30.0 && distance >= 20.0 {
            self.services.event(Event::Cue(Cue::Cancel));
            self.state.phase = 2;
        } else {
            self.state.velocity = velocity.y;
        }
    }
    fn mouse_gesture(&mut self, displacement: Vec2, velocity: Vec2) {
        let candidate = crate::adv::input_area::desktop_gesture_vector(displacement, velocity);
        let angle = candidate.y.atan2(candidate.x) / f32::from_bits(0x4049_0fd8) * 180.0;
        // Total displacement is a desktop close fallback only. Vertical/left
        // releases keep their measured velocity, never displacement inertia.
        let closing = self.state.phase == 1;
        if candidate.length() >= 64.0 && angle > -30.0 && angle < 30.0 {
            self.gesture(candidate);
        } else if velocity.length() > 20.0 {
            // Native InputArea also gates the callback itself at strict>20.
            // Its desktop branch delivers every drag release, so retain that
            // outer producer gate before applying native vertical inertia.
            self.gesture(velocity);
        }
        if closing && self.state.phase == 2 {
            tracing::debug!(phase = "desktop_close", "HIGURASHI_CHAPTER_LIST");
        }
    }
    fn cancel(&mut self) {
        if self.browser.navigation_region() >= 0 {
            self.browser.navigation_close_clear();
            self.browser.set_navigation_region(0);
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod desktop_tests;
