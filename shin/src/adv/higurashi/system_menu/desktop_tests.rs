//! The updated native1d86c/1d870 gesture is RET, on desktop as on Switch.
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
fn higurashi_system_menu_left_swipe_does_not_close() {
    for hz in [60, 120] {
        for desktop in [true, false] {
            let mut h = Harness::new(0);
            h.drag(hz, vec2(-128., 0.), desktop, 1);
            assert_eq!(h.menu.phase(), 1, "{hz} Hz, desktop {desktop}");
            assert!(h.events.cues.is_empty());
        }
    }
}
