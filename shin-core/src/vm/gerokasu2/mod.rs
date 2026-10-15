//! Later-engine execution contracts, isolated from the Umineko VM.
//!
//! Bytecode execution reuses the engine arithmetic; ADV integration is separate.
pub mod audio;
mod executor;
pub mod message_face;
pub use executor::{GerReplayState, Gerokasu2RuntimeCommand, Gerokasu2Scripter};
