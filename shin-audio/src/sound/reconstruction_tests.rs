use super::tests::{advance, manager, sound};
use shin_core::{
    time::{Ticks, Tween},
    vm::command::types::{AudioWaitStatus, Volume},
};

#[test]
fn older_stop_finishes_queued_volume_and_reconstruction_restarts_tail_from_silence() {
    let mut manager = manager();
    let mut data = sound(10000, 0.0);
    data.settings.volume = Volume(0.2);
    let mut handle = manager.play(data).unwrap();
    handle
        .enqueue_volume(Volume(0.6), Tween::linear(Ticks::from_millis(100.0)))
        .unwrap();
    handle
        .enqueue_volume(Volume(0.8), Tween::linear(Ticks::from_millis(100.0)))
        .unwrap();
    advance(&mut manager, 20);
    handle
        .finish_volume_and_stop(Tween::linear(Ticks::from_millis(100.0)))
        .unwrap();
    let half = advance(&mut manager, 50);
    assert!(
        (half.left - 0.1).abs() < 0.00001,
        "must finish target0.8 before half fade: {half:?}"
    );
    assert!(handle.get_wait_status().contains(AudioWaitStatus::PLAYING));
    handle
        .finish_volume_and_stop(Tween::linear(Ticks::from_millis(40.0)))
        .unwrap();
    let silent = advance(&mut manager, 20);
    assert_eq!(silent.left, 0.0);
    assert!(
        handle.get_wait_status().contains(AudioWaitStatus::PLAYING),
        "tail stays owned until restarted timer finishes"
    );
    advance(&mut manager, 25);
    assert!(handle.get_wait_status().is_empty());
}
#[test]
fn older_skip_finishes_volume_and_pan_while_speed_continues() {
    let mut manager = manager();
    let mut handle = manager.play(sound(10000, 0.0)).unwrap();
    handle
        .enqueue_volume(Volume(0.2), Tween::linear(Ticks::from_millis(100.0)))
        .unwrap();
    handle
        .enqueue_volume(Volume(0.8), Tween::linear(Ticks::from_millis(100.0)))
        .unwrap();
    handle
        .enqueue_panning(
            shin_core::vm::command::types::Pan(0.0),
            Tween::linear(Ticks::from_millis(100.0)),
        )
        .unwrap();
    handle
        .set_playback_rate(2.0, Tween::linear(Ticks::from_millis(200.0)))
        .unwrap();
    // past the resampler window's start, where its taps still hold silence
    advance(&mut manager, 30);
    handle.finish_volume_and_pan().unwrap();
    let output = advance(&mut manager, 1);
    assert!(
        (output.left - 0.2).abs() < 0.00001,
        "finished volume must be0.8:{output:?}"
    );
    let status = handle.get_wait_status().bits();
    assert_eq!(status & (4 | 8), 0);
    assert_ne!(status & 16, 0);
    assert!(handle.get_wait_status().contains(AudioWaitStatus::PLAYING));
}
