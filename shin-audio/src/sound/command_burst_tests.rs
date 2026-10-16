//! A game update can publish many commands before the next hardware callback.
use super::*;
use crate::AudioHandle;

fn send(handle: &mut AudioHandle, index: usize) {
    let tween = Tween::linear(Ticks::from_f32(3.0));
    match index % 10 {
        0 => handle.set_parameters(Volume(0.6), Pan(-0.25), 1.5, tween),
        1 => handle.enqueue_volume(Volume(0.4), tween),
        2 => handle.enqueue_panning(Pan(0.75), tween),
        3 => handle.finish_volume_and_pan(),
        4 => handle.stop(tween),
        5 => handle.resume_parameters(Volume(0.7), Pan(-0.5), 0.75, tween),
        6 => handle.set_playback_rate(2.0, tween),
        7 => handle.finish_volume_and_stop(tween),
        8 => handle.resume_parameters(Volume(0.8), Pan(0.25), 1.25, tween),
        _ => handle.set_volume(Volume(0.5), tween),
    }
    .unwrap();
}

#[test]
fn command_burst_preserves_fifo_pcm_and_waits_across_callback_delays() {
    fn render(burst: bool) -> (Vec<Frame>, Vec<i32>, Vec<u32>) {
        let mut manager = tests::manager();
        let mut handle = manager.play(tests::sound(10_000, 0.0)).unwrap();
        manager.backend_mut().on_start_processing();
        for index in 0..512 {
            send(&mut handle, index);
            if !burst {
                // Deliver commands separately without advancing sound time.
                manager.backend_mut().on_start_processing();
            }
        }
        handle
            .resume_parameters(Volume(0.65), Pan(0.1), 1.0, Tween::IMMEDIATE)
            .unwrap();
        handle
            .enqueue_volume(Volume(0.2), Tween::linear(Ticks::from_f32(6.0)))
            .unwrap();
        handle
            .enqueue_panning(Pan(-0.8), Tween::linear(Ticks::from_f32(6.0)))
            .unwrap();
        let mut pcm = Vec::new();
        let mut waits = Vec::new();
        let mut positions = Vec::new();
        for _ in 0..240 {
            manager.backend_mut().on_start_processing();
            waits.push(handle.get_wait_status().bits());
            positions.push(handle.position_samples());
            pcm.push(manager.backend_mut().process());
        }
        assert!(
            waits
                .iter()
                .any(|bits| bits & AudioWaitStatus::VOLUME_TWEENING.bits() != 0)
        );
        assert_eq!(*waits.last().unwrap(), AudioWaitStatus::PLAYING.bits());
        assert!(positions.last().unwrap() > &0);
        (pcm, waits, positions)
    }
    assert_eq!(render(true), render(false));
}

#[test]
fn command_burst_immediate_retirement_wins_over_pending_resumes() {
    let mut manager = tests::manager();
    let mut handle = manager.play(tests::sound(10_000, 0.0)).unwrap();
    manager.backend_mut().on_start_processing();
    for index in 0..512 {
        send(&mut handle, index);
    }
    handle.stop_immediate();
    manager.backend_mut().on_start_processing();
    assert!(handle.get_wait_status().is_empty());
    assert_eq!(manager.backend_mut().process(), Frame::ZERO);
    assert_eq!(handle.position_samples(), 0);
}

#[test]
fn command_burst_retired_sources_ignore_late_commands_but_reject_invalid_rates() {
    use kira::sound::SoundData;
    let (sound, mut handle) = tests::sound(100, 0.0).into_sound().unwrap();
    drop(sound);
    for index in 0..512 {
        send(&mut handle, index);
    }
    for rate in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(handle.set_playback_rate(rate, Tween::IMMEDIATE).is_err());
        assert!(
            handle
                .set_parameters(Volume(1.0), Pan(0.0), rate, Tween::IMMEDIATE)
                .is_err()
        );
        assert!(
            handle
                .resume_parameters(Volume(1.0), Pan(0.0), rate, Tween::IMMEDIATE)
                .is_err()
        );
    }
}

#[test]
fn command_burst_ordered_appends_preserve_every_target_pcm() {
    fn render(burst: bool, reverse: bool) -> Vec<Frame> {
        let mut manager = tests::manager();
        let mut handle = manager.play(tests::sound(10_000, 0.0)).unwrap();
        tests::advance(&mut manager, 32);
        // Every target remains in the interpolator queue: no final retarget
        // can erase the early commands and conceal a delivery-order defect.
        for item in 0..128 {
            let index = if reverse { 127 - item } else { item };
            let volume = 0.1 + (index % 7) as f32 * 0.1;
            let pan = ((index * 3) % 9) as f32 * 0.2 - 0.8;
            let tween = Tween::linear(Ticks::from_millis(4.0));
            handle.enqueue_volume(Volume(volume), tween).unwrap();
            handle.enqueue_panning(Pan(pan), tween).unwrap();
            if !burst {
                manager.backend_mut().on_start_processing();
            }
        }
        let mut pcm = Vec::new();
        for index in 0..128 {
            let mut frame = Frame::ZERO;
            for _ in 0..4 {
                manager.backend_mut().on_start_processing();
                frame = manager.backend_mut().process();
                pcm.push(frame);
            }
            let target = if reverse { 127 - index } else { index };
            let volume = 0.1 + (target % 7) as f32 * 0.1;
            let pan = ((target * 3) % 9) as f32 * 0.2 - 0.8;
            let mix = (pan + 1.0) * 0.5;
            let expected = Frame::new(
                0.25 * volume * (1.0 - mix).sqrt() * SQRT_2,
                0.25 * volume * mix.sqrt() * SQRT_2,
            );
            assert!(
                (frame.left - expected.left).abs() < 0.00002,
                "left target{target}: {frame:?} vs {expected:?}"
            );
            assert!(
                (frame.right - expected.right).abs() < 0.00002,
                "right target{target}: {frame:?} vs {expected:?}"
            );
        }
        pcm
    }
    let burst = render(true, false);
    assert_eq!(burst, render(false, false));
    assert_ne!(
        burst,
        render(false, true),
        "The oracle must detect reversed delivery"
    );
}
