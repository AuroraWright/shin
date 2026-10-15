//! Shared original6aaec framebuffer processing, including preview snapshots.
#[cfg(test)]
use super::*;
#[cfg(test)]
pub(super) use crate::adv::higurashi::framebuffer::Packet;
pub(super) use crate::adv::higurashi::framebuffer::apply;
#[cfg(test)]
use glam::{Mat4, Vec2, Vec4};
#[cfg(test)]
use shin_core::primitives::color::FloatColor4;
#[cfg(test)]
use shin_render::{
    DrawPrimitive, LayerFragmentShader, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosTexVertex},
};
#[cfg(test)]
pub(in crate::app::higurashi::save_menu::preview) mod tests;
