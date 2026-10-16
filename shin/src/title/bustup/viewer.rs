//! Native shared-scene viewer3c8e0/3cb60/3cdf0 and return waiter40110.
use super::menu::BustupMenu;
use crate::{
    adv::input_area::{InputArea, InputAreaTarget, PointerFrame},
    app::AppAction,
    layer::{DrawableLayer, ScreenLayer},
};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::{
    time::{Easing, Ticks, Tween},
    vm::command::types::{LayerProperty, PlaneId},
};
use shin_input::ActionState;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Opening,
    InstallingInput,
    Active,
    Returning,
    Finished,
}
#[derive(Default)]
pub(crate) struct Controls {
    pub cancel: bool,
    pub confirm: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub axes: Vec2,
}
pub(crate) struct BustupViewer {
    phase: Phase,
    position: Vec2,
    velocity: Vec2,
    sounds: Vec<&'static str>,
    input_enabled: bool,
}
fn tween(screen: &mut ScreenLayer, property: LayerProperty, value: f32) {
    screen
        .properties_mut()
        .property_tweener_mut(property)
        .enqueue_now(
            value,
            Tween {
                duration: Ticks::from_f32(30.0),
                easing: Easing::SineOut,
            },
        );
}
fn camera(screen: &mut ScreenLayer, position: Vec2, animate: bool) {
    let props = screen
        .page_layer_mut()
        .get_plane_mut(PlaneId::new(0))
        .properties_mut();
    for (property, value) in [
        (LayerProperty::CameraPositionX, position.x),
        (LayerProperty::CameraPositionY, position.y),
    ] {
        let mut clock = props.property_tweener_mut(property);
        if animate {
            clock.enqueue_now(
                value,
                Tween {
                    duration: Ticks::from_f32(30.0),
                    easing: Easing::SineOut,
                },
            );
        } else {
            clock.fast_forward_to(value);
        }
    }
}
impl BustupViewer {
    pub fn new(menu: &mut BustupMenu, screen: &mut ScreenLayer) -> Self {
        //75d20(...,0) selects TranslateX, centering the retained screen. This
        //does not change depth or load another copy of the selected character.
        tween(screen, LayerProperty::TranslateX, 0.0);
        menu.begin_viewer();
        Self {
            phase: Phase::Opening,
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            sounds: Vec::new(),
            input_enabled: false,
        }
    }
    pub fn is_finished(&self) -> bool {
        self.phase == Phase::Finished
    }
    pub fn is_closing(&self) -> bool {
        matches!(self.phase, Phase::Returning | Phase::Finished)
    }
    pub fn position(&self) -> Vec2 {
        self.position
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }

