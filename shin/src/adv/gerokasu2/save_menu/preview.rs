//! Native SaveMenu B3270's two serial preview channels.
//!
//! Payloads and GPU work belong to the host. A replay snapshot is retained for
//! the menu lifetime; its thumbnail and labels are discarded when offscreen.
//! Tickets are a desktop transport guard, not an additional native cache key.

pub const SLOT_COUNT: usize = 100;
pub const THUMBNAIL_SIZE: [u32; 2] = [336, 189];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub ticket: u64,
    pub slot: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Labels {
    Empty,
    Loading,
    /// An empty first captured SAVEINFO label promotes the second label into
    /// the first font/position and leaves the second text owner empty.
    Saved {
        first_is_empty: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    /// GOT1588B8 resolves to1B8EF8, the same /system.fnt owner as Secondary.
    Primary,
    Secondary,
}

impl Labels {
    /// B35A8..B3990 uses44px GenericText, width1252 and this row-local
    /// baseline. The second captured label uses the secondary font at564,79.
    pub fn first_line(self) -> (Font, [i32; 2]) {
        match self {
            Self::Empty => (Font::Secondary, [564, 61]),
            Self::Loading
            | Self::Saved {
                first_is_empty: true,
            } => (Font::Primary, [564, 61]),
            Self::Saved {
                first_is_empty: false,
            } => (Font::Primary, [564, 31]),
        }
    }
    pub fn second_line(self) -> Option<(Font, [i32; 2])> {
        matches!(
            self,
            Self::Saved {
                first_is_empty: false
            }
        )
        .then_some((Font::Secondary, [564, 79]))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    StartReplay(Request),
    /// Replace the retained texture service job, canceling/joining its previous
    /// worker before it can mutate the reused private scene (D68B0).
    StartTexture(Request),
    DropTexture {
        slot: usize,
    },
    ClearLabels {
        slot: usize,
    },
    SetLabels {
        slot: usize,
        labels: Labels,
    },
    ReplayPublished(Request),
    ReplayDiscarded(Request),
    TexturePublished(Request),
    /// Stop polling this target. Native leaves the service's worker retained
    /// until StartTexture replaces it or the entire menu is destroyed.
    AbandonTexture(Request),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Completions {
    /// Deliver only a non-null completed replay snapshot. Completion is
    /// published after all rows have been visited, even if its row is hidden.
    pub replay: Option<(Request, bool)>,
    /// Deliver only a non-null texture returned by the ready texture service.
    pub texture: Option<Request>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Row {
    snapshot: Option<bool>,
    texture: bool,
    dirty: bool,
    replay_failed: bool,
}

pub struct Controller {
    rows: [Row; SLOT_COUNT],
    replay: Option<Request>,
    discard_replay: bool,
    texture: Option<Request>,
    serial: u64,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            rows: [Row {
                dirty: true,
                ..Row::default()
            }; SLOT_COUNT],
            replay: None,
            discard_replay: false,
            texture: None,
            serial: 0,
        }
    }
}

impl Controller {
    fn request(&mut self, slot: usize) -> Request {
        self.serial = self
            .serial
            .checked_add(1)
            .expect("preview ticket exhausted");
        Request {
            ticket: self.serial,
            slot,
        }
    }

    pub fn replay_request(&self) -> Option<Request> {
        self.replay
    }
    pub fn texture_request(&self) -> Option<Request> {
        self.texture
    }
    pub fn has_snapshot(&self, slot: usize) -> bool {
        self.rows[slot].snapshot.is_some()
    }
    pub fn has_texture(&self, slot: usize) -> bool {
        self.rows[slot].texture
    }

    /// Desktop recovery for a failed replay transport. Keep this row's pending
    /// fallback and let other rows progress; a changed/rolled-back slot permits
    /// another attempt. This is not a native retry or a fabricated snapshot.
    pub fn replay_failed(&mut self, request: Request) {
        if self.replay == Some(request) {
            self.rows[request.slot].replay_failed = !self.discard_replay;
            self.replay = None;
            self.discard_replay = false;
        }
    }

    /// Native Save/Delete and their rollback clear this one row's caches. An
    /// already-running metadata replay is allowed to finish, then discarded.
    /// The host must drop its corresponding payloads before the next frame.
    pub fn invalidate(&mut self, slot: usize) -> Vec<Event> {
        let row = &mut self.rows[slot];
        let mut events = Vec::new();
        if row.texture {
            events.push(Event::DropTexture { slot });
        }
        *row = Row {
            dirty: true,
            ..Row::default()
        };
        if self.replay.is_some_and(|request| request.slot == slot) {
            self.discard_replay = true;
        }
        if self.texture.is_some_and(|request| request.slot == slot) {
            events.push(Event::AbandonTexture(self.texture.take().unwrap()));
        }
        events
    }

    /// Visit ascending row IDs, then publish completed replay, then texture.
    /// New work cannot consume a replay snapshot published later in this frame.
    /// `scroll` is the native scroll tween value after its literal-one update.
    pub fn frame(
        &mut self,
        scroll: f32,
        occupied: &[bool; SLOT_COUNT],
        completions: Completions,
    ) -> Vec<Event> {
        assert!(scroll.is_finite());
        let mut events = Vec::new();
        for (slot, &occupied) in occupied.iter().enumerate() {
            if row_visible(slot, scroll) {
                if self.replay.is_none()
                    && occupied
                    && self.rows[slot].snapshot.is_none()
                    && !self.rows[slot].replay_failed
                {
                    let request = self.request(slot);
                    self.replay = Some(request);
                    self.discard_replay = false;
                    events.push(Event::StartReplay(request));
                }
                if self.texture.is_none()
                    && !self.rows[slot].texture
                    && self.rows[slot].snapshot.is_some()
                {
                    let request = self.request(slot);
                    self.texture = Some(request);
                    events.push(Event::StartTexture(request));
                }
                let row = &mut self.rows[slot];
                if row.dirty {
                    events.push(Event::SetLabels {
                        slot,
                        labels: if !occupied {
                            Labels::Empty
                        } else if let Some(first_is_empty) = row.snapshot {
                            Labels::Saved { first_is_empty }
                        } else {
                            Labels::Loading
                        },
                    });
                    row.dirty = false;
                }
            } else {
                let row = &mut self.rows[slot];
                if std::mem::take(&mut row.texture) {
                    events.push(Event::DropTexture { slot });
                }
                row.dirty = true;
                events.push(Event::ClearLabels { slot });
                if self.texture.is_some_and(|request| request.slot == slot) {
                    events.push(Event::AbandonTexture(self.texture.take().unwrap()));
                }
            }
        }
        if let Some((request, first_is_empty)) = completions.replay
            && self.replay == Some(request)
        {
            if self.discard_replay {
                events.push(Event::ReplayDiscarded(request));
            } else {
                self.rows[request.slot].snapshot = Some(first_is_empty);
                self.rows[request.slot].dirty = true;
                events.push(Event::ReplayPublished(request));
            }
            self.replay = None;
        }
        if let Some(request) = completions.texture
            && self.texture == Some(request)
        {
            self.rows[request.slot].texture = true;
            self.texture = None;
            events.push(Event::TexturePublished(request));
        }
        events
    }
}

/// B3500..B352C uses the full1080 canvas, including exact top-edge contact.
/// This differs from the list's800px clipping rectangle used at render time.
pub fn row_visible(slot: usize, scroll: f32) -> bool {
    let top = (slot as i32 * 201) as f32 - (scroll + -176.0);
    top < 1080.0 && top + 197.0 >= 0.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Descriptor {
    pub kind: i32,
    pub parameters: [i32; 8],
}

/// D6FCC..D7060: a movie uses its signed linked-picture ID, with argument1
/// cleared. A missing/negative/out-of-range movie lookup becomes kind0 while
/// preserving all eight original arguments. Other kinds pass through.
/// The lookup closure must implement the native signed bounds check (BCFC8).
pub fn scene_descriptor(
    mut descriptor: Descriptor,
    movie_picture: impl FnOnce(i32) -> Option<i16>,
) -> Descriptor {
    if descriptor.kind == 6 {
        if let Some(picture) = movie_picture(descriptor.parameters[0]) {
            descriptor.kind = 2;
            descriptor.parameters[0] = i32::from(picture);
            descriptor.parameters[1] = 0;
        } else {
            descriptor.kind = 0;
        }
    }
    descriptor
}

/// D61E0 builds a root render target at the first halved size. It retains
/// intermediate color targets until reaching the requested thumbnail extent;
/// D7870 appends that final target when rendering the first completed scene.
pub fn capture_extents(target: [u32; 2]) -> Vec<[u32; 2]> {
    assert!(target.into_iter().all(|value| value > 0));
    let mut current = [1920u32, 1080u32];
    let mut extents = Vec::new();
    loop {
        current = std::array::from_fn(|axis| (current[axis] >> 1).max(target[axis]));
        extents.push(current);
        if current == target {
            return extents;
        }
    }
}

#[cfg(test)]
mod native_reference;
#[cfg(test)]
mod tests;
