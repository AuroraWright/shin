//! MSGFACE's retained selection, bustup request and asynchronous publication.
//! Native factory `eb4`, poll `566c`, selector `146e0`, worker `8f990`.

use std::sync::{
    Arc, Weak,
    atomic::{AtomicU32, Ordering},
};

use parking_lot::Mutex;

use crate::{format::scenario::gerokasu2::BustupInfoItem, vm::command::types::LayerProperty};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortraitRequest {
    pub bustup_index: i32,
    /// Native path spelling. Archive lookup applies its own ASCII case fold.
    pub path: String,
    pub expression: String,
    pub lipsync_character_id: i32,
    pub negated_translation_x: i32,
    pub negated_translation_y: i32,
    pub scale: i32,
    /// Raw TranslateX/TranslateY. Gerokasu2's worker installs (-960, 540);
    /// Gerokasu's face worker (native 93290) leaves them neutral.
    pub base_translation: [i32; 2],
    /// Gerokasu's per-entry bustup placement (scale %, x, y), drawn by the
    /// bustup layer itself (native 351c0).
    pub placement: Option<[i16; 3]>,
}

impl PortraitRequest {
    /// Raw layer-property units, not normalized scale values. The shared layer
    /// transform divides the scale properties by 1000 when drawing.
    pub fn properties(&self) -> [(LayerProperty, i32); 6] {
        use LayerProperty::*;
        [
            (TranslateX, self.base_translation[0]),
            (TranslateY, self.base_translation[1]),
            (NegatedTranslationX, self.negated_translation_x),
            (NegatedTranslationY, self.negated_translation_y),
            (ScaleX, self.scale),
            (ScaleY, self.scale),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortraitChange {
    Unchanged,
    Clear,
    Load(PortraitRequest),
}

pub struct MessageFaceState {
    retained_index: i32,
    /// Gerokasu's five-operand MSGFACE publishes immediately and MSGSET does
    /// not re-request the face.
    gerokasu: bool,
}

impl MessageFaceState {
    /// The caller supplies the VM's initialized/restored selection. This type
    /// does not invent an unverified VM-constructor default.
    pub fn new(retained_index: i32) -> Self {
        Self {
            retained_index,
            gerokasu: false,
        }
    }

    /// Gerokasu: MSGSET never requests a face; only MSGFACE does.
    pub fn set_gerokasu(&mut self) {
        self.gerokasu = true;
    }
    pub fn is_gerokasu(&self) -> bool {
        self.gerokasu
    }

    pub fn retained_index(&self) -> i32 {
        self.retained_index
    }

    pub fn on_msgface(
        &mut self,
        index: i32,
        async_message_active: bool,
        table: &[BustupInfoItem],
    ) -> PortraitChange {
        // Factory eb4 retains the selection even when poll566c is inactive.
        // Its guard is VM90f0: MSGSET constructor4ed8 clears it for auto-wait,
        // while non-waiting text retains it until a negative MSGWAIT.
        self.retained_index = index;
        if async_message_active {
            self.on_message_installed(table)
        } else {
            PortraitChange::Unchanged
        }
    }

    /// Gerokasu MSGFACE `(slot, entry, dress, face, variant)` (handler c0dd0,
    /// worker 93290): a valid entry loads `%DRESS%`/`%03d` bustup paths with
    /// the entry placement and negated translation (760, -550); a negative or
    /// unknown entry clears the face.
    pub fn on_ger_msgface(&mut self, args: [i32; 5], table: &[BustupInfoItem]) -> PortraitChange {
        use crate::format::scenario::gerokasu2::Gerokasu2Scenario;
        self.gerokasu = true;
        let [_slot, entry, dress, face, variant] = args;
        self.retained_index = entry;
        let Some(item) = usize::try_from(entry).ok().and_then(|index| table.get(index)) else {
            return PortraitChange::Clear;
        };
        PortraitChange::Load(PortraitRequest {
            bustup_index: entry,
            path: Gerokasu2Scenario::gerokasu_bustup_path(item.name.as_str(), dress),
            expression: format!("{face:03}{}", if variant == 1 { "a" } else { "" }),
            lipsync_character_id: i32::from(item.lipsync_character_id as i16),
            negated_translation_x: 760,
            negated_translation_y: -550,
            scale: 1000,
            base_translation: [0, 0],
            placement: Some(item.raw_placement),
        })
    }

    /// MSGSET's first poll5130 uses VM9100 after installing text, independently
    /// of MSGSET's extra numeric argument and its asynchronous-message flag.
    pub fn on_message_installed(&self, table: &[BustupInfoItem]) -> PortraitChange {
        if self.gerokasu {
            return PortraitChange::Unchanged;
        }
        let Some(item) = usize::try_from(self.retained_index)
            .ok()
            .and_then(|index| table.get(index))
        else {
            return PortraitChange::Clear;
        };
        let [x, y, scale] = item.raw_placement;
        PortraitChange::Load(PortraitRequest {
            bustup_index: self.retained_index,
            path: format!("/bustup/M/{}.bup", item.name.as_str()),
            expression: item.emotion.as_str().to_owned(),
            lipsync_character_id: i32::from(item.lipsync_character_id as i16),
            negated_translation_x: i32::from(x),
            negated_translation_y: i32::from(y),
            scale: i32::from(scale),
            base_translation: [-960, 540],
            placement: None,
        })
    }
}

struct PortraitShared<T> {
    generation: AtomicU32,
    visible: Mutex<Option<T>>,
}

/// A new selection retains the visible portrait until its asset is ready.
/// Clearing invalidates pending work and removes the visible resource.
pub struct MessagePortrait<T> {
    shared: Arc<PortraitShared<T>>,
}

pub struct PortraitJob<T> {
    shared: Weak<PortraitShared<T>>,
    generation: u32,
    request: PortraitRequest,
}

impl<T> Default for MessagePortrait<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> MessagePortrait<T> {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(PortraitShared {
                generation: AtomicU32::new(0),
                visible: Mutex::new(None),
            }),
        }
    }

    pub fn generation(&self) -> u32 {
        self.shared.generation.load(Ordering::Acquire)
    }

    pub fn apply(&self, change: PortraitChange) -> Option<PortraitJob<T>> {
        match change {
            PortraitChange::Unchanged => None,
            PortraitChange::Clear => {
                self.clear();
                None
            }
            PortraitChange::Load(request) => {
                // Native8d8b0: an acquire/release 32-bit increment, outside
                // the publication lock, without discarding the old portrait.
                let generation = self
                    .shared
                    .generation
                    .fetch_add(1, Ordering::AcqRel)
                    .wrapping_add(1);
                Some(PortraitJob {
                    shared: Arc::downgrade(&self.shared),
                    generation,
                    request,
                })
            }
        }
    }

    pub fn clear(&self) {
        let mut visible = self.shared.visible.lock();
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        *visible = None;
    }

    pub fn visible(&self) -> Option<T>
    where
        T: Clone,
    {
        self.shared.visible.lock().clone()
    }
}

impl<T> PortraitJob<T> {
    pub fn request(&self) -> &PortraitRequest {
        &self.request
    }
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Check between asset acquisition stages, like native8f9b4/8fa1c. Final
    /// publication always rechecks under the same lock used by clear().
    pub fn is_current(&self) -> bool {
        self.shared
            .upgrade()
            .is_some_and(|shared| shared.generation.load(Ordering::Acquire) == self.generation)
    }

    pub fn publish(self, value: T) -> bool {
        let Some(shared) = self.shared.upgrade() else {
            return false;
        };
        let mut visible = shared.visible.lock();
        if shared.generation.load(Ordering::Acquire) != self.generation {
            return false;
        }
        *visible = Some(value);
        true
    }
}

#[cfg(test)]
mod tests;
