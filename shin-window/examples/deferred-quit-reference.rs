//! Focused AppKit Quit bridge check; hidden window, no game saves/audio.
#[cfg(target_os = "macos")]
#[path = "../src/macos_quit.rs"]
mod macos_quit;

#[cfg(target_os = "macos")]
fn main() {
    use objc2::{class, msg_send, msg_send_id, rc::Retained, runtime::AnyObject, sel};
    use winit::{
        application::ApplicationHandler,
        event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
        window::Window,
    };
    struct Reference {
        proxy: EventLoopProxy<()>,
        window: Option<Window>,
        guard: Option<macos_quit::QuitHandler>,
        requests: usize,
    }
    impl ApplicationHandler<()> for Reference {
        fn resumed(&mut self, events: &ActiveEventLoop) {
            self.window = Some(
                events
                    .create_window(Window::default_attributes().with_visible(false))
                    .unwrap(),
            );
            let proxy = self.proxy.clone();
            self.guard = Some(macos_quit::install(move || proxy.send_event(()).is_ok()).unwrap());
            // Same NSApplication entry points used by menu/Cmd-Q and Dock Quit.
            // Native termination must return without ending the live event loop.
            unsafe {
                let app: Retained<AnyObject> =
                    msg_send_id![class!(NSApplication), sharedApplication];
                let delegate: *mut AnyObject = msg_send![&*app, delegate];
                let menu: *mut AnyObject = msg_send![&*app, mainMenu];
                let top: *mut AnyObject = msg_send![menu, itemAtIndex:0isize];
                let submenu: *mut AnyObject = msg_send![top, submenu];
                let count: isize = msg_send![submenu, numberOfItems];
                let mut found = false;
                for index in 0..count {
                    let item: *mut AnyObject = msg_send![submenu, itemAtIndex:index];
                    let action: Option<objc2::runtime::Sel> = msg_send![item, action];
                    if action == Some(sel!(terminate:)) {
                        let key: *mut AnyObject = msg_send![item, keyEquivalent];
                        let key_length: usize = msg_send![key, length];
                        assert_eq!(key_length, 1);
                        let character: u16 = msg_send![key, characterAtIndex:0usize];
                        assert_eq!(character, b'q' as u16);
                        let _: () = msg_send![submenu, performActionForItemAtIndex:index];
                        found = true;
                        break;
                    }
                }
                assert!(found, "Native Quit menu missing");
                let _: () = msg_send![&*app, terminate:std::ptr::null::<AnyObject>()];
                let _: () = msg_send![&*app, terminate:std::ptr::null::<AnyObject>()];
                let after: *mut AnyObject = msg_send![&*app, delegate];
                assert_eq!(delegate, after, "Winit delegate must remain intact");
                println!(
                    "Menu/Cmd-Q item and two NSApplication Quit requests cancelled native termination; delegate identity preserved."
                );
            }
        }
        fn user_event(&mut self, events: &ActiveEventLoop, _: ()) {
            self.requests += 1;
            if self.requests == 3 {
                println!(
                    "All three Quit requests delivered to the live event loop; owner may now exit."
                );
                events.exit();
            }
        }
        fn window_event(
            &mut self,
            _: &ActiveEventLoop,
            _: winit::window::WindowId,
            _: winit::event::WindowEvent,
        ) {
        }
    }
    let event_loop = EventLoop::<()>::with_user_event().build().unwrap();
    let mut reference = Reference {
        proxy: event_loop.create_proxy(),
        window: None,
        guard: None,
        requests: 0,
    };
    event_loop.run_app(&mut reference).unwrap();
    assert_eq!(reference.requests, 3);
    println!("Deferred Quit bridge passed.");
}
#[cfg(not(target_os = "macos"))]
fn main() {}
