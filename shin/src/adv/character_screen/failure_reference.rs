//! Character sprite failure/retention/reselection through the production owner.
use super::*;
use crate::{
    adv::assets::AdvAssets,
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache, supplied_payload},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires supplied ROM and Metal; temporary sprite files only, no audio/window/save writes"]
fn reference_character_sprite_failure_retains_display_and_retries() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_REFERENCE_SPRITE_FAILURE_OUTPUT").unwrap());
    let directory = output.join("overrides");
    std::fs::create_dir_all(directory.join("chars")).unwrap();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut cases = Vec::new();
    for profile in ["short-header", "empty-archive"] {
        let mut io = LayeredAssetIo::new();
        io.try_with_dir(&directory).unwrap();
        if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
            io.try_with_rom(patch).unwrap();
        }
        io.try_with_rom(std::env::var_os("SHIN_REFERENCE_ROM").unwrap())
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
        let assets = shin_tasks::block_on(AdvAssets::load(&server)).unwrap();
        let mut screen = CharacterScreen::new(&server, &assets.scenario, 0, 0).unwrap();
        fn path(screen: &CharacterScreen) -> String {
            format!(
                "/chars/{}.txa",
                screen
                    .model
                    .characters
                    .selected_cell()
                    .unwrap()
                    .definition()
                    .state()
                    .unwrap()
                    .variant()
                    .unwrap()
                    .full_sprite
                    .to_ascii_lowercase()
            )
        }
        let first_path = path(&screen);
        assert_ne!(first_path, "/chars/.txa");
        fn wait(screen: &mut CharacterScreen, server: &Arc<AssetServer>) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while screen.pending_full_sprite.is_some() {
                assert!(Instant::now() < deadline);
                screen.update(Ticks::ZERO, server);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        wait(&mut screen, &server);
        let retained = screen.full_sprite.as_ref().unwrap().clone();
        let first = screen.model.characters.selected_index().unwrap();
        let other = screen
            .model
            .characters
            .cells
            .iter()
            .enumerate()
            .find_map(|(i, c)| {
                let v = c.definition().state()?.variant()?;
                (c.page == screen.model.characters.page
                    && c.is_selectable()
                    && !v.full_sprite.is_empty()
                    && format!("/chars/{}.txa", v.full_sprite.to_ascii_lowercase()) != first_path)
                    .then_some(i)
            })
            .expect("Supplied grid has another selectable full sprite on the initial page");
        let other_sprite = screen.model.characters.cells[other]
            .definition()
            .state()
            .unwrap()
            .variant()
            .unwrap()
            .full_sprite
            .to_ascii_lowercase();
        let other_path = format!("/chars/{other_sprite}.txa");
        let file = directory.join(&other_path[1..]);
        let bytes = supplied_payload(&other_path).2;
        let bad = if profile == "short-header" {
            bytes[..15].to_vec()
        } else {
            let mut b = vec![0u8; 32];
            b[..4].copy_from_slice(b"TXA4");
            b[4..8].copy_from_slice(&2u32.to_le_bytes());
            b[8..12].copy_from_slice(&32u32.to_le_bytes());
            b
        };
        std::fs::write(&file, &bad).unwrap();
        assert!(screen.model.characters.select(other));
        screen.model.details_changed();
        screen.update(Ticks::ZERO, &server);
        wait(&mut screen, &server);
        assert!(
            Arc::ptr_eq(screen.full_sprite.as_ref().unwrap(), &retained),
            "failed sprite must retain displayed owner"
        );
        let selected = screen.model.characters.selected_index();
        let text = screen.text.as_ref().unwrap().layout.height;
        assert_eq!(selected, Some(other));
        // Failure retires once; steady frames do not retry or replace displayed sprite.
        for _ in 0..4 {
            screen.update(Ticks::ZERO, &server);
            assert!(screen.pending_full_sprite.is_none());
        }
        std::fs::write(&file, &bytes).unwrap();
        assert!(screen.model.characters.select(first));
        screen.model.details_changed();
        screen.update(Ticks::ZERO, &server);
        wait(&mut screen, &server);
        assert!(Arc::ptr_eq(screen.full_sprite.as_ref().unwrap(), &retained));
        assert!(screen.model.characters.select(other));
        screen.model.details_changed();
        screen.update(Ticks::ZERO, &server);
        wait(&mut screen, &server);
        let expected: Arc<CharacterTextures> = server.load_sync(&other_path).unwrap();
        assert!(Arc::ptr_eq(screen.full_sprite.as_ref().unwrap(), &expected));
        assert!(!Arc::ptr_eq(&expected, &retained));
        assert_eq!(screen.model.characters.selected_index(), selected);
        assert_eq!(screen.text.as_ref().unwrap().layout.height, text);
        std::fs::remove_file(file).unwrap();
        cases.push(serde_json::json!({"profile":profile,"first_asset":first_path,"failed_asset":other_path,
            "display_owner_retained":true,"selected_text_preserved":true,"failed_request_retired":true,
            "no_per_frame_retry":true,"same_screen_same_server_reselection_retry":true}));
    }
    std::fs::write(output.join("sprite-recovery.json"),serde_json::to_vec_pretty(&serde_json::json!({"cases_passed":cases.len(),"cases":cases,
        "patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),"audio_device_opened":false,"window_created":false,"working_progress_written":false})).unwrap()).unwrap();
    println!(
        "{} character sprite failures retain displayed owner and retry on reselection",
        cases.len()
    );
}

#[test]
#[ignore = "requires supplied ROM and Metal; read-only artwork validation, no audio/window/save writes"]
fn reference_character_artwork_validation_covers_supplied_grids() {
    crate::init_test_task_pools();
    let output =
        PathBuf::from(std::env::var_os("SHIN_REFERENCE_CHARACTER_ARTWORK_OUTPUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let mut io = LayeredAssetIo::new();
    if let Some(patch) = std::env::var_os("SHIN_REFERENCE_PATCH_ROM") {
        io.try_with_rom(patch).unwrap();
    }
    io.try_with_rom(std::env::var_os("SHIN_REFERENCE_ROM").unwrap())
        .unwrap();
    let server = Arc::new(AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Umineko,
            higurashi_bustup_cache: Default::default(),
            wgpu_device: device,
            wgpu_queue: queue,
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    ));
    let assets = shin_tasks::block_on(AdvAssets::load(&server)).unwrap();
    let atlas: Arc<CharacterTextures> = server.load_sync("/chars.txa").unwrap();
    let tables = assets.scenario.info_tables();
    let mut grids = Vec::new();
    for (index, grid) in tables.chars_grid_info.iter().enumerate() {
        let model = CharacterModel::new(grid, &tables.chars_sprite_info, index, 0);
        atlas
            .validate_character_screen(&model)
            .unwrap_or_else(|e| panic!("Supplied grid {index} rejected: {e:#}"));
        grids.push(serde_json::json!({"grid":index,"cells":model.cells.len(),"editable":model.editable,"page_mask":model.page_mask}));
    }
    let mut model =
        CharacterModel::new(&tables.chars_grid_info[0], &tables.chars_sprite_info, 0, 0);
    let definition = model
        .cells
        .iter_mut()
        .flat_map(|c| &mut c.definitions)
        .find(|d| d.default_state != 0 && d.states.iter().any(|s| !s.variants.is_empty()))
        .unwrap();
    let variant = definition
        .states
        .iter_mut()
        .flat_map(|s| &mut s.variants)
        .next()
        .unwrap();
    let prior = std::mem::replace(
        &mut variant.portrait,
        "__missing_portrait_recovery_probe__".into(),
    );
    let error = atlas.validate_character_screen(&model).unwrap_err();
    assert!(format!("{error:#}").contains("__missing_portrait_recovery_probe__"));
    assert!(format!("{error:#}").contains("/chars.txa"));
    std::fs::write(output.join("artwork-validation.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "grids_passed":grids.len(),"grids":grids,"missing_portrait_control":true,"original_portrait":prior,
        "error":format!("{error:#}"),"patch_mounted":std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some(),
        "archives_modified":false,"audio_device_opened":false,"window_created":false,"working_progress_written":false
    })).unwrap()).unwrap();
    println!(
        "{} supplied character grids accepted; missing reachable portrait returns an error",
        grids.len()
    );
}
