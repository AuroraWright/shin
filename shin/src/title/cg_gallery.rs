//! Native CGMode list (470e0), controls (49230), selection (497c0), and viewer task (4d180).
//! Live resource, rendering, and child ownership are in cg_gallery_scene.
use crate::title::motion::rate;
use shin_core::{
    format::{
        save::SaveData,
        scenario::info::{PictureBoxInfo, PictureId},
    },
    time::{Ticks, Tween, Tweener},
};

pub(super) struct CgRow {
    pub entry: usize,
    pub thumbnail: String,
    pub pictures: Vec<PictureId>,
    pub unlocked: Vec<bool>,
    pub plate: [f32; 4],
}
impl CgRow {
    pub fn unlocked_count(&self) -> usize {
        self.unlocked.iter().filter(|v| **v).count()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CgRequest {
    OpenRow(usize),
    ReturnTitle { argument: u8, music_stop_ticks: u32 },
}
#[derive(Default)]
pub(super) struct CgControls {
    pub confirm: bool,
    pub cancel: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub page_up: bool,
    pub page_down: bool,
}
#[derive(Clone, Copy)]
enum Task {
    Play,
    Close,
}
pub(super) struct CgGalleryList {
    rows: Vec<CgRow>,
    focus: Vec<Tweener>,
    flashes: Vec<Tweener>,
    selected: usize,
    scroll: Tweener,
    close: Tweener,
    closing: bool,
    prepared: bool,
    tasks: Vec<Task>,
    requests: Vec<CgRequest>,
    sounds: Vec<&'static str>,
    pointer_target: i32,
    pointer_row: i32,
    velocity: f32,
}
fn flash(clock: &mut Tweener) {
    rate(clock, 0.7, 0.2);
    clock.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
}
impl CgGalleryList {
    pub fn new(info: &PictureBoxInfo, save: &SaveData) -> anyhow::Result<Self> {
        anyhow::ensure!(!info.is_empty(), "CG gallery has no authored entries");
        let rows = info
            .iter()
            .enumerate()
            .map(|(entry, item)| CgRow {
                entry,
                thumbnail: item.name.as_str().to_owned(),
                pictures: item.picture_ids.0.iter().copied().collect(),
                unlocked: item
                    .picture_ids
                    .0
                    .iter()
                    .map(|id| {
                        save.save_vectors
                            .unlocked_cgs
                            .contains(id.into_i32() as usize)
                    })
                    .collect(),
                plate: [
                    (entry % 4) as f32 * 412.0,
                    (entry / 4) as f32 * 242.0,
                    412.0,
                    242.0,
                ],
            })
            .collect::<Vec<_>>();
        let focus = (0..rows.len())
            .map(|i| Tweener::new(if i == 0 { 1.0 } else { 0.0 }))
            .collect();
        let flashes = (0..rows.len()).map(|_| Tweener::new(0.0)).collect();
        Ok(Self {
            rows,
            focus,
            flashes,
            selected: 0,
            scroll: Tweener::new(0.0),
            close: Tweener::new(0.0),
            closing: false,
            prepared: false,
            tasks: Vec::new(),
            requests: Vec::new(),
            sounds: Vec::new(),
            pointer_target: -1,
            pointer_row: -1,
            velocity: 0.0,
        })
    }
    pub fn rows(&self) -> &[CgRow] {
        &self.rows
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn scroll(&self) -> f32 {
        self.scroll.value()
    }
    pub fn content_height(&self) -> f32 {
        (self.rows.len().div_ceil(4) as f32 * 242.0).max(726.0)
    }
    pub fn all_unlocked(&self) -> bool {
        self.rows
            .iter()
            .all(|r| r.unlocked_count() == r.pictures.len())
    }
    pub fn completion_tenths(&self) -> usize {
        let total = self.rows.iter().map(|r| r.pictures.len()).sum::<usize>();
        // AArch64 SDIV by zero yields zero; empty variant lists still have rows.
        if total == 0 {
            0
        } else {
            self.rows.iter().map(CgRow::unlocked_count).sum::<usize>() * 1000 / total
        }
    }
    pub fn completion_label(&self) -> String {
        let n = self.completion_tenths();
        if n >= 1000 {
            "達成率:100％".into()
        } else {
            format!("達成率:{}.{}％", n / 10, n % 10)
        }
    }
    pub fn feedback(&self, index: usize) -> (f32, f32) {
        (self.focus[index].value(), self.flashes[index].value())
    }
    pub fn close_flash(&self) -> f32 {
        self.close.value()
    }
    pub fn closing(&self) -> bool {
        self.closing
    }
    ///496c8/496d0 creates a generic blocking task for deferred viewer opening.
    /// Query before prepare_frame: its completion poll still blocks input.
    pub fn has_blocking_tasks(&self) -> bool {
        !self.tasks.is_empty()
    }
    pub fn controller_ready(&self) -> bool {
        !self.closing && self.scroll.is_idle()
    }
    pub fn prepare_frame(&mut self) {
        if self.prepared {
            return;
        }
        self.prepared = true;
        let mut keep = Vec::new();
        for task in std::mem::take(&mut self.tasks).into_iter().rev() {
            match task {
                Task::Play if self.flashes[self.selected].is_idle() => self
                    .requests
                    .push(CgRequest::OpenRow(self.rows[self.selected].entry)),
                Task::Close if self.close.is_idle() => self.requests.push(CgRequest::ReturnTitle {
                    argument: 6,
                    music_stop_ticks: 30,
                }),
                _ => keep.push(task),
            }
        }
        keep.reverse();
        self.tasks = keep;
    }
    pub fn update(&mut self, delta: Ticks) {
        self.prepare_frame();
        // 480b8 integrates signed time before the positive-only feedback clocks.
        if self.velocity != 0.0 {
            let next = self.clamp_scroll(self.scroll() - self.velocity * delta.as_f32());
            if (next - self.scroll()).abs() < f32::EPSILON {
                self.velocity = 0.0;
            } else {
                self.scroll_to(next);
                self.velocity *= 0.95_f32.powf(delta.as_f32());
                if self.velocity.abs() < 0.1 {
                    self.velocity = 0.0;
                }
            }
        }
        if delta > Ticks::ZERO {
            for clock in self
                .focus
                .iter_mut()
                .chain(&mut self.flashes)
                .chain([&mut self.scroll, &mut self.close])
            {
                clock.update(delta);
            }
        }
        self.prepared = false;
    }
    pub fn select(&mut self, index: usize, reveal: bool) {
        if index >= self.rows.len() {
            return;
        }
        rate(&mut self.focus[self.selected], 0.0, 0.2);
        self.selected = index;
        rate(&mut self.focus[index], 1.0, 0.25);
        if reveal {
            let y = self.rows[index].plate[1];
            let bottom = y + 242.0;
            let scroll = self.scroll();
            let target = if y < scroll {
                y.max(0.0)
            } else if bottom - scroll > 726.0 {
                (bottom - 726.0).min(self.content_height() - 726.0)
            } else {
                scroll
            };
            // Even an unchanged target queues the native four-tick controller gate.
            self.scroll
                .enqueue_now(target, Tween::linear(Ticks::from_f32(4.0)));
            self.velocity = 0.0;
        }
    }
    pub fn navigate(&mut self, column: i32, row: i32) {
        if !self.controller_ready() {
            return;
        }
        let x = (self.selected as i32 % 4 + column).clamp(0, 3);
        let y = (self.selected as i32 / 4 + row).clamp(0, self.rows.len().div_ceil(4) as i32 - 1);
        let next = (y as usize * 4 + x as usize).min(self.rows.len() - 1);
        if next != self.selected {
            self.sounds.push("move");
            self.select(next, true);
        }
    }
    /// Native 48230 retains the selected column while moving focus into the viewport.
    pub fn analog_scroll(&mut self, axis: f32) {
        if !self.controller_ready() || !axis.is_finite() || axis == 0.0 {
            return;
        }
        self.scroll_to(self.clamp_scroll(self.scroll() + axis * 32.0));
        self.velocity = 0.0;
    }
    fn clamp_scroll(&self, value: f32) -> f32 {
        value.clamp(0.0, self.content_height() - 726.0)
    }
    fn scroll_to(&mut self, scroll: f32) {
        let mut candidate = self.selected as i32;
        while ((candidate / 4 * 242) as f32) < scroll {
            candidate += 4;
        }
        while (candidate / 4 * 242 + 242) as f32 > scroll + 726.0 {
            candidate -= 4;
        }
        if candidate >= 0
            && (candidate as usize) < self.rows.len()
            && candidate as usize != self.selected
        {
            self.select(candidate as usize, false);
        }
        self.scroll.enqueue_now(scroll, Tween::IMMEDIATE);
    }
    pub fn velocity(&self) -> f32 {
        self.velocity
    }
    /// 49b00: -1 outside the viewport, 0 beyond the authored tail, ordinal+1 on a plate.
    pub fn pointer_hit(&self, point: glam::Vec2) -> i32 {
        if !(136.0..1784.0).contains(&point.x) || !(190.0..916.0).contains(&point.y) {
            return -1;
        }
        let x = point.x - 136.0;
        let y = point.y - 190.0 + self.scroll();
        self.rows
            .iter()
            .find_map(|row| {
                let [left, top, width, height] = row.plate;
                ((left..left + width).contains(&x) && (top..top + height).contains(&y))
                    .then_some(row.entry as i32 + 1)
            })
            .unwrap_or(0)
    }
    pub fn pointer_probe(&mut self, point: glam::Vec2) -> bool {
        self.prepare_frame();
        if self.closing {
            return false;
        }
        self.pointer_row = -1;
        self.pointer_target =
            if (1764.0..1848.0).contains(&point.x) && (32.0..116.0).contains(&point.y) {
                1
            } else if (150.0..992.0).contains(&point.y) {
                -1
            } else {
                0
            };
        if self.pointer_target < 0 {
            self.pointer_row = self.pointer_hit(point);
            if self.pointer_row < 0 {
                return false;
            }
            if self.velocity.abs() >= f32::EPSILON {
                // 49d14 clears the contiguous row and velocity words together.
                self.pointer_row = 0;
                self.velocity = 0.0;
            }
        }
        true
    }
    pub fn pointer_reset(&mut self) {
        self.pointer_target = -1;
        self.pointer_row = -1;
    }
    pub fn pointer_press(&mut self) {
        if self.pointer_target == 1 {
            rate(&mut self.close, 0.7, 0.2);
        } else if self.pointer_target < 0 && self.pointer_row > 0 {
            let index = self.pointer_row as usize - 1;
            rate(&mut self.flashes[index], 0.7, 0.2);
            self.select(index, true);
        }
    }
    pub fn pointer_release(&mut self) {
        if self.pointer_target == 1 {
            self.sounds.push("decide");
            self.cancel();
        } else if self.pointer_target < 0 && self.pointer_row > 0 {
            self.select(self.pointer_row as usize - 1, true);
            self.play();
        }
    }
    pub fn pointer_cancel(&mut self) {
        if self.pointer_target >= 0 {
            rate(&mut self.close, 0.0, 0.2);
            self.pointer_target = 0;
        } else if self.pointer_row > 0 {
            rate(&mut self.flashes[self.pointer_row as usize - 1], 0.0, 0.2);
        }
    }
    pub fn pointer_drag(&mut self, dy: f32) {
        if self.pointer_target >= 0 {
            self.pointer_cancel();
        } else if self.pointer_row >= 0 && dy.is_finite() {
            if self.pointer_row > 0 {
                rate(&mut self.flashes[self.pointer_row as usize - 1], 0.0, 0.2);
                self.pointer_row = 0;
            }
            self.scroll_to(self.clamp_scroll(self.scroll() - dy));
        }
    }
    pub fn pointer_gesture(&mut self, velocity: glam::Vec2) {
        if self.pointer_target < 0 && self.pointer_row >= 0 && velocity.y.is_finite() {
            self.velocity = velocity.y;
        }
    }
    pub fn controller(&mut self, c: CgControls, axis: f32) {
        self.prepare_frame();
        if !self.controller_ready() {
            return;
        }
        if c.confirm {
            self.play();
            return;
        }
        if c.cancel {
            self.sounds.push("cancel");
            self.cancel();
            return;
        }
        let x = if c.left { -1 } else { i32::from(c.right) };
        let y = if c.up {
            -1
        } else if c.down {
            1
        } else if c.page_up {
            -3
        } else if c.page_down {
            3
        } else {
            0
        };
        if x != 0 || y != 0 {
            self.navigate(x, y);
        } else {
            self.analog_scroll(axis);
        }
    }
    pub fn play(&mut self) {
        self.prepare_frame();
        flash(&mut self.flashes[self.selected]);
        if self.rows[self.selected].unlocked_count() > 0 {
            self.sounds.push("decide");
            self.tasks.push(Task::Play);
        } else {
            self.sounds.push("error");
        }
    }
    pub fn cancel(&mut self) {
        self.prepare_frame();
        self.closing = true;
        flash(&mut self.close);
        self.tasks.push(Task::Close);
    }
    pub fn take_requests(&mut self) -> Vec<CgRequest> {
        std::mem::take(&mut self.requests)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
}

#[cfg(test)]
mod tests;
#[allow(dead_code)]
mod input;
#[cfg(test)]
mod render_reference;
pub(super) mod scene;
#[allow(dead_code)]
pub(super) mod text;
#[allow(dead_code)]
mod view;
