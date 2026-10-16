//! Ger MessageBrowser82C04 controls, supplied with native history/layout rows.
//! History replay, row text/voices, child tasks and modal publication are host
//! services. This module does not substitute the Umineko730px browser geometry.
pub mod chapter;
pub mod history;
pub mod lifecycle;
pub mod view;
pub mod voice_queue;

pub const ATLAS: &str = "/logview.txa";
pub const VIEW_HEIGHT: f32 = 832.0;
pub const JUMP_PROMPT: &str = "シーンジャンプを実行します。@rよろしいですか？";

///18EC4 releases the ADV modal on reason0 at browser Exit, before its outgoing
/// fade. The subsequent reason1 completion is ignored by that callback.
pub fn returns_to_adv(reason: i32) -> bool {
    reason == 0
}

///88E10 mutates the frozen active progress entry only for final Yes. Preserve
/// the old saved boundary as replay_target and select the history row boundary
/// as save_position; the resulting owner queues a fresh ADV after its fade.
pub fn jump_positions(
    final_stage: bool,
    answer: i32,
    old_save_position: u32,
    row_boundary: u32,
) -> Option<(u32, u32)> {
    (final_stage && answer == 0).then_some((row_boundary, old_save_position))
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub marker: bool,
    pub voiced: bool,
    pub top: f32,
    pub height: f32,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub permission: bool,
    pub captured: bool,
    /// Native pressed/repeated controller bits, supplied by the shared mapper.
    pub pressed: u32,
    pub repeated: u32,
    pub axis: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Move,
    Close,
    ChapterSelector,
    /// Main invokes the voice method even on an unvoiced row. That method
    /// returns immediately unless the row is a voiced ordinary Message.
    ReplayVoice {
        row: usize,
    },
    QuickLoad {
        row: usize,
    },
}
struct Scroll {
    current: f32,
    start: f32,
    target: f32,
    elapsed: f32,
    busy: bool,
}
pub struct Controller {
    rows: Vec<Row>,
    allowed: std::ops::Range<usize>,
    selected: usize,
    minimum: f32,
    maximum: f32,
    scroll: Scroll,
}
impl Controller {
    /// The native builder supplies absolute row positions, allowed range and
    /// initial scroll; preceding rows may have negative coordinates. Preserve
    /// that document origin when adapting replayed history.
    pub fn new(
        rows: Vec<Row>,
        selected: usize,
        allowed: std::ops::Range<usize>,
        scroll: f32,
        minimum: f32,
        maximum: f32,
    ) -> Result<Self, &'static str> {
        if allowed.start >= allowed.end
            || allowed.end > rows.len()
            || !allowed.contains(&selected)
            || !minimum.is_finite()
            || !maximum.is_finite()
            || minimum > maximum
            || !scroll.is_finite()
            || scroll < minimum
            || scroll > maximum
            || rows
                .iter()
                .any(|r| !r.top.is_finite() || !r.height.is_finite() || r.height < 0.0)
        {
            return Err("Ger backlog rows/range/scroll disagree with native builder");
        }
        Ok(Self {
            rows,
            allowed,
            selected,
            minimum,
            maximum,
            scroll: Scroll {
                current: scroll,
                start: scroll,
                target: scroll,
                elapsed: 0.0,
                busy: false,
            },
        })
    }
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.current
    }
    pub fn scroll_busy(&self) -> bool {
        self.scroll.busy
    }
    pub fn allowed(&self) -> std::ops::Range<usize> {
        self.allowed.clone()
    }
    /// Original7FF24 publishes row geometry under the browser mutex while Main
    /// retains its selection and scroll owner. Expanding the measured range
    /// does not retarget or clamp an in-flight scroll, nor a Chapter selection
    /// of an existing row beyond the measured range.
    pub fn update_geometry(&mut self, geometry: &history::Geometry) -> Result<(), &'static str> {
        if geometry.rows.len() != self.rows.len()
            || geometry.allowed.start >= geometry.allowed.end
            || geometry.allowed.end > geometry.rows.len()
            || !geometry.minimum.is_finite()
            || !geometry.maximum.is_finite()
            || geometry.minimum > geometry.maximum
            || geometry.rows.iter().zip(&self.rows).any(|(new, old)| {
                new.marker != old.marker
                    || !new.top.is_finite()
                    || !new.height.is_finite()
                    || new.height < 0.
            })
        {
            return Err("Ger backlog geometry changed metadata identity or has invalid bounds");
        }
        for (row, new) in self.rows.iter_mut().zip(&geometry.rows) {
            row.voiced = new.voice_count != 0;
            row.top = new.top;
            row.height = new.height;
        }
        self.allowed = geometry.allowed.clone();
        self.minimum = geometry.minimum;
        self.maximum = geometry.maximum;
        Ok(())
    }
    /// Pointer callbacks and ChapterSelector8541C supply their own selection
    /// and scroll request. They do not use Main's repeat/cue priority chain.
    pub fn select_external(&mut self, row: usize, scroll: f32, immediate: bool) -> bool {
        if row >= self.rows.len() || !scroll.is_finite() {
            return false;
        }
        self.selected = row;
        let scroll = scroll.clamp(self.minimum, self.maximum);
        if immediate {
            self.scroll.current = scroll;
            self.scroll.busy = false;
        } else {
            self.scroll.start = self.scroll.current;
            self.scroll.target = scroll;
            self.scroll.elapsed = 0.;
            self.scroll.busy = true;
        }
        true
    }
    pub fn select_and_reveal(&mut self, row: usize) -> bool {
        if row >= self.rows.len() {
            return false;
        }
        self.selected = row;
        self.reveal();
        true
    }
    pub fn voice_available(&self, row: usize) -> bool {
        self.rows.get(row).is_some_and(|r| !r.marker && r.voiced)
    }
    fn reveal(&mut self) {
        let row = self.rows[self.selected];
        let relative = row.top - self.scroll.current;
        let target = if relative < 0.0 {
            row.top.max(self.minimum)
        } else if relative + row.height > VIEW_HEIGHT {
            (row.top + row.height - VIEW_HEIGHT)
                .min(self.maximum)
                .max(self.minimum)
        } else {
            self.scroll.current
        };
        self.scroll.start = self.scroll.current;
        self.scroll.target = target;
        self.scroll.elapsed = 0.0;
        self.scroll.busy = true;
    }
    pub fn input(&mut self, input: Input) -> Vec<Event> {
        if !input.permission || input.captured || self.scroll.busy {
            return vec![];
        }
        let old = self.selected;
        let repeat = input.repeated;
        let raw = if repeat & (1 << 24) != 0 {
            old as i64 - 1
        } else if repeat & (1 << 25) != 0 {
            old as i64 + 1
        } else if repeat & (1 << 26) != 0 {
            old as i64 - 4
        } else if repeat & (1 << 27) != 0 {
            old as i64 + 4
        } else if repeat & (1 << 11) != 0 {
            // Native82D28 searches from the actual selected row, including a
            // Chapter/pointer row outside keyboard bounds, and only clamps the
            // result at82D6C. An empty/reversed search must not pre-clamp old.
            self.rows
                .get(old + 1..self.allowed.end)
                .unwrap_or(&[])
                .iter()
                .position(|r| r.marker)
                .map_or(self.allowed.end - 1, |i| old + 1 + i) as i64
        } else if repeat & (1 << 10) != 0 {
            self.rows
                .get(self.allowed.start..old)
                .unwrap_or(&[])
                .iter()
                .rposition(|r| r.marker)
                .map_or(self.allowed.start, |i| self.allowed.start + i) as i64
        } else {
            old as i64
        };
        self.selected = raw.clamp(self.allowed.start as i64, self.allowed.end as i64 - 1) as usize;
        if old != self.selected {
            self.reveal();
            return vec![Event::Move];
        }
        let pressed = input.pressed;
        if pressed & (1 << 29) != 0 {
            return vec![Event::Close];
        }
        if pressed & (1 << 6) != 0 {
            return vec![Event::ChapterSelector];
        }
        if pressed & (1 << 28) != 0 {
            return vec![Event::ReplayVoice { row: self.selected }];
        }
        if pressed & (1 << 7) != 0 {
            return vec![Event::QuickLoad { row: self.selected }];
        }
        if input.axis.is_finite() && input.axis != 0.0 {
            let next = input
                .axis
                .mul_add(32.0, self.scroll.current)
                .clamp(self.minimum, self.maximum);
            //80D30 walks toward a visible row and stops before reversing direction.
            let mut direction = 0;
            loop {
                let row = self.rows[self.selected];
                if row.top < next {
                    if direction == -1 || self.selected + 1 >= self.allowed.end {
                        break;
                    }
                    direction = 1;
                    self.selected += 1;
                } else if row.top + row.height > next + VIEW_HEIGHT {
                    if direction == 1 || self.selected <= self.allowed.start {
                        break;
                    }
                    direction = -1;
                    self.selected -= 1;
                } else {
                    break;
                }
            }
            self.scroll.current = next;
            self.scroll.busy = false;
        }
        vec![]
    }
    ///80C34 passes literal1.0 to the scroll tween each browser parent update,
    /// independent of the incoming animation delta.
    pub fn advance_scroll(&mut self) {
        if self.scroll.busy {
            self.scroll.elapsed += 1.0;
            let t = (self.scroll.elapsed / 4.0).min(1.0);
            self.scroll.current =
                (self.scroll.target - self.scroll.start).mul_add(t, self.scroll.start);
            if t >= 1.0 {
                self.scroll.busy = false;
                self.scroll.current = self.scroll.target;
            }
        }
    }
}
#[cfg(test)]
#[path = "backlog/tests.rs"]
mod tests;
