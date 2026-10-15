//! Non-drawing45338/45424 input owner. Artwork lives in the scene layer.
use super::view::View;
use crate::{
    adv::higurashi::{
        input::{Buttons, OverlayInput},
        touch::Overlay,
    },
    layer::higurashi_properties::HigurashiProperties,
};
use anyhow::Result;
use glam::Vec2;
use shin_core::time::Ticks;

pub struct Wrapper {
    view: View,
    properties: HigurashiProperties,
}
impl Wrapper {
    pub fn new(view: View, seeds: [u32; 4]) -> Result<Self> {
        let mut properties = HigurashiProperties::from_seeds(seeds);
        properties.control(5, 1.0, Ticks::ZERO, 0)?;
        Ok(Self { view, properties })
    }
}
impl Overlay for Wrapper {
    fn identity(&self) -> usize {
        std::sync::Arc::as_ptr(&self.view.model) as usize
    }
    fn input_permissions(&self) -> OverlayInput {
        self.view.model.lock().permissions()
    }
    fn probe(&mut self, position: Vec2) -> bool {
        self.view.model.lock().probe(position)
    }
    fn press(&mut self, position: Vec2) {
        self.view.model.lock().press(position);
    }
    fn release(&mut self, position: Vec2) {
        self.view.model.lock().release(position);
    }
    fn cancel(&mut self) {}
    fn input(&mut self, permission: bool, buttons: Buttons) {
        self.view.model.lock().input(permission, buttons);
    }
    fn advance(&mut self, _delta: Ticks) {
        self.view.model.lock().poll_phase();
    }
    // Native wrapper virtual10 is base67c78; only the scene layer440b8
    // fast-forwards character clocks. Conceal/draw methods remain inert.
    fn fast_forward(&mut self) {
        self.properties.fast_forward();
    }
    fn take_cues(&mut self) -> Vec<&'static str> {
        self.view.model.lock().take_cues()
    }
    fn priority(&self) -> f32 {
        self.properties.values()[5]
    }
}
