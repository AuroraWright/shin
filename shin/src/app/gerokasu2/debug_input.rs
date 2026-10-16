//! Debug-build scripted input for headless investigation of the real window
//! loop: `SHIN_DEBUG_INPUT="120:Enter,300:Enter,400-460:Act"` holds the named
//! `AppAction` on those update frames (a range holds it on every frame).
//! Release builds and an unset variable leave input untouched.
use std::sync::OnceLock;

use enum_map::EnumMap;
use shin_input::ActionState;

use crate::app::AppAction;

fn script() -> &'static [(u64, u64, AppAction)] {
    static SCRIPT: OnceLock<Vec<(u64, u64, AppAction)>> = OnceLock::new();
    SCRIPT.get_or_init(|| {
        if !cfg!(debug_assertions) {
            return Vec::new();
        }
        let Ok(text) = std::env::var("SHIN_DEBUG_INPUT") else {
            return Vec::new();
        };
        text.split(',')
            .filter_map(|item| {
                let (frames, name) = item.trim().split_once(':')?;
                let (start, end) = match frames.split_once('-') {
                    Some((a, b)) => (a.parse().ok()?, b.parse().ok()?),
                    None => {
                        let frame = frames.parse().ok()?;
                        (frame, frame)
                    }
                };
                let action = EnumMap::<AppAction, ()>::default()
                    .into_iter()
                    .map(|(action, ())| action)
                    .find(|action| format!("{action:?}") == name)?;
                Some((start, end, action))
            })
            .collect()
    })
}

/// Apply the script to this update frame's input.
pub(super) fn apply(frame: u64, input: &mut EnumMap<AppAction, ActionState>) {
    for &(start, end, action) in script() {
        if (start..=end).contains(&frame) {
            input[action] = ActionState {
                is_held: true,
                is_clicked: frame == start,
                is_clicked_or_repeated: frame == start,
                is_clicked_or_rapid_repeated: frame == start,
            };
        }
    }
}
