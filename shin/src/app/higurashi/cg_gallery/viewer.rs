//! Updated Viewer3f3d0/3f634/3fe60/3ff70, independent of the gallery grid.
//! A concrete owner must own real async Picture14a8 resources, scoped retirement,
//! capture textures and the private mixed scope. This model never auto-completes
//! the gallery's OpenViewer child or reduces complete outer GameData owners.
use super::Entry;
use crate::{
    adv::higurashi::{input::Buttons, reconstruction_mode::OverlayClock},
    layer::higurashi_properties::HigurashiProperties,
};
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_core::time::Ticks;
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Input {
    pub buttons: Buttons,
    pub first_stick: [i8; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Metadata {
    pub origin: [i16; 2],
    pub extent: [u16; 2],
}
/// A ready original Picture owner is constructed on the worker AFTER opening
/// its file and BEFORE header/block reads, with exactly four fresh OS seeds.
/// The service must retain matching art under this stable identity. No second
/// properties owner may advance these clocks while this model owns them.
pub(super) struct Picture {
    pub identity: u64,
    pub metadata: Metadata,
    pub properties: HigurashiProperties,
}
impl Picture {
    #[cfg(test)]
    pub fn from_native_metadata(identity: u64, metadata: Metadata, seeds: [u32; 4]) -> Self {
        Self {
            identity,
            metadata,
            properties: HigurashiProperties::from_seeds(seeds),
        }
    }
    fn bounds(&self) -> [f32; 4] {
        // Actual67134/67158 use literal274d0=1920/274e0=1080, not the physical
        // output extent; preserve negative-zero origin and operation order.
        let sx = 1280f32 / 1920.;
        let sy = 720f32 / 1080.;
        [
            -sx * self.metadata.origin[0] as f32,
            -sy * self.metadata.origin[1] as f32,
            sx * self.metadata.extent[0] as f32,
            sy * self.metadata.extent[1] as f32,
        ]
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cue {
    Decide,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Input,
    Ready,
    CaptureWait,
    CloseWait,
}
pub(super) struct Task {
    pub kind: Kind,
    pub done: bool,
}
pub(super) struct Model {
    pub entry: Entry,
    pub variant: i32,
    pub closing: bool,
    pub show_info: bool,
    pub fit: bool,
    pub velocity: Vec2,
    pub offset: Vec2,
    pub picture: Option<Picture>,
    pub capture: OverlayClock,
    pending: Vec<Kind>,
}
pub(super) trait Services {
    /// Start/replace the scoped worker. Old prepared work/job ownership must
    /// retire exactly; this call must not synthesize a ready Picture.
    fn prepare_picture(&mut self, id: u16) -> Result<()>;
    fn worker_status(&mut self) -> u32;
    fn take_prepared(&mut self) -> Result<Picture>;
    /// Snapshot the REAL composed gallery including this Viewer with badges
    /// temporarily hidden. The old Picture is still live during this callback.
    /// Replacement texture lifetime belongs to the concrete owner, not this
    /// metadata clock. Capture failure must propagate before publication.
    fn capture_parent(&mut self, model: &Model) -> Result<()>;
    /// Install the staged capture only after old live art retires and Ready
    /// applies the new bounds (or close clears its live Picture).
    fn publish_capture(&mut self, model: &Model) -> Result<()>;
    fn input(&mut self, model: &mut Model, permission: bool) -> Result<()>;
    fn cue(&mut self, cue: Cue) -> Result<()>;
    fn disable_input(&mut self);
}
impl Model {
    pub fn new(entry: Entry, services: &mut dyn Services) -> Result<Self> {
        ensure!(
            !entry.pictures.is_empty()
                && entry.pictures.len() < 32
                && entry.pictures.len() == entry.unlocked.len()
                && entry.unlocked.iter().any(|x| *x),
            "Viewer requires the original reachable unlocked group and low5 variant-count range"
        );
        let mut s = Self {
            entry,
            variant: -1,
            closing: false,
            show_info: true,
            fit: false,
            velocity: Vec2::ZERO,
            offset: Vec2::ZERO,
            picture: None,
            capture: OverlayClock::captured(false),
            pending: vec![Kind::Input],
        };
        ensure!(
            s.next(services)?,
            "Original unlocked group has no viewable variant"
        );
        Ok(s)
    }
    pub fn take_pending_tasks(&mut self) -> Vec<Task> {
        std::mem::take(&mut self.pending)
            .into_iter()
            .map(|kind| Task { kind, done: false })
            .collect()
    }
    fn next(&mut self, services: &mut dyn Services) -> Result<bool> {
        self.velocity = Vec2::ZERO;
        let mut next = self.variant + 1;
        while (next as usize) < self.entry.pictures.len() && !self.entry.unlocked[next as usize] {
            next += 1;
        }
        if next as usize >= self.entry.pictures.len() {
            self.variant = -1;
            return Ok(false);
        }
        self.variant = next;
        services.prepare_picture(self.entry.pictures[next as usize])?;
        self.pending.push(Kind::Ready);
        Ok(true)
    }
    fn close(&mut self, services: &mut dyn Services) -> Result<()> {
        self.closing = true;
        self.show_info = false;
        services.capture_parent(self)?;
        self.picture = None;
        services.publish_capture(self)?;
        self.capture = OverlayClock::captured(true);
        self.capture.clear(30);
        self.pending.push(Kind::CloseWait);
        Ok(())
    }
    fn set_property(&mut self, index: usize, value: f32, animated: bool) {
        let p = &mut self
            .picture
            .as_mut()
            .expect("Native3ff70 requires a live Picture")
            .properties;
        if animated {
            p.control(index, value, Ticks::from_f32(10.), 0)
                .expect("Original Viewer ten-frame tween");
        } else {
            p.tweener_mut(index).unwrap().fast_forward_to(value);
        }
    }
    fn apply_bounds(&mut self, animated: bool) {
        let [left, top, width, height] = self
            .picture
            .as_ref()
            .expect("Native Viewer bound owner")
            .bounds();
        if self.fit {
            let a = width / 1280.;
            let b = height / 720.;
            let ratio = if a > b { a } else { b };
            let scale = 1000. / ratio;
            self.set_property(12, scale, animated);
            self.set_property(13, scale, animated);
            self.set_property(0, 0., animated);
            self.set_property(1, 0., animated);
        } else {
            let min_x = (left + 640.).min(0.);
            if self.offset.x < min_x {
                self.offset.x = min_x;
            } else {
                let max_x = ((left + width) + (-640.)).max(0.);
                if self.offset.x > max_x {
                    self.offset.x = max_x;
                }
            }
            let min_y = (top + 720.).min(0.);
            if self.offset.y < min_y {
                self.offset.y = min_y;
            } else {
                let max_y = (top + height).max(0.);
                if self.offset.y > max_y {
                    self.offset.y = max_y;
                }
            }
            self.set_property(0, self.offset.x, animated);
            self.set_property(1, self.offset.y, animated);
            self.set_property(12, 1000., animated);
            self.set_property(13, 1000., animated);
        }
    }
    fn pan_allowed(&self) -> bool {
        self.picture
            .as_ref()
            .is_some_and(|p| p.properties.tweener(12).unwrap().is_idle())
            && !self.fit
    }
    pub fn controls(
        &mut self,
        permission: bool,
        captured: bool,
        input: Input,
        services: &mut dyn Services,
    ) -> Result<bool> {
        if !permission {
            services.disable_input();
        } else if !captured {
            let pressed = input.buttons.pressed;
            if pressed & (1 << 25) != 0 {
                services.cue(Cue::Cancel)?;
                self.close(services)?;
            } else if pressed & (1 << 24) != 0 {
                services.cue(Cue::Decide)?;
                if !self.next(services)? {
                    self.close(services)?;
                }
            } else if pressed & (1 << 12) != 0 {
                if self.picture.is_some() {
                    self.fit = !self.fit;
                    self.apply_bounds(true);
                }
                self.velocity = Vec2::ZERO;
            } else {
                if pressed & (1 << 15) != 0 {
                    self.show_info = !self.show_info;
                }
                if self.pan_allowed() {
                    let held = input.buttons.held;
                    let mut dx = if held & (1 << 7) != 0 {
                        16
                    } else if held & (1 << 5) != 0 {
                        -16
                    } else {
                        0
                    };
                    let mut dy = if held & (1 << 4) != 0 {
                        -16
                    } else if held & (1 << 6) != 0 {
                        16
                    } else {
                        0
                    };
                    if dx == 0 && dy == 0 {
                        dx = -(i32::from(input.first_stick[0]) / 10);
                        dy = i32::from(input.first_stick[1]) / 10;
                    }
                    if dx != 0 || dy != 0 {
                        self.velocity = Vec2::ZERO;
                        self.offset.x += dx as f32;
                        self.offset.y += dy as f32;
                    }
                    self.apply_bounds(false);
                }
            }
        }
        Ok(!self.closing)
    }
    /// Original3fe60 runs after the private scope. Inertia applies one position
    /// step even delta0, then pow(.95,delta) decay; Picture clocks and capture
    /// clock advance once here, including a just-published worker owner.
    pub fn advance_clocks(&mut self, delta: Ticks) {
        if self.velocity.x != 0. || self.velocity.y != 0. {
            if self.pan_allowed() {
                self.offset.x += self.velocity.x;
                self.offset.y -= self.velocity.y;
                let decay = f32::from_bits(0x3f733333).powf(delta.as_f32());
                self.velocity.x *= decay;
                self.velocity.y *= decay;
                self.apply_bounds(false);
            } else {
                self.velocity = Vec2::ZERO;
            }
        }
        if let Some(p) = &mut self.picture {
            p.properties.advance(delta);
        }
        self.capture.advance(delta);
    }
    pub fn probe_contact(&mut self, _position: Vec2) -> bool {
        self.velocity = Vec2::ZERO;
        true
    }
    pub fn release_contact(&mut self, services: &mut dyn Services) -> Result<()> {
        services.cue(Cue::Decide)?;
        if !self.next(services)? {
            self.close(services)?;
        }
        Ok(())
    }
    pub fn drag_contact(&mut self, old: Vec2, new: Vec2) {
        if self.picture.is_some() && !self.fit {
            let dx = (new.x - old.x) * f32::from_bits(0x3f2aaaab);
            let dy = (new.y - old.y) * (-0.375);
            self.offset.x += dx;
            self.offset.y += dy;
            self.apply_bounds(false);
        }
    }
    pub fn gesture_contact(&mut self, velocity: Vec2) {
        if self.picture.is_some() && !self.fit {
            self.velocity = Vec2::new(velocity.x * f32::from_bits(0x3f2aaaab), velocity.y * 0.375);
        }
    }
}
impl Task {
    pub fn poll_native(
        &mut self,
        model: &mut Model,
        delta: Ticks,
        permission: bool,
        services: &mut dyn Services,
    ) -> Result<Vec<Task>> {
        match self.kind {
            Kind::Input => {
                services.input(model, permission)?;
                self.done = model.closing;
            }
            Kind::Ready => {
                if services.worker_status() == 2 {
                    model.show_info = false;
                    services.capture_parent(model)?;
                    model.show_info = true;
                    model.picture = Some(services.take_prepared()?);
                    model.apply_bounds(false);
                    services.publish_capture(model)?;
                    model.capture = OverlayClock::captured(true);
                    model.capture.clear(30);
                    model.pending.push(Kind::CaptureWait);
                    self.done = true;
                }
            }
            Kind::CaptureWait | Kind::CloseWait => {
                self.done = !model.capture.busy();
            }
        }
        let _ = delta;
        Ok(model.take_pending_tasks())
    }
}
#[cfg(test)]
mod model_tests;
pub(super) mod owner;
pub(super) mod view;
