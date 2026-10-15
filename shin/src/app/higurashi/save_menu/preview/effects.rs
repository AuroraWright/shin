//! Saved factories use fresh, unadvanced older effect owners.
pub(super) use crate::layer::user::higurashi::effect::{Prepared, View};
#[cfg(test)]
use crate::layer::{
    higurashi_properties::HigurashiProperties,
    user::higurashi::effect::{Packet, ResourcePlan},
};
#[cfg(test)]
use {glam::Vec4, shin_render::PassKind};

#[cfg(test)]
pub(in crate::app::higurashi::save_menu::preview) mod tests;
