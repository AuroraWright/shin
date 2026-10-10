//! Deliver AppKit Quit through the same deferred close path as the window.
//! Winit's existing delegate and all activation/window methods remain intact.
use objc2::{
    class, ffi, msg_send, msg_send_id,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Sel},
    sel,
};
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
};

type Callback = Box<dyn Fn() -> bool>;
thread_local! {
    static REQUEST: RefCell<Option<Callback>> = RefCell::new(None);
}

pub(super) struct QuitHandler;
impl Drop for QuitHandler {
    fn drop(&mut self) {
        REQUEST.with(|request| {
            request.borrow_mut().take();
        });
    }
}

// NSApplicationTerminateReply is NSUInteger on macOS. AppKit calls on the
// main thread. Cancel native termination while the event-loop owner saves;
// its eventual event_loop.exit() retires the application normally.
unsafe extern "C" fn should_terminate(
    _delegate: *mut AnyObject,
    _selector: Sel,
    _application: *mut AnyObject,
) -> usize {
    let deferred = catch_unwind(AssertUnwindSafe(|| {
        REQUEST.with(|request| request.borrow().as_ref().is_some_and(|send| send()))
    }));
    match deferred {
        Ok(true) | Err(_) => 0, // NSTerminateCancel, including unexpected panic.
        Ok(false) => 1,         // No live application owner: NSTerminateNow.
    }
}

/// Called only from winit's main-thread resumed handler. Add the optional
/// delegate method that winit0.30.9 does not implement; never replace an
/// existing implementation. This covers the menu, Cmd-Q and Dock Quit.
pub(super) fn install(send: impl Fn() -> bool + 'static) -> anyhow::Result<QuitHandler> {
    // SAFETY: NSApplication exists by the time winit creates our window, and
    // resumed is main-thread-only on macOS. Borrow its live delegate; do not
    // retain or replace it, or change any of its existing method implementations.
    unsafe {
        let app: Retained<AnyObject> = msg_send_id![class!(NSApplication), sharedApplication];
        let delegate: *mut AnyObject = msg_send![&*app, delegate];
        let delegate = delegate
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No AppKit delegate"))?;
        let class = delegate.class();
        anyhow::ensure!(
            class.name() == "WinitApplicationDelegate",
            "Unreviewed AppKit delegate {}",
            class.name()
        );
        let selector = sel!(applicationShouldTerminate:);
        let class_ptr = (class as *const AnyClass).cast_mut().cast();
        let implementation = Some(std::mem::transmute::<
            unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
            unsafe extern "C" fn(),
        >(should_terminate));
        // Q = 64-bit unsigned reply; @ : @ = receiver, selector, application.
        // The callback's signature above matches this exact encoding.
        let added = ffi::class_addMethod(
            class_ptr,
            selector.as_ptr(),
            implementation,
            c"Q@:@".as_ptr(),
        );
        if added == ffi::NO {
            let method = ffi::class_getInstanceMethod(class_ptr, selector.as_ptr());
            anyhow::ensure!(
                !method.is_null(),
                "AppKit Quit callback registration failed"
            );
            let installed = ffi::method_getImplementation(method);
            anyhow::ensure!(
                installed.map(|f| f as usize) == implementation.map(|f| f as usize),
                "AppKit already owns a different Quit callback"
            );
        }
    }
    REQUEST.with(|request| *request.borrow_mut() = Some(Box::new(send)));
    Ok(QuitHandler)
}
