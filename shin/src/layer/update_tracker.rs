use shin_core::primitives::update::FrameId;

/// Native layer objects dispatch through 6aa40. Constructors, including the
/// layer copy constructors, call 6a9e0 and initialize their own marker to zero.
/// Property objects separately copy their marker through 6aa00.
/// Retained clones (Murderer9b040) preserve the existing object's marker.
#[derive(Debug)]
pub(crate) enum LayerUpdateTracker {
    Unique(FrameId),
    Retained(std::sync::Arc<parking_lot::Mutex<FrameId>>),
}

impl Default for LayerUpdateTracker {
    fn default() -> Self {
        Self::Unique(FrameId::default())
    }
}

impl Clone for LayerUpdateTracker {
    fn clone(&self) -> Self {
        match self {
            Self::Unique(_) => Self::default(),
            Self::Retained(marker) => Self::Retained(marker.clone()),
        }
    }
}

impl LayerUpdateTracker {
    pub fn retained() -> Self {
        Self::Retained(std::sync::Arc::new(parking_lot::Mutex::new(
            FrameId::default(),
        )))
    }
    pub fn update(&mut self, frame: FrameId) -> bool {
        let mut retained;
        let marker = match self {
            Self::Unique(marker) => marker,
            Self::Retained(marker) => {
                retained = marker.lock();
                &mut retained
            }
        };
        let changed = *marker != frame;
        *marker = frame;
        changed
    }
}
