//! Native BustupMode menu: controller39e80 and clamped selector3a310.
//! The resource owner must suppress input while its blocking load tasks run.
use super::bustup_catalog::{BustupCatalog, BustupSelection};
use shin_core::{
    format::scenario::info::{BustupId, PictureId},
    time::{Easing, Ticks, Tween, Tweener},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Row {
    Character,
    Outfit,
    Pose,
    Expression,
    Background,
}
impl Row {
    const ALL: [Self; 5] = [
        Self::Character,
        Self::Outfit,
        Self::Pose,
        Self::Expression,
        Self::Background,
    ];
}
#[derive(Default)]
pub(super) struct Controls {
    pub cancel: bool,
    pub confirm: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub page_up: bool,
    pub page_down: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Request {
    LoadBustup(BustupId),
    LoadBackground {
        primary: PictureId,
        secondary: Option<PictureId>,
    },
    OpenViewer,
    ReturnTitle {
        argument: u8,
        music_stop_ticks: u32,
    },
}
#[derive(Clone, Copy)]
enum Transition {
    Close,
    Viewer,
}

pub(super) struct BustupMenu {
    selection: BustupSelection,
    focus: usize,
    arrows: [Tweener; 10],
    close: Tweener,
    confirm: Tweener,
    panel_offset: Tweener,
    controls_visibility: Tweener,
    retired: bool,
    transition: Option<Transition>,
    requests: Vec<Request>,
    sounds: Vec<&'static str>,
    pointer_target: i32,
    pointer_selector: i32,
}
fn flash(clock: &mut Tweener) {
    // bb420 evaluates the current value and clears the previous queue.
    let duration = (0.7 - clock.value()).abs() / 0.2;
    clock.enqueue_now(0.7, Tween::linear(Ticks::from_f32(duration)));
    clock.enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
}
impl BustupMenu {
    pub fn new(catalog: &BustupCatalog) -> Self {
        Self {
            selection: BustupSelection::new(catalog),
            focus: 0,
            arrows: std::array::from_fn(|_| Tweener::new(0.0)),
            close: Tweener::new(0.0),
            confirm: Tweener::new(0.0),
            panel_offset: Tweener::new(0.0),
            controls_visibility: Tweener::new(1.0),
            retired: false,
            transition: None,
            requests: Vec::new(),
            sounds: Vec::new(),
            pointer_target: -1,
            pointer_selector: -1,
        }
    }
    pub fn row(&self) -> Row {
        Row::ALL[self.focus]
    }
    pub fn selection(&self) -> &BustupSelection {
        &self.selection
    }
    pub fn arrow_values(&self) -> [f32; 10] {
        std::array::from_fn(|i| self.arrows[i].value())
    }
    pub fn panel_offset(&self) -> f32 {
        self.panel_offset.value()
    }
    pub fn controls_visibility(&self) -> f32 {
        self.controls_visibility.value()
    }
    pub(super) fn begin_viewer(&mut self) {
        self.controls_visibility
            .enqueue_now(0.0, Tween::linear(Ticks::from_f32(10.0)));
    }
    pub(super) fn return_from_viewer(&mut self) {
        self.controls_visibility
            .enqueue_now(1.0, Tween::linear(Ticks::from_f32(30.0)));
        self.panel_offset.enqueue_now(
            0.0,
            Tween {
                duration: Ticks::from_f32(30.0),
                easing: Easing::SineOut,
            },
        );
    }
    pub(super) fn panel_is_waiting(&self) -> bool {
        self.panel_offset.is_waiting()
    }
    pub(super) fn resume_input(&mut self) {
        self.retired = false;
        self.pointer_target = -1;
        self.pointer_selector = -1;
    }
    pub fn has_blocking_transition(&self) -> bool {
        self.transition.is_some()
    }
    pub fn input_alive(&self) -> bool {
        !self.retired
    }
    pub fn button_values(&self) -> [f32; 2] {
        [self.close.value(), self.confirm.value()]
    }
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }

    /// Consumes eligibility even when disabled or a pointer contact won.
    /// Returns the menu input task's liveness (scene+D8), not scene liveness.
    pub fn controller(
        &mut self,
        catalog: &BustupCatalog,
        eligible: &mut bool,
        pointer_consumed: bool,
        controls: Controls,
    ) -> bool {
        let enabled = *eligible;
        *eligible = false;
        if !enabled || pointer_consumed || self.retired {
            return !self.retired;
        }
        if controls.cancel {
            self.begin_transition(Transition::Close, "cancel");
        } else if controls.confirm {
            self.begin_transition(Transition::Viewer, "decide");
        } else if controls.up {
            if self.focus > 0 {
                self.focus -= 1;
                self.sounds.push("move");
            }
        } else if controls.down {
            if self.focus < 4 {
                self.focus += 1;
                self.sounds.push("move");
            }
        } else {
            let delta = if controls.left {
                -1
            } else if controls.right {
                1
            } else if controls.page_up {
                -10
            } else if controls.page_down {
                10
            } else {
                0
            };
            if self.change(catalog, delta) {
                flash(&mut self.arrows[self.focus * 2 + usize::from(delta > 0)]);
            }
        }
        !self.retired
    }

    fn begin_transition(&mut self, transition: Transition, sound: &'static str) {
        self.sounds.push(sound);
        match transition {
            Transition::Close => flash(&mut self.close),
            Transition::Viewer => {
                flash(&mut self.confirm);
                self.panel_offset.enqueue(
                    500.0,
                    Tween {
                        duration: Ticks::from_f32(30.0),
                        easing: Easing::SineOut,
                    },
                );
            }
        }
        self.transition = Some(transition);
        self.retired = true;
    }

    /// Preserves remembered child indices; compares indices even for duplicate IDs.
    fn change(&mut self, catalog: &BustupCatalog, delta: i32) -> bool {
        let (c, o, p, e) = self.selection.indices();
        let (current, count) = match self.row() {
            Row::Character => (c, catalog.characters.len()),
            Row::Outfit => (o, catalog.characters[c].outfits.len()),
            Row::Pose => (p, catalog.characters[c].outfits[o].poses.len()),
            Row::Expression => (
                e,
                catalog.characters[c].outfits[o].poses[p].expressions.len(),
            ),
            Row::Background => (self.selection.background_index(), catalog.backgrounds.len()),
        };
        let next = (current as i64 + i64::from(delta)).clamp(0, count as i64 - 1) as usize;
        if next == current {
            return false;
        }
        match self.row() {
            Row::Character => {
                self.selection.select_character(next);
            }
            Row::Outfit => {
                self.selection.select_outfit(next);
            }
            Row::Pose => {
                self.selection.select_pose(next);
            }
            Row::Expression => {
                self.selection.select_expression(catalog, next);
            }
            Row::Background => {
                self.selection.select_background(catalog, next);
            }
        }
        self.sounds.push("move");
        self.requests.push(if self.row() == Row::Background {
            let bg = self.selection.background(catalog);
            Request::LoadBackground {
                primary: bg.primary,
                secondary: bg.secondary,
            }
        } else {
            Request::LoadBustup(self.selection.bustup(catalog))
        });
        true
    }

    /// Native deferred functors3dd00/3dea0 poll before the scene advances clocks.
    /// The owner must snapshot blocking status before this completion poll.
    pub fn prepare_frame(&mut self) {
        let ready = match self.transition {
            Some(Transition::Close) => !self.close.is_waiting(),
            Some(Transition::Viewer) => !self.confirm.is_waiting(),
            None => false,
        };
        if ready {
            self.requests.push(match self.transition.take().unwrap() {
                Transition::Close => Request::ReturnTitle {
                    argument: 10,
                    music_stop_ticks: 30,
                },
                Transition::Viewer => Request::OpenViewer,
            });
        }
    }
    pub fn update(&mut self, delta: Ticks) {
        self.close.update(delta);
        self.confirm.update(delta);
        self.panel_offset.update(delta);
        self.controls_visibility.update(delta);
        for arrow in &mut self.arrows {
            arrow.update(delta);
        }
    }
}
mod pointer;
#[cfg(test)]
mod tests;
