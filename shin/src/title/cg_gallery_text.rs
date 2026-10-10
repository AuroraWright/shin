//! CGMode completion label47a3c..47b8c, SimpleText style1.
use super::cg_gallery::CgGalleryList;
use crate::{
    adv::DialogText,
    asset::{font::GpuFontLazy, system::AssetServer},
};
use glam::{Mat4, vec3};
use shin_core::primitives::color::FloatColor4;
use shin_render::{ColorBlendType, RenderRequestBuilder, render_pass::RenderPass};
use std::sync::Arc;

pub(super) struct CgCompletionText {
    text: DialogText,
}
impl CgCompletionText {
    pub fn new(server: &AssetServer, font: &Arc<GpuFontLazy>, list: &CgGalleryList) -> Self {
        Self {
            text: DialogText::load_simple_text(server, font, &list.completion_label(), 40.0, 640.0),
        }
    }
    #[cfg(test)]
    pub(super) fn reference_text(&self) -> &DialogText {
        &self.text
    }
    pub fn render(&self, pass: &mut RenderPass) {
        self.text.render_with_builder(
            pass,
            crate::render::top_left_projection_matrix()
                * Mat4::from_translation(vec3(48.0, 1012.0, 0.0)),
            FloatColor4::WHITE,
            FloatColor4::from_rgba(47.0 / 255.0, 25.0 / 255.0, 20.0 / 255.0, 1.0),
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
        );
    }
}
