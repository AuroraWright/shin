//! Native ChapterSelector84500/84EC0/8541C/86378..86CDC controller.
//! Parent browser scrolling, voice, dialogs, text/assets and InputArea are services.
#[path = "../cg_gallery/motion.rs"]
mod motion;
use motion::Motion;
#[path = "chapter/artwork.rs"]
pub mod artwork;
#[cfg(test)]
#[path = "chapter/tests.rs"]
mod tests;

pub const ROW_HEIGHT: f32 = 60.;
pub const VIEW_HEIGHT: f32 = 720.;
const EPSILON: f32 = f32::from_bits(0x34000000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: u32,
    pub marker_index: i32,
    pub row: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Opening,
    Active,
    Closing,
    Finished,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub permission: bool,
    pub captured: bool,
    pub pressed: u32,
    pub repeated: u32,
    pub axis: f32,
    /// Original reads these as inclusive browser row bounds every active frame.
    pub first_row: usize,
    pub last_row: usize,
    pub parent_close_busy: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    ParentSelect {
        row: usize,
    },
    Move,
    Back,
    Decide,
    ReplayVoice {
        row: usize,
    },
    QuickLoad {
        row: usize,
    },
    /// The shared close button closes the browser as well as this child.
    CloseBrowser,
    CloseFlash {
        held: bool,
    },
    Jump {
        row: usize,
    },
    DisableInput,
}
pub struct Controller {
    entries: Vec<Entry>,
    selected: usize,
    phase: Phase,
    progress: f32,
    scroll: Motion,
    target: f32,
    hover: Option<usize>,
    parent_target: i32,
    dragging: bool,
    velocity: f32,
}
impl Controller {
    pub fn new(
        row_count: usize,
        markers: &[usize],
        selected: usize,
        current: usize,
    ) -> Result<Self, &'static str> {
        if row_count == 0
            || selected >= row_count
            || current >= row_count
            || markers.iter().any(|&row| row >= row_count)
        {
            return Err("Ger chapter rows disagree with browser");
        }
        let mut entries = vec![Entry {
            kind: 0,
            marker_index: -1,
            row: 0,
        }];
        entries.extend(markers.iter().map(|&row| Entry {
            kind: 1,
            marker_index: row as i32,
            row,
        }));
        if selected == current {
            entries.push(Entry {
                kind: 99,
                marker_index: -1,
                row: selected,
            })
        } else {
            entries.push(Entry {
                kind: 97,
                marker_index: -1,
                row: selected,
            });
            entries.push(Entry {
                kind: 98,
                marker_index: -1,
                row: current,
            })
        }
        entries.push(Entry {
            kind: 100,
            marker_index: -1,
            row: row_count - 1,
        });
        entries.sort_by_key(|entry| (entry.row, entry.kind));
        let selected = entries
            .iter()
            .position(|entry| entry.kind | 2 == 99)
            .unwrap();
        let maximum = (entries.len() as i32 * 60).max(720) - 720;
        let scroll = if selected < 6 {
            0
        } else {
            ((selected as i32 * 60 - 330).min(maximum) / 60) * 60
        };
        Ok(Self {
            entries,
            selected,
            phase: Phase::Opening,
            progress: 0.,
            scroll: Motion::new(scroll as f32),
            target: scroll as f32,
            hover: None,
            parent_target: 0,
            dragging: false,
            velocity: 0.,
        })
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn selected_row(&self) -> usize {
        self.entries[self.selected].row
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn progress(&self) -> f32 {
        self.progress
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn scroll_target(&self) -> f32 {
        self.scroll_target_value()
    }
    fn scroll_target_value(&self) -> f32 {
        self.target
    }
    pub fn input_allowed(&self, permission: bool) -> bool {
        permission && self.phase == Phase::Active && !self.scroll.busy()
    }
    pub fn hover(&self) -> Option<usize> {
        self.hover
    }
    pub fn parent_target(&self) -> i32 {
        self.parent_target
    }
    pub fn velocity(&self) -> f32 {
        self.velocity
    }
    pub fn dragging(&self) -> bool {
        self.dragging
    }
    fn maximum_scroll(&self) -> f32 {
        (self.entries.len() as f32 * 60. - 720.).max(0.)
    }
    fn choose(&mut self, index: usize, events: &mut Vec<Event>) {
        self.selected = index;
        let y = (index * 60) as f32;
        let old = self.scroll.value();
        let relative = y - old;
        let target = if relative < 60. {
            (y - 60.).max(0.)
        } else if relative + 60. > 660. {
            (y - 600.).min(self.maximum_scroll())
        } else {
            old
        };
        self.set_target(target);
        events.push(Event::ParentSelect {
            row: self.selected_row(),
        });
    }
    fn set_target(&mut self, target: f32) {
        self.target = target;
        self.scroll.target(target, 4., 0, false)
    }
    fn set_scroll(&mut self, value: f32, events: &mut Vec<Event>) {
        let mut index = self.selected;
        while (index * 60) as f32 <= value {
            index += 1
        }
        while (index * 60 + 60) as f32 >= value + 720. {
            index -= 1
        }
        self.scroll.snap(value);
        self.target = value;
        if index != self.selected {
            self.selected = index;
            events.push(Event::ParentSelect {
                row: self.selected_row(),
            })
        }
    }
    pub fn update(&mut self, delta: f32, input: Input) -> Vec<Event> {
        self.update_with_contact(delta, input, |_| (false, vec![]))
    }
    ///84EC0 advances the phase before dispatching62B44, then applies keyboard,
    /// inertia and the literal1 scroll tick. The callback returns InputArea's
    /// captured result and its parent events. Readiness is sampled before the
    /// callback, so selecting a row does not cancel that same live contact.
    pub fn update_with_contact(
        &mut self,
        delta: f32,
        input: Input,
        contact: impl FnOnce(&mut Self) -> (bool, Vec<Event>),
    ) -> Vec<Event> {
        assert!(delta.is_finite() && delta >= 0. && input.axis.is_finite());
        let mut events = vec![];
        match self.phase {
            Phase::Opening => {
                self.progress = delta.mul_add(0.2, self.progress);
                if self.progress >= 1. {
                    self.progress = 1.;
                    self.phase = Phase::Active;
                }
            }
            Phase::Closing => {
                if !input.parent_close_busy {
                    self.progress = delta.mul_add(-0.2, self.progress);
                    if self.progress <= 0. {
                        self.progress = 0.;
                        self.phase = Phase::Finished;
                    }
                }
            }
            _ => {}
        }
        let active = self.phase == Phase::Active;
        let allowed = active && input.permission && !self.scroll.busy();
        let captured = if allowed {
            let (captured, mut callback_events) = contact(self);
            events.append(&mut callback_events);
            captured
        } else {
            false
        };
        if active {
            if !allowed {
                events.push(Event::DisableInput)
            } else if !(input.captured || captured) {
                self.dragging = false;
                let old = self.selected;
                let repeated = input.repeated;
                let mut next = if repeated & (1 << 24) != 0 {
                    old as i64 - 1
                } else if repeated & (1 << 25) != 0 {
                    old as i64 + 1
                } else if repeated & (1 << 26) != 0 {
                    old as i64 - 4
                } else if repeated & (1 << 27) != 0 {
                    old as i64 + 4
                } else if repeated & (1 << 11) != 0 {
                    self.entries.len() as i64
                } else if repeated & (1 << 10) != 0 {
                    0
                } else if input.pressed & (1 << 6) != 0 {
                    self.entries
                        .iter()
                        .position(|entry| entry.kind | 2 == 99)
                        .unwrap() as i64
                } else {
                    old as i64
                };
                next = next.clamp(0, self.entries.len() as i64 - 1);
                let mut next = next as usize;
                while self.entries[next].row < input.first_row {
                    next += 1
                }
                while self.entries[next].row > input.last_row {
                    next -= 1
                }
                if next != old {
                    events.push(Event::Move);
                    self.velocity = 0.;
                    self.choose(next, &mut events)
                } else if input.pressed & (1 << 29) != 0 {
                    events.push(Event::Back);
                    self.velocity = 0.;
                    self.phase = Phase::Closing
                } else if input.pressed & (1 << 28) != 0 {
                    self.velocity = 0.;
                    events.push(Event::ReplayVoice {
                        row: self.selected_row(),
                    })
                } else if input.pressed & (1 << 7) != 0 {
                    self.velocity = 0.;
                    events.push(Event::QuickLoad {
                        row: self.selected_row(),
                    })
                } else if input.axis != 0. {
                    let value = input
                        .axis
                        .mul_add(32., self.scroll.value())
                        .clamp(0., self.maximum_scroll());
                    self.set_scroll(value, &mut events);
                    self.scroll.snap(value);
                    self.target = value;
                    self.velocity = 0.;
                    self.dragging = true;
                }
            }
        }
        if self.velocity.abs() >= EPSILON {
            let old = self.scroll.value();
            let value = self
                .velocity
                .mul_add(-delta, old)
                .clamp(0., self.maximum_scroll());
            if (old - value).abs() < EPSILON {
                self.velocity = 0.
            } else {
                self.set_scroll(value, &mut events);
                self.velocity *= 0.95f32.powf(delta);
                if self.velocity.abs() < 0.1 {
                    self.velocity = 0.
                }
            }
        }
        if self.velocity.abs() < EPSILON && !self.dragging && !self.scroll.busy() {
            let old = self.scroll.value() as i32;
            let remainder = old % 60;
            if remainder != 0 {
                self.set_target((if remainder < 30 { old } else { old + 60 } - remainder) as f32)
            }
        }
        self.scroll.update(1.);
        events
    }
    pub fn probe(&mut self, x: f32, y: f32) -> bool {
        self.dragging = false;
        self.hover = None;
        if (1766.0..1874.).contains(&x) && (46.0..154.).contains(&y) {
            self.parent_target = 1;
            return true;
        }
        self.parent_target = if (160.0..992.).contains(&y) { -1 } else { 0 };
        if self.parent_target < 0 && (240.0..960.).contains(&y) {
            let raw = (y + -240. + self.scroll.value()) as i32;
            let index = raw / 60;
            if raw >= -59 && index >= 0 && (index as usize) < self.entries.len() {
                self.hover = Some(index as usize);
                if self.velocity.abs() >= EPSILON {
                    self.hover = None;
                    self.velocity = 0.;
                    self.dragging = true
                }
            }
        }
        true
    }
    pub fn reset(&mut self) {
        self.parent_target = -1;
        self.hover = None;
        self.dragging = false;
    }
    fn select_hover(&mut self, events: &mut Vec<Event>) {
        if let Some(index) = self.hover
            && index != self.selected
        {
            events.push(Event::Move);
            self.choose(index, events)
        }
    }
    pub fn press(&mut self) -> Vec<Event> {
        let mut events = vec![];
        if self.parent_target == 1 {
            events.push(Event::CloseFlash { held: true })
        } else if self.parent_target < 0 {
            self.select_hover(&mut events)
        }
        events
    }
    pub fn release(&mut self) -> Vec<Event> {
        let mut events = vec![];
        if self.parent_target == 1 {
            events.push(Event::Decide);
            events.push(Event::CloseBrowser);
            self.phase = Phase::Closing
        } else if self.parent_target < 0 {
            self.select_hover(&mut events)
        }
        events
    }
    pub fn drag(&mut self, previous: [f32; 2], current: [f32; 2]) -> Vec<Event> {
        let mut events = vec![];
        if self.parent_target >= 0 {
            events.push(Event::CloseFlash { held: false });
            self.parent_target = 0
        } else {
            let value =
                (self.scroll.value() - (current[1] - previous[1])).clamp(0., self.maximum_scroll());
            self.set_scroll(value, &mut events);
            self.dragging = true
        }
        events
    }
    pub fn fling(&mut self, velocity: [f32; 2]) -> Vec<Event> {
        if self.parent_target >= 0 {
            return vec![];
        }
        let angle = (velocity[1].atan2(velocity[0]) / f32::from_bits(0x40490fd8)) * 180.;
        let length = velocity[0]
            .mul_add(velocity[0], velocity[1] * velocity[1])
            .sqrt();
        if length >= 20. && angle > -30. && angle < 30. {
            self.phase = Phase::Closing;
            vec![Event::Back]
        } else {
            self.velocity = velocity[1];
            vec![]
        }
    }
    pub fn cancel(&mut self) -> Vec<Event> {
        if self.parent_target >= 0 {
            self.parent_target = 0;
            vec![Event::CloseFlash { held: false }]
        } else {
            vec![]
        }
    }
    pub fn jump_response(&mut self, final_stage: bool, answer: i32) -> Vec<Event> {
        if final_stage && answer == 0 {
            self.phase = Phase::Closing;
            vec![Event::Jump {
                row: self.selected_row(),
            }]
        } else {
            vec![]
        }
    }
}
