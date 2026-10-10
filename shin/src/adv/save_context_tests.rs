mod save_context {
    use super::*;
    use crate::adv::{
        persistence::SaveStore,
        save_menu::{SaveMenuInput, SaveMenuModel, SaveMenuPhase, SaveMenuRequest},
        system_menu::SystemMenuSnapshot,
    };

    fn game(position: u32) -> GameData {
        let mut game = GameData::from_entry(entry(position));
        let mut inactive = entry(0xffff_fffe);
        inactive.scenario_id = 19;
        inactive.random_seed = 0xabcdef01;
        game.primary_entry = Some(inactive);
        game
    }
    fn primary(game: &GameData) -> String {
        format!("{:?}", game.primary_entry)
    }
    fn confirm(model: &mut SaveMenuModel) {
        model.assets_ready();
        model.update(10.0);
        model.input(SaveMenuInput {
            confirm: true,
            ..Default::default()
        });
        for _ in 0..10 {
            model.update(1.0);
        }
        assert_eq!(model.phase(), SaveMenuPhase::Confirmation);
        model.take_requests();
    }

    #[test]
    fn inactive_context_survives_all_replay_boundaries_and_new_autosaves() {
        let scenario = scenario();
        let globals = SaveData::default();
        for position in 0..=5 {
            let game = game(position);
            let expected = primary(&game);
            let mut scene =
                ReconstructedScene::from_game_data(&scenario, &game, &globals, 100).unwrap();
            for phase in 0..4 {
                scene.state.save_position = position + phase;
                scene.state.route.record_choice(phase as i32);
                scene.state.capture_autosave();
                let saved = scene.state.auto_save_slot.as_ref().unwrap();
                assert_eq!(primary(saved), expected);
                assert_eq!(saved.entry.save_position, position + phase);
                assert_eq!(saved.entry.scenario_id, 7);
                assert_eq!(saved.entry.random_seed, 42);
            }
            let revision = scene.state.save_revision;
            let saved = format!("{:?}", scene.state.auto_save_slot);
            scene.state.persistent_commits_enabled = false;
            scene.state.capture_autosave();
            assert_eq!(scene.state.save_revision, revision);
            assert_eq!(format!("{:?}", scene.state.auto_save_slot), saved);
        }
        // Starting a new single-entry scene cannot inherit a previous slot's context.
        let scene = ReconstructedScene::from_game_data(
            &scenario,
            &GameData::from_entry(entry(1)),
            &globals,
            100,
        )
        .unwrap();
        assert!(scene.state.primary_entry.is_none());
        assert!(VmState::for_scenario(7, 42).primary_entry.is_none());
    }

    #[test]
    fn full_context_survives_menu_snapshot_disk_save_and_load_handoff() {
        let scenario = scenario();
        let game = game(2);
        let expected = primary(&game);
        let mut scene =
            ReconstructedScene::from_game_data(&scenario, &game, &SaveData::default(), 100)
                .unwrap();
        scene.state.capture_autosave();
        let retained = SystemMenuSnapshot::from_vm(&scene.state);
        let mut menu = SaveMenuModel::new(&SaveData::default(), Some(retained.entry));
        // The live VM may change while the menu owns its opening snapshot.
        scene.state.primary_entry = None;
        scene.state.save_position = 99;
        confirm(&mut menu);
        menu.dialog_response(0, 0);
        let requests = menu.take_requests();
        let [
            SaveMenuRequest::Save {
                slot: 0,
                game: saved,
            },
        ] = requests.as_slice()
        else {
            panic!("Missing complete save request")
        };
        let mut saved = saved.clone();
        saved.refresh_timestamp();
        assert_eq!(primary(&saved), expected);
        assert_eq!(saved.entry.save_position, 2);
        let directory =
            std::env::temp_dir().join(format!("shin-save-context-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("progress.dat");
        let mut store = SaveStore::open(path.clone(), None).unwrap();
        store
            .write_manual_slot(&scene.state, 0, saved.clone())
            .unwrap();
        menu.write_finished(true, Some(saved));
        menu.dialog_response(0, 1);
        assert_eq!(menu.phase(), SaveMenuPhase::Active);
        let disk = SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            primary(disk.manual_save_slots[0].as_ref().unwrap()),
            expected
        );
        assert_eq!(primary(disk.auto_save_slot.as_ref().unwrap()), expected);
        let mut load = SaveMenuModel::new(&disk, None);
        confirm(&mut load);
        load.dialog_response(0, 0);
        assert!(load.take_requests().is_empty());
        load.dialog_response(0, 1);
        load.update(10.0);
        let requests = load.take_requests();
        let [
            SaveMenuRequest::Load {
                slot: 0,
                game: loaded,
            },
        ] = requests.as_slice()
        else {
            panic!("Missing complete load request")
        };
        assert_eq!(primary(loaded), expected);
        let mut restored =
            ReconstructedScene::from_game_data(&scenario, loaded, &disk, 100).unwrap();
        assert_eq!(restored.state.save_position, 2);
        restored.state.capture_autosave();
        assert_eq!(
            primary(restored.state.auto_save_slot.as_ref().unwrap()),
            expected
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
