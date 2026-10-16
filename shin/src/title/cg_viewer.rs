//! Native CG viewer variant cursor (4ab80), picture placement (4b2a0), and pan (4b190).
//! Picture/capture ownership is in cg_viewer_scene; cg_gallery_scene integrates it into the live Title route.
use super::cg_gallery::CgRow;
use glam::{Vec2, Vec4};
use shin_core::{
    format::scenario::info::PictureId,
    time::{Ticks, Tween, Tweener},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ViewerRequest {
    Load { ordinal: usize, picture: PictureId },
    Close,
}
#[derive(Default)]
pub(super) struct ViewerControls {
    pub cancel: bool,
    pub confirm: bool,
    pub fit_toggle: bool,
    pub indicator_toggle: bool,
    /// Displacement already mapped by the native controller: logical X/Y pan.
    pub pan: Vec2,
}
pub(super) struct CgViewerNavigation {
    ordinal: i32,
    closing: bool,
    fit: bool,
    indicators: bool,
    dimensions: Option<Vec2>,
    position: Vec2,
    velocity: Vec2,
    x: Tweener,
    y: Tweener,
    scale_x: Tweener,
    scale_y: Tweener,
    requests: Vec<ViewerRequest>,
    sounds: Vec<&'static str>,
}
impl CgViewerNavigation {
    pub fn new(row: &CgRow) -> Self {
        let mut this = Self {
            ordinal: -1,
            closing: false,
            fit: false,
            indicators: true,
            dimensions: None,
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            x: Tweener::new(0.0),
            y: Tweener::new(0.0),
            scale_x: Tweener::new(1000.0),
            scale_y: Tweener::new(1000.0),
            requests: Vec::new(),
            sounds: Vec::new(),
        };
        this.next(row);
        this
    }
    pub fn ordinal(&self) -> i32 {
        self.ordinal
    }
    pub fn closing(&self) -> bool {
        self.closing
    }
    pub fn fit(&self) -> bool {
        self.fit
    }
    pub fn indicators(&self) -> bool {
        self.indicators
    }
    pub fn position(&self) -> Vec2 {
        self.position
    }
    pub fn velocity(&self) -> Vec2 {
        self.velocity
    }
    /// Layer translation and scale in native property units (1000 = 1x).
    pub fn transform(&self) -> Vec4 {
        Vec4::new(
            self.x.value(),
            self.y.value(),
            self.scale_x.value(),
            self.scale_y.value(),
        )
    }
    pub fn pan_ready(&self) -> bool {
        self.dimensions.is_some() && self.scale_x.is_idle() && !self.fit
    }
    /// Parent rows are supplied live: native 4ab80 rereads the gallery selection.
    fn next(&mut self, row: &CgRow) -> bool {
        self.velocity = Vec2::ZERO;
        let start = (self.ordinal + 1).max(0) as usize;
        if let Some(ordinal) = (start..row.pictures.len()).find(|i| row.unlocked[*i]) {
            self.ordinal = ordinal as i32;
            self.requests.push(ViewerRequest::Load {
                ordinal,
                picture: row.pictures[ordinal],
            });
            true
        } else {
            self.ordinal = -1;
            false
        }
    }
    fn close(&mut self) {
        self.closing = true;
        self.indicators = false;
        self.dimensions = None;
        self.requests.push(ViewerRequest::Close);
    }
    /// 4dd54 preserves logical pan and fit mode across picture publications.
    pub fn publish_picture(&mut self, dimensions: Vec2) {
        assert!(dimensions.is_finite() && dimensions.min_element() > 0.0);
        self.dimensions = Some(dimensions);
        self.indicators = true;
        self.place(false);
    }
    fn place(&mut self, animate: bool) {
        let Some(dimensions) = self.dimensions else {
            return;
        };
        let (translation, scale) = if self.fit {
            (
                Vec2::ZERO,
                1000.0 / (dimensions.x / 1920.0).max(dimensions.y / 1080.0),
            )
        } else {
            let limit = (dimensions * 0.5 - Vec2::new(960.0, 540.0)).max(Vec2::ZERO);
            self.position = self.position.clamp(-limit, limit);
            (Vec2::new(self.position.x, -self.position.y), 1000.0)
        };
        for (clock, value) in [
            (&mut self.x, translation.x),
            (&mut self.y, translation.y),
            (&mut self.scale_x, scale),
            (&mut self.scale_y, scale),
        ] {
            if animate {
                clock.enqueue(value, Tween::linear(Ticks::from_f32(10.0)));
            } else {
                clock.fast_forward_to(value);
            }
        }
    }
    pub fn controller(&mut self, row: &CgRow, c: ViewerControls) {
        if self.closing {
            return;
        }
        if c.cancel {
            self.sounds.push("cancel");
            self.close();
            return;
        }
        if c.confirm {
            self.sounds.push("decide");
            if !self.next(row) {
                self.close();
            }
            return;
        }
        if c.fit_toggle {
            if self.dimensions.is_some() {
                self.fit = !self.fit;
                self.place(true);
            }
            self.velocity = Vec2::ZERO;
            return;
        }
        if c.indicator_toggle {
            self.indicators = !self.indicators;
        }
        if self.pan_ready() && c.pan.is_finite() {
            if c.pan != Vec2::ZERO {
                self.position += c.pan;
                self.velocity = Vec2::ZERO;
            }
            self.place(false);
        }
    }
    pub fn pointer_probe(&mut self) -> bool {
        self.velocity = Vec2::ZERO;
        true
    }
    pub fn pointer_release(&mut self, row: &CgRow) {
        if !self.closing {
            self.sounds.push("decide");
            if !self.next(row) {
                self.close();
            }
        }
    }
    pub fn pointer_drag(&mut self, delta: Vec2) {
        // 4bcb0 gates on picture presence and fit mode, not the scale tween.
        if self.dimensions.is_some() && !self.fit && delta.is_finite() {
            self.position += Vec2::new(delta.x, -delta.y * 0.5625);
            self.place(false);
        }
    }
    pub fn pointer_gesture(&mut self, velocity: Vec2) {
        if self.dimensions.is_some() && !self.fit && velocity.is_finite() {
            self.velocity = Vec2::new(velocity.x, velocity.y * 0.5625);
        }
    }
    pub fn update(&mut self, delta: Ticks) {
        if self.velocity != Vec2::ZERO {
            if self.pan_ready() {
                // Native pan integrates one velocity step per frame, without delta.
                self.position += Vec2::new(self.velocity.x, -self.velocity.y);
                self.velocity *= 0.95_f32.powf(delta.as_f32());
                self.place(false);
            } else {
                self.velocity = Vec2::ZERO;
            }
        }
        if delta > Ticks::ZERO {
            for clock in [
                &mut self.x,
                &mut self.y,
                &mut self.scale_x,
                &mut self.scale_y,
            ] {
                clock.update(delta);
            }
        }
    }
    pub fn take_requests(&mut self) -> Vec<ViewerRequest> {
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
#[allow(dead_code)]
mod motion;
#[allow(dead_code)]
pub(super) mod scene;
#[allow(dead_code)]
pub(super) mod view;
