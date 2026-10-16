use super::*;
#[cfg(unix)]
mod save_failure_reference;

#[test]
fn opens_before_one_worker_and_notifies_after_exit_and_cooldown() {
    let mut task = SaveGameModel::new();
    assert_eq!(task.dialog.button_count(), 0);
    assert!(!task.dialog.cancel() && !task.dialog.confirm());
    assert_eq!(task.update(Ticks::from_f32(12.0), true), None);
    assert_eq!(task.phase, Phase::Opening);
    assert_eq!(task.update(Ticks::ZERO, true), Some(Event::StartWorker));
    for _ in 0..10 {
        assert_eq!(task.update(Ticks::from_f32(1000.0), false), None);
        assert_eq!(task.phase, Phase::Running);
    }
    assert_eq!(task.update(Ticks::ZERO, true), None);
    assert_eq!(task.update(Ticks::from_f32(12.0), true), None);
    assert!(!task.dialog.is_done());
    assert_eq!(task.update(Ticks::from_f32(4.0), true), None);
    assert_eq!(task.update(Ticks::from_f32(1.0), true), None);
    assert_eq!(task.phase, Phase::Notify);
    assert_eq!(task.update(Ticks::ZERO, true), Some(Event::FinishWorker));
    assert_eq!(task.update(Ticks::ZERO, true), None);
}

#[test]
fn pending_dialog_matches_complete_original_and_updated_native_traces() {
    let golden =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../reverse-engineering/native-save-dialog-timing-golden.txt"));
    let mut lines = golden.lines();
    for case in 0..128 {
        let input: Vec<_> = lines.next().unwrap().split_whitespace().collect();
        assert_eq!(input[0].parse::<usize>().unwrap(), case);
        let frames = input[1].parse::<usize>().unwrap();
        assert_eq!(input.len(), 2 + 2 * frames);
        let mut model = SaveGameModel::new();
        let (mut polls, mut starts, mut early, mut final_calls, mut owner_calls) =
            (0, 0, 0, 0, 0);
        let mut final_frame = None;
        for frame in 0..frames {
            let before = model.phase;
            let delta = input[2 + frame * 2].parse::<f32>().unwrap();
            let ready = input[3 + frame * 2] == "1";
            if before == Phase::Running {
                polls += 1;
            }
            match model.update(Ticks::from_f32(delta), ready) {
                Some(Event::StartWorker) => {
                    polls += 1;
                    starts += 1;
                }
                Some(Event::FinishWorker) => {
                    owner_calls += 1;
                    assert_eq!(Some(frame), final_frame.map(|at| at + 1));
                }
                None => {}
            }
            if model.dialog.take_intermediate_outcome().is_some() {
                early += 1;
            }
            if model.dialog.take_outcome().is_some() {
                final_calls += 1;
                final_frame = Some(frame);
            }
            let native: Vec<_> = lines.next().unwrap().split_whitespace().collect();
            assert_eq!(native[0].parse::<usize>().unwrap(), case);
            assert_eq!(native[1].parse::<usize>().unwrap(), frame);
            let phase = match model.phase {
                Phase::Opening => 1,
                Phase::Running => 2,
                Phase::Closing if early == 0 => 3,
                Phase::Closing => 4,
                Phase::Notify | Phase::Done => -1,
            };
            assert_eq!(
                phase,
                native[2].parse::<i32>().unwrap(),
                "case{case} frame{frame}"
            );
            for (value, bits) in [
                (model.dialog.scale.value(), native[3]),
                (model.dialog.opacity.value(), native[4]),
            ] {
                let expected = f32::from_bits(u32::from_str_radix(bits, 16).unwrap());
                assert!(
                    (value - expected).abs() <= 0.000001,
                    "case{case} frame{frame}: {value} != {expected}"
                );
            }
            assert_eq!(
                [polls, starts, early, final_calls],
                std::array::from_fn(|i| native[5 + i].parse::<usize>().unwrap()),
                "case{case} frame{frame}"
            );
        }
        assert_eq!((starts, early, final_calls, owner_calls), (1, 1, 1, 1));
    }
    assert!(lines.next().is_none());
}
