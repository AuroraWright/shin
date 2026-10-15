#[test]
#[ignore = "requires supplied ROM and Metal; writes only an isolated temporary save, no audio"]
fn reference_save_failure_waits_for_acknowledgement() {
    use super::super::{VmState, persistence::ExplicitSave};
    use crate::asset::system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache};
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc};
    crate::init_test_task_pools();
    let adapter =
        shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
            adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
        } else {
            wgpu::Features::empty()
        },
            ..Default::default()
        }))
    .unwrap();
    let mut io = LayeredAssetIo::new();
    if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Umineko,
            higurashi_bustup_cache: Default::default(),
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let directory = std::env::temp_dir().join(format!("shin-save-failure-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let _cleanup = Cleanup(directory.clone());
    let path = directory.join("progress.dat");
    let mut state = VmState::for_scenario(7, 42);
    state.primary_entry = Some(VmState::for_scenario(19, 99).snapshot_entry());
    let previous = state.snapshot_game_data();
    let mut saved = shin_core::format::save::SaveData::default();
    saved.manual_save_slots[99] = Some(previous.clone());
    saved.save_menu_position = 17;
    let original = saved.encode();
    fs::write(&path, &original).unwrap();
    let mut store = SaveStore::open(path.clone(), None).unwrap();
    let previous = store.data.manual_save_slots[99].clone().unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
    store
        .prepare_explicit(
            &state,
            ExplicitSave::Slot {
                slot: 99,
                game: Some(VmState::for_scenario(21, 1).snapshot_game_data()),
            },
        )
        .unwrap();
    let mut task = SaveGameTask::load(&server).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = task.update(Ticks::from_f32(1.0), Some(&mut store)) {
            assert!(
                result.is_err(),
                "Read-only temporary directory must reject the save"
            );
            panic!("Failed save must retain a visible acknowledgement before returning to its owner: {result:?}");
        }
        if task.model.phase == Phase::Done {
            break;
        }
        assert!(Instant::now() < deadline, "Save failure dialog stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(store.data.save_menu_position, 17);
    assert_eq!(
        format!("{:?}", store.data.manual_save_slots[99]),
        format!("{:?}", Some(previous))
    );
    for _ in 0..30 {
        assert!(task
            .update(Ticks::from_f32(1.0), Some(&mut store))
            .is_none());
    }
    assert!(task.reference_failure_is_open());
    let mut actions = EnumMap::default();
    actions[AppAction::Cancel] = ActionState {
        is_held: true,
        is_clicked: true,
        ..Default::default()
    };
    task.keyboard(&actions);
    assert!(task
        .update(Ticks::from_f32(30.0), Some(&mut store))
        .is_none());
    assert!(
        task.reference_failure_is_open(),
        "Cancel cannot silently dismiss failed saves"
    );
    let rect = task.error_view.geometry.buttons[0];
    let center = task.error_view.geometry.origin
        + Vec2::new(30.0, 46.0)
        + rect.truncate().truncate()
        + Vec2::new(rect.z, rect.w) * 0.5;
    task.pointer_state(center, false, 1.0 / 60.0, (1280, 720));
    task.pointer_state(center, true, 1.0 / 60.0, (1280, 720));
    assert!(
        task.update(Ticks::ZERO, Some(&mut store)).is_none(),
        "Press alone cannot acknowledge"
    );
    task.pointer_state(center, false, 1.0 / 60.0, (1280, 720));
    assert_eq!(task.take_sounds(), vec!["decide"]);
    let mut result = None;
    for _ in 0..100 {
        result = task.update(Ticks::from_f32(1.0), Some(&mut store));
        if result.is_some() {
            break;
        }
    }
    assert!(result
        .unwrap()
        .unwrap_err()
        .to_string()
        .contains("temporary save"));
    assert!(
        task.update(Ticks::ZERO, Some(&mut store)).is_none(),
        "Failure callback must be delivered once"
    );
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let replacement = VmState::for_scenario(21, 1).snapshot_game_data();
    store
        .prepare_explicit(
            &state,
            ExplicitSave::Slot {
                slot: 99,
                game: Some(replacement.clone()),
            },
        )
        .unwrap();
    let mut retry = SaveGameTask::load(&server).unwrap();
    let start = Instant::now();
    loop {
        if let Some(result) = retry.update(Ticks::from_f32(1.0), Some(&mut store)) {
            result.unwrap();
            break;
        }
        assert!(retry.failure.is_none());
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(start.elapsed() >= Duration::from_secs(1));
    let reopened = SaveStore::open(path, None).unwrap();
    assert_eq!(reopened.data.save_menu_position, 99);
    let actual = reopened.data.manual_save_slots[99].as_ref().unwrap();
    assert_eq!(
        format!("{:?}", (&actual.entry, &actual.primary_entry)),
        format!("{:?}", (&replacement.entry, &replacement.primary_entry))
    );
}
