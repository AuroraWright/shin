//! Native probe3be00, detailed selector hit3b450, press3c040,
//! release3c0f0, drag3c240 and cancel3c360. cd810/cd840 are identity helpers.
use super::*;
use glam::Vec2;

fn rect(point: Vec2, x: f32, y: f32, size: f32) -> bool {
    point.x >= x && point.x < x + size && point.y >= y && point.y < y + size
}
fn rate(clock: &mut Tweener, value: f32) {
    let ticks = (value - clock.value()).abs() / 0.2;
    clock.enqueue_now(value, Tween::linear(Ticks::from_f32(ticks)));
}
impl BustupMenu {
    /// The whole background captures contacts, including outside the menu band.
    pub fn pointer_probe(&mut self, catalog: &BustupCatalog, point: Vec2) -> bool {
        self.pointer_selector = -1;
        if point.y >= 32.0 && point.y < 116.0 {
            if point.x >= 1764.0 && point.x < 1848.0 {
                self.pointer_target = 1;
                return true;
            }
            if point.x >= 1668.0 && point.x < 1752.0 {
                self.pointer_target = 2;
                return true;
            }
        }
        self.pointer_target = if point.y >= 162.0 && point.y < 980.0 {
            -1
        } else {
            0
        };
        if self.pointer_target == -1 {
            self.pointer_selector = self.selector_hit(catalog, point);
        }
        true
    }
    pub(super) fn selector_hit(&self, catalog: &BustupCatalog, point: Vec2) -> i32 {
        let (c, o, p, e) = self.selection.indices();
        let indices = [c, o, p, e, self.selection.background_index()];
        let counts = [
            catalog.characters.len(),
            catalog.characters[c].outfits.len(),
            catalog.characters[c].outfits[o].poses.len(),
            catalog.characters[c].outfits[o].poses[p].expressions.len(),
            catalog.backgrounds.len(),
        ];
        // Labels win first, then enabled left/right arrows, in authored row order.
        for row in 0..5 {
            let (x, y, size) = match row {
                0 => (1504.0, 248.0, 144.0),
                4 => (1504.0, 728.0, 144.0),
                _ => (1520.0, 280.0 + row as f32 * 112.0, 112.0),
            };
            if rect(point, x, y, size) {
                return row as i32 * 3 + 2;
            }
            let x = if row == 0 || row == 4 { 1424.0 } else { 1440.0 };
            let y = if row == 4 {
                760.0
            } else {
                280.0 + row as f32 * 112.0 + if row == 0 { 0.0 } else { 16.0 }
            };
            if indices[row] > 0 && rect(point, x, y, 80.0) {
                return row as i32 * 3;
            }
            let x = if row == 0 || row == 4 { 1648.0 } else { 1632.0 };
            if indices[row] + 1 < counts[row] && rect(point, x, y, 80.0) {
                return row as i32 * 3 + 1;
            }
        }
        -1
    }
    pub fn pointer_reset(&mut self) {
        self.pointer_target = -1;
        self.pointer_selector = -1;
    }
    pub fn pointer_press(&mut self) {
        match self.pointer_target {
            1 => rate(&mut self.close, 0.7),
            2 => rate(&mut self.confirm, 0.7),
            target if target < 0 && self.pointer_selector >= 0 => {
                let row = self.pointer_selector as usize / 3;
                let part = self.pointer_selector as usize % 3;
                self.focus = row;
                if part < 2 {
                    rate(&mut self.arrows[row * 2 + part], 0.7);
                }
            }
            _ => {}
        }
    }
    pub fn pointer_release(&mut self, catalog: &BustupCatalog) {
        match self.pointer_target {
            1 => self.begin_transition(Transition::Close, "decide"),
            2 => self.begin_transition(Transition::Viewer, "decide"),
            target if target < 0 && self.pointer_selector >= 0 => {
                let row = self.pointer_selector as usize / 3;
                let part = self.pointer_selector as usize % 3;
                self.focus = row;
                if part < 2 && self.change(catalog, if part == 0 { -1 } else { 1 }) {
                    flash(&mut self.arrows[row * 2 + part]);
                }
            }
            _ => {}
        }
    }
    /// Drag/cancel fade highlights and invalidate the captured target. Gesture
    /// and long-press callbacks are no-ops; release coordinates are ignored.
    pub fn pointer_cancel(&mut self) {
        if self.pointer_target >= 0 {
            rate(&mut self.close, 0.0);
            rate(&mut self.confirm, 0.0);
            self.pointer_target = 0;
        } else {
            if self.pointer_selector >= 0 {
                let part = self.pointer_selector as usize % 3;
                if part < 2 {
                    rate(&mut self.arrows[self.focus * 2 + part], 0.0);
                }
            }
            self.pointer_selector = -1;
        }
    }
}
