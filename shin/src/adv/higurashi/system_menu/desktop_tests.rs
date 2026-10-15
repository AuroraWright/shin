//! Desktop policy only: the updated native1d86c/1d870 gesture is still RET.
use super::*;
use crate::adv::input_area::PointerSampler;
use glam::vec2;

#[derive(Default)]
struct Events {
    entered: usize,
    exited: usize,
    cues: Vec<Cue>,
    reloads: usize,
    confirmations: usize,
    items: Vec<u32>,
}
impl MenuServices for Events {
    fn enter_modal(&mut self) {
        self.entered += 1;
    }
    fn exit_modal(&mut self) {
        self.exited += 1;
    }
    fn cue(&mut self, cue: Cue) {
        self.cues.push(cue);
    }
    fn push_confirmation(&mut self, _: Confirmation) {
        self.confirmations += 1;
    }
    fn push_layer_reloader(&mut self) {
        self.reloads += 1;
    }
    fn dispatch_item(&mut self, item: u32) {
        self.items.push(item);
    }
}
struct Harness {
    menu: SystemMenu,
    events: Events,
    pointer: PointerSampler,
    setting: u8,
    origin: Vec2,
}
impl Harness {
    fn new(scenario: i32) -> Self {
        let mut events = Events::default();
        let menu = SystemMenu::new(scenario, 0, &mut events);
        let mut h = Self {
            menu,
            events,
            pointer: PointerSampler::default(),
            setting: 0,
            origin: vec2(700., 300.),
        };
        for _ in 0..40 {
            h.sample(h.origin, false, 1. / 60., 1, true);
        }
        assert!(h.menu.input_ready());
        h
    }
    fn sample(&mut self, p: Vec2, held: bool, dt: f64, permission: u32, desktop: bool) {
        self.pointer.sample(p, held, dt);
        assert_eq!(
            self.menu.poll_with_mouse_release(
                Ticks::from_f32((dt * 60.) as f32),
                permission,
                0,
                0,
                &self.pointer.frame(),
                self.setting,
                0.,
                &mut self.events,
                if desktop {
                    self.pointer.mouse_release()
                } else {
                    None
                },
            ),
            0
        );
    }
    fn drag(&mut self, hz: usize, displacement: Vec2, desktop: bool, permission: u32) {
        let dt = 1. / hz as f64;
        self.sample(self.origin, true, dt, permission, desktop);
        for _ in 0..hz * 3 / 10 {
            self.sample(self.origin, true, dt, permission, desktop);
        }
        for i in 1..=hz / 5 {
            self.sample(
                self.origin + displacement * (i as f32 / (hz / 5) as f32),
                true,
                dt,
                permission,
                desktop,
            );
        }
        for _ in 0..(hz as f64 * 0.04).ceil() as usize {
            self.sample(self.origin + displacement, true, dt, permission, desktop);
        }
        self.sample(self.origin + displacement, false, dt, permission, desktop);
    }
}

#[test]
fn higurashi_system_menu_desktop_slow_left_closes_at_60_and_120_hz() {
    for hz in [60, 120] {
        let mut h = Harness::new(0);
        h.drag(hz, vec2(-128., 0.), true, 1);
        assert_eq!(h.menu.phase(), 2);
        assert_eq!(h.events.cues, [Cue::Cancel]);
        assert_eq!(h.events.entered, 1);
        assert_eq!(
            h.events.exited, 0,
            "modal retirement waits for closing clocks"
        );
        for _ in 0..80 {
            h.sample(h.origin, false, 1. / hz as f64, 1, true);
        }
        assert_eq!(h.menu.phase(), -1);
        assert_eq!(h.events.exited, 1);
        assert_eq!(h.events.confirmations, 0);
        assert!(h.events.items.is_empty());
        let mut native = Harness::new(0);
        native.drag(hz, vec2(-128., 0.), false, 1);
        assert_eq!(
            native.menu.phase(),
            1,
            "native/None gesture remains a no-op"
        );
        assert!(native.events.cues.is_empty());
    }
}

#[test]
fn higurashi_system_menu_desktop_direction_permission_cancel_and_busy_gates() {
    for d in [
        vec2(128., 0.),
        vec2(0., -128.),
        vec2(-128., 100.),
        vec2(-12., 0.),
    ] {
        let mut h = Harness::new(0);
        h.drag(60, d, true, 1);
        assert_eq!(h.menu.phase(), 1, "{d:?}");
        assert!(h.events.cues.is_empty());
    }
    let mut denied = Harness::new(0);
    denied.drag(60, vec2(-128., 0.), true, 0);
    assert_eq!(denied.menu.phase(), 1);
    assert!(denied.events.cues.is_empty());
    let mut cancelled = Harness::new(0);
    cancelled.sample(cancelled.origin, true, 0.3, 1, true);
    cancelled.sample(cancelled.origin + vec2(-128., 0.), true, 0.2, 1, true);
    cancelled.menu.cancel_input(&mut cancelled.events);
    cancelled.pointer.cancel_contact(true);
    cancelled.sample(cancelled.origin + vec2(-128., 0.), true, 0.04, 1, true);
    cancelled.sample(cancelled.origin + vec2(-128., 0.), false, 0.04, 1, true);
    assert_eq!(cancelled.menu.phase(), 1);
    assert!(cancelled.events.cues.is_empty());
    let mut busy = Harness::new(0);
    busy.sample(busy.origin, true, 0.3, 1, true);
    busy.sample(busy.origin + vec2(-128., 0.), true, 0.2, 1, true);
    busy.menu.child_started();
    busy.sample(busy.origin + vec2(-128., 0.), false, 0.04, 1, true);
    assert_eq!(busy.menu.phase(), 1);
    assert!(busy.events.cues.is_empty());
}

#[test]
fn higurashi_system_menu_desktop_close_preserves_changed_setting_reload_owner() {
    let mut h = Harness::new(10008);
    h.setting = 1;
    h.drag(60, vec2(-128., 0.), true, 1);
    assert_eq!(h.menu.phase(), 2);
    assert_eq!(h.events.cues, [Cue::Cancel]);
    assert_eq!(h.events.reloads, 1);
    for _ in 0..80 {
        h.sample(h.origin, false, 1. / 60., 1, true);
    }
    assert_eq!(
        h.menu.phase(),
        2,
        "reload retains the modal owner after the close animation"
    );
    assert_eq!(h.events.exited, 0);
    h.menu.layer_reload_finished();
    h.sample(h.origin, false, 0., 1, true);
    assert_eq!(h.menu.phase(), -1);
    assert_eq!(h.events.exited, 1);
}