    /// Poll before advancing clocks. A completing blocking waiter still consumes
    /// eligibility; newly installed input tasks enter the following snapshot.
    pub fn prepare_frame(&mut self, menu: &mut BustupMenu, screen: &ScreenLayer, eligible: bool) {
        self.input_enabled = false;
        let centering = screen
            .properties()
            .property_tweener(LayerProperty::TranslateX)
            .is_waiting();
        match self.phase {
            Phase::Opening if !centering => self.phase = Phase::InstallingInput,
            Phase::InstallingInput => {
                self.phase = Phase::Active;
                self.input_enabled = eligible;
            }
            Phase::Active => self.input_enabled = eligible,
            Phase::Returning if !centering && !menu.panel_is_waiting() => {
                menu.resume_input();
                self.phase = Phase::Finished;
            }
            _ => {}
        }
    }
    fn close(&mut self, menu: &mut BustupMenu, screen: &mut ScreenLayer, sound: &'static str) {
        if !self.input_enabled || self.is_closing() {
            return;
        }
        self.sounds.push(sound);
        self.phase = Phase::Returning;
        self.input_enabled = false;
        tween(screen, LayerProperty::TranslateX, -200.0);
        camera(screen, Vec2::ZERO, true);
        menu.return_from_viewer();
    }
    pub fn controller(
        &mut self,
        menu: &mut BustupMenu,
        screen: &mut ScreenLayer,
        eligible: &mut bool,
        pointer_consumed: bool,
        controls: Controls,
    ) {
        let enabled = *eligible && self.input_enabled;
        *eligible = false;
        if !enabled || pointer_consumed {
            return;
        }
        if controls.cancel || controls.confirm {
            self.close(menu, screen, "cancel");
            return;
        }
        // Native checks Up/Down and Left/Right separately. Digital movement
        // overrides that axis; analog samples already carry native axis signs.
        let y = if controls.up {
            -10.0
        } else if controls.down {
            10.0
        } else {
            controls.axes.y * 20.0
        };
        let x = if controls.left {
            -10.0
        } else if controls.right {
            10.0
        } else {
            controls.axes.x * 20.0
        };
        if x != 0.0 || y != 0.0 {
            self.position += Vec2::new(x, y);
            self.velocity = Vec2::ZERO;
        }
        self.clamp();
    }
    fn clamp(&mut self) {
        self.position = self
            .position
            .clamp(Vec2::new(-420.0, -130.0), Vec2::new(420.0, 130.0));
    }
    fn probe(&mut self) -> bool {
        self.velocity = Vec2::ZERO;
        true
    }
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        let delta = current - previous;
        self.position -= Vec2::new(delta.x, delta.y * 0.5625);
        self.clamp();
    }
    fn gesture(&mut self, velocity: Vec2) {
        self.velocity = Vec2::new(velocity.x, velocity.y * 0.5625);
    }
    ///3cb60 updates inertia after its child task snapshot, even with a zero or
    ///negative delta. Closing keeps local inertia but stops publishing the camera.
    pub fn advance(&mut self, delta: Ticks, screen: &mut ScreenLayer) {
        if self.velocity != Vec2::ZERO {
            self.position -= self.velocity;
            self.velocity *= 0.95f32.powf(delta.as_f32());
            self.clamp();
        }
        if !self.is_closing() {
            camera(screen, self.position, false);
        }
    }
}

struct Target<'a> {
    viewer: &'a mut BustupViewer,
    menu: &'a mut BustupMenu,
    screen: &'a mut ScreenLayer,
}
impl InputAreaTarget for Target<'_> {
    fn probe(&mut self, _: Vec2) -> bool {
        self.viewer.probe()
    }
    fn reset(&mut self) {}
    fn press(&mut self, _: Vec2) {}
    fn release(&mut self, _: Vec2) {
        self.viewer.close(self.menu, self.screen, "decide");
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, previous: Vec2, current: Vec2) {
        self.viewer.drag(previous, current);
    }
    fn gesture(&mut self, velocity: Vec2) {
        self.viewer.gesture(velocity);
    }
    fn cancel(&mut self) {}
}
pub(crate) struct BustupViewerInput {
    area: InputArea,
}
impl BustupViewerInput {
    pub fn new(extent: (u16, u16)) -> Self {
        Self {
            area: InputArea::new(extent.0, extent.1),
        }
    }
    pub fn resize(
        &mut self,
        viewer: &mut BustupViewer,
        menu: &mut BustupMenu,
        screen: &mut ScreenLayer,
        extent: (u16, u16),
    ) {
        self.area.disable(&mut Target {
            viewer,
            menu,
            screen,
        });
        self.area = InputArea::new(extent.0, extent.1);
    }
    pub fn input_frame(
        &mut self,
        viewer: &mut BustupViewer,
        menu: &mut BustupMenu,
        screen: &mut ScreenLayer,
        eligible: &mut bool,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        axes: Vec2,
    ) {
        let mut target = Target {
            viewer,
            menu,
            screen,
        };
        let consumed = if *eligible && target.viewer.input_enabled {
            self.area.update(&mut target, frame)
        } else {
            self.area.disable(&mut target);
            false
        };
        target.viewer.controller(
            target.menu,
            target.screen,
            eligible,
            consumed,
            Controls {
                cancel: state[AppAction::Cancel].is_clicked,
                confirm: state[AppAction::Enter].is_clicked,
                up: state[AppAction::Up].is_clicked_or_repeated,
                down: state[AppAction::AnyDown].is_clicked_or_repeated,
                left: state[AppAction::Left].is_clicked_or_repeated,
                right: state[AppAction::Right].is_clicked_or_repeated,
                axes,
            },
        );
    }
}

#[cfg(test)]
mod tests;
