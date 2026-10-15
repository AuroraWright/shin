use super::*;
use crate::app::higurashi::{
    load_game::{Source, Storage},
    runtime_saves::Slots,
};
use shin_core::format::save::higurashi::SaveData;
use std::{
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

struct Held {
    entered: AtomicBool,
    gate: (Mutex<bool>, Condvar),
    bytes: Mutex<Vec<u8>>,
}
impl Storage for Held {
    fn trial_token(&self) -> bool {
        false
    }
    fn read(&self, _: Source) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    fn trial_exists(&self) -> Result<bool> {
        Ok(false)
    }
    fn write(&self, bytes: &[u8]) -> Result<()> {
        *self.bytes.lock().unwrap() = bytes.to_vec();
        self.entered.store(true, Ordering::SeqCst);
        let mut open = self.gate.0.lock().unwrap();
        while !*open {
            open = self.gate.1.wait(open).unwrap();
        }
        Ok(())
    }
}
struct Release(Arc<Held>);
impl Drop for Release {
    fn drop(&mut self) {
        *self.0.gate.0.lock().unwrap() = true;
        self.0.gate.1.notify_all();
    }
}

#[test]
fn higurashi_shutdown_writer_freezes_full_rows_and_waits_for_actual_io() {
    crate::init_test_task_pools();
    let storage = Arc::new(Held {
        entered: AtomicBool::new(false),
        gate: (Mutex::new(false), Condvar::new()),
        bytes: Mutex::new(Vec::new()),
    });
    let mut data = SaveData::default();
    data.save_vectors.select_last_choices = vec![0xfe, 0x91];
    let mut slots = Slots::loaded(&data);
    slots.manual[99].valid = true;
    slots.manual[99].data.progress.extra_word = 0xdead_beef;
    slots.manual[99].data.progress.entries[1].response_data = vec![0xf1, 0xe2];
    slots.manual[99].data.progress.set_active_index(1).unwrap();
    slots.manual[99].data.trailing_values = vec![0xffff, 0x1234];
    let mut writer = Writer::new(FrozenProgress {
        data: data.clone(),
        slots: Some(slots.clone()),
        storage: storage.clone(),
    });
    // Release first during unwinding, before Writer joins the controlled IO.
    let _release = Release(storage.clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !storage.entered.load(Ordering::SeqCst) {
        assert!(writer.poll().is_none());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(writer.poll().is_none());
    assert!(!writer.retirement.is_finished());
    data.settings.common.v0_bgmvol = 19;
    slots.manual[99].data.progress.extra_word = 0xbbbb0005;
    let frozen = writer.snapshot.slots.as_ref().unwrap();
    assert_eq!(frozen.manual[99].data.progress.extra_word, 0xdead_beef);
    assert_eq!(
        frozen.manual[99].data.progress.entries[1].response_data,
        vec![0xf1, 0xe2]
    );
    assert_eq!(frozen.manual[99].data.trailing_values, vec![0xffff, 0x1234]);
    let decoded = SaveData::decode(&storage.bytes.lock().unwrap()).unwrap();
    assert_eq!(decoded.settings.common.v0_bgmvol, 40);
    assert_eq!(decoded.save_vectors.select_last_choices, vec![14, 1]);
    assert_eq!(
        decoded.manual_save_slots[99].as_ref().unwrap().extra_word,
        7
    );
    assert_eq!(
        decoded.manual_save_slots[99]
            .as_ref()
            .unwrap()
            .trailing_values,
        vec![1023, 0x234]
    );
    *storage.gate.0.lock().unwrap() = true;
    storage.gate.1.notify_all();
    while writer.poll().is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(writer);
    assert_eq!(data.settings.common.v0_bgmvol, 19);
    assert_eq!(slots.manual[99].data.progress.extra_word, 0xbbbb0005);
}

#[test]
fn higurashi_shutdown_before_loaded_progress_cannot_create_a_writer() {
    let mut shutdown = Shutdown::default();
    shutdown.request();
    assert!(shutdown.requested());
    shutdown.begin(None);
    assert!(matches!(shutdown.phase, Phase::Ready));
    assert!(shutdown.writer_retirement().is_none());
}
