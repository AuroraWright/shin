use super::*;
use crate::adv::higurashi::{audio::HigurashiAudio, reference_gpu_assets};
use shin_core::format::scenario::higurashi::HigurashiScenario;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
#[ignore = "live MSGSET11780 clock/copy retry and immediate/waiting messages on offscreen Metal and exact Loopback Audio"]
fn higurashi_message_start_copies_loaded_progress_once_before_waiting() {
    let (_, _, assets) = reference_gpu_assets();
    let assets = Arc::new(assets);
    let authored: Arc<HigurashiScenario> = assets.load_sync("/main.snr").unwrap();
    // First message continues immediately, then WAIT; second waits for input.
    let code = [
        0x86, 1, 0, 0, 0, 0x7f, 4, 0, b'@', b'r', b'A', 0, 0x83, 1, 30, 0x86, 2, 0, 0, 1, 0x7f, 4,
        0, b'@', b'r', b'B', 0, 0, 0, 0,
    ];
    let mut raw = authored.raw().to_vec();
    let start = authored.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let script = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let runner = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        script.clone(),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let mut host = HigurashiHost::new(
        runner,
        HigurashiAudio::new(
            Arc::new(shin_audio::AudioManager::new_for_tests()),
            assets,
            script,
        ),
    );
    let date = SaveDate {
        year: 2026,
        month: 10,
        day: 12,
        hour: 12,
        minute: 45,
        second: 19,
    };
    let mut saved = SaveData::default();
    let mut prior_slot = fresh();
    prior_slot.trailing_values = vec![1, 0xffff, 0x1234];
    prior_slot.byte_variables = vec![0, 255, 27];
    saved.auto_save_slot = Some(prior_slot.clone());
    saved.manual_save_slots[2] = Some(prior_slot.clone());
    let mut data = fresh();
    data.entry.random_seed = 42;
    data.entry.response_data = vec![0x15, 0xff, 0];
    let mut bindings = Bindings::new(&saved, &data).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let mut clock: Box<dyn FnMut() -> Result<SaveDate> + Send> = Box::new(move || {
        if counter.fetch_add(1, Ordering::SeqCst) == 0 {
            bail!("controlled clock unavailable");
        }
        Ok(date)
    });
    let update = |host: &mut HigurashiHost,
                  bindings: &mut Bindings,
                  saved: &mut SaveData,
                  clock: &mut Box<dyn FnMut() -> Result<SaveDate> + Send>| {
        shin_tasks::block_on(host.update_native(
            Ticks::from_f32(3.0),
            policy(),
            reads(),
            &mut bindings::LiveReads {
                bindings,
                saved,
                clock,
            },
            audio(),
            32,
            10000,
            || [1, 2, 3, 4],
        ))
    };
    assert!(update(&mut host, &mut bindings, &mut saved, &mut clock).is_err());
    assert_eq!(host.runner.completed_commands(), 0);
    assert!(host.runner.frame().message.glyphs().is_some());
    assert_eq!(host.runner.frame().message.playback.message_id, 0);
    assert_eq!(bindings.progress.progress().active_entry().save_position, 1);
    assert!(bindings.autosave.valid);
    assert_eq!(saved.auto_save_slot, Some(prior_slot.clone()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        update(&mut host, &mut bindings, &mut saved, &mut clock).unwrap(),
        Status::Waiting
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(host.runner.completed_commands(), 1);
    for _ in 0..5 {
        assert_eq!(
            update(&mut host, &mut bindings, &mut saved, &mut clock).unwrap(),
            Status::Waiting
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    for _ in 0..20 {
        update(&mut host, &mut bindings, &mut saved, &mut clock).unwrap();
        if host.runner.frame().message.playback.message_id == 1 {
            break;
        }
    }
    assert_eq!(host.runner.frame().message.playback.message_id, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    for _ in 0..5 {
        update(&mut host, &mut bindings, &mut saved, &mut clock).unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let slot = saved.auto_save_slot.as_ref().unwrap();
    assert_eq!(slot.date, date);
    assert_eq!(slot.entry.save_position, 2);
    assert_eq!(slot.entry.replay_target, 2);
    assert_eq!(slot.entry.response_data, data.entry.response_data);
    assert_eq!(slot.trailing_values, prior_slot.trailing_values);
    assert_eq!(slot.byte_variables, prior_slot.byte_variables);
    assert_eq!(saved.manual_save_slots[2], Some(prior_slot));
    assert_eq!(bindings.date, data.date);
    assert!(!host.ui.indicator.transition.is_waiting());
    eprintln!(
        "MSGSET progress copy:clock failure retains installed message,retry/once-per-message,immediate and waiting messages,current boundaries/response tails,loaded target vectors/manual slot/date and no notification pass"
    );
}
