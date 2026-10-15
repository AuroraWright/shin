#![cfg(target_os = "macos")]

extern crate block;

use block::ConcreteBlock;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[test]
fn copied_block_invokes_captures_and_releases_once() {
    struct Capture(Arc<AtomicUsize>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = Capture(drops.clone());
    let stack = ConcreteBlock::new(move |value: i32| {
        let _keep_capture_alive = &capture;
        value + 7
    });
    let heap = stack.copy();
    assert_eq!(unsafe { heap.call((35,)) }, 42);
    let second_owner = heap.clone();
    drop(heap);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert_eq!(unsafe { second_owner.call((5,)) }, 12);
    drop(second_owner);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
