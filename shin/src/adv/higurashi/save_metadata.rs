//! Updated SAVEINFO's two live pointers and AUTOSAVE's two retained copies.
//! This is memory metadata, independent of save-file encoding/platform storage.
use anyhow::{Result, ensure};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct SaveMetadata {
    current: [Arc<str>; 2],
    saved: [Arc<str>; 2],
}
impl SaveMetadata {
    pub fn new() -> Self {
        // Native10984..10998 initializes all four pointers to one empty string.
        let empty: Arc<str> = Arc::from("");
        Self {
            current: [empty.clone(), empty.clone()],
            saved: [empty.clone(), empty],
        }
    }
    pub fn current(&self) -> &[Arc<str>; 2] {
        &self.current
    }
    pub fn saved(&self) -> &[Arc<str>; 2] {
        &self.saved
    }
    pub fn set(&mut self, level: i32, text: &str) -> Result<()> {
        // Updated authored SAVEINFO uses only levels0/1. The original indexes
        // unchecked and corrupts adjacent VM fields for other values.
        ensure!(
            (0..2).contains(&level),
            "Higurashi SAVEINFO level {level} exceeds the two metadata entries"
        );
        self.current[level as usize] = Arc::from(text);
        Ok(())
    }
    pub(super) fn capture_chapter(&mut self) {
        self.saved.clone_from(&self.current);
    }
}
