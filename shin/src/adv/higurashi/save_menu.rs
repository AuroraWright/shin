//! Updated manual Save/Load menu; full save payloads stay in the application owner.
pub(crate) mod model;
pub(crate) mod resources;
pub(crate) mod view;

pub(crate) use model::{
    Action, Mode, Model, Motion, QuestionKind, Services, SlotMetadata, ViewState,
};

#[cfg(test)]
mod tests;
