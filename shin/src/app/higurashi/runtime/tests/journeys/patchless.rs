//! Real asset discovery/startup on base, partial and updated content; inputs
//! enter through the normal Runtime. No game inputs or user saves are changed.
use super::*;
use crate::{
    asset::system::{AssetLoadContext, AssetServer, locate_assets_for_game},
    cli::Game,
};
use shin_core::format::scenario::higurashi::HigurashiVariant;
use std::os::unix::fs::symlink;

impl Journey {
    fn with_archives(patch: bool, append: bool, partial: bool, old: &[u8]) -> Self {
        let temporary = TemporaryProgress(std::env::temp_dir().join(format!(
            "higurashi-patchless-{}-{}-{}-{}",
            std::process::id(),
            u8::from(patch),
            u8::from(append),
            u8::from(partial)
        )));
        assert!(!temporary.0.exists());
        let inputs = temporary.0.join("inputs");
        std::fs::create_dir_all(&inputs).unwrap();
        let root = PathBuf::from(std::env::var_os("SHIN_HIGURASHI_ASSETS").unwrap());
        symlink(root.join("data.rom"), inputs.join("data.rom")).unwrap();
        if partial {
            // A valid older/partial patch has only an updated scenario and
            // Config archive. Every other archive must fall back independently.
            use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
            use std::io::Read;
            let layer = inputs.join("patch");
            std::fs::create_dir_all(&layer).unwrap();
            let mut rom = RomReader::new(
                StatelessFile::new(std::fs::File::open(root.join("patch.rom")).unwrap()).unwrap(),
            )
            .unwrap();
            for path in ["/main.snr", "/config.txa"] {
                let index = rom.find_file(path).unwrap();
                let mut bytes = Vec::new();
                rom.open_file(index)
                    .unwrap()
                    .read_to_end(&mut bytes)
                    .unwrap();
                std::fs::write(layer.join(&path[1..]), bytes).unwrap();
            }
        } else if patch {
            symlink(root.join("patch.rom"), inputs.join("patch.rom")).unwrap();
        }
        if append {
            symlink(root.join("append.rom"), inputs.join("append.rom")).unwrap();
        }
        let paths = Paths {
            current: Some(temporary.0.join("progress.dat")),
            trial: None,
        };
        std::fs::write(paths.current.as_ref().unwrap(), old).unwrap();
        let storage = Arc::new(ObservedPaths {
            paths,
            reads: StdMutex::new(Vec::new()),
            writes: StdMutex::new(Vec::new()),
        });
        let (device, queue, _) = crate::adv::higurashi::reference_gpu_assets();
        let server = AssetServer::new(
            locate_assets_for_game(Game::Higurashi, Some(&inputs))
                .unwrap()
                .into(),
            AssetLoadContext {
                game: Game::Higurashi,
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: crate::asset::system::cache::AssetCache::new(),
                higurashi_bustup_cache: Default::default(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        );
        assert_eq!(
            server.contains("/append.txt"),
            patch && append,
            "Origin-less mounts must skip append"
        );
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
        let mut runtime = Runtime::new(Arc::new(server), manager, sounds, storage.clone());
        runtime.set_focused(true);
        Self {
            runtime,
            gpu: Graphics::new(device, queue),
            storage,
            _temporary: temporary,
        }
    }
}
fn top(r: &Runtime) -> Option<i32> {
    r.slot()?.title.as_ref()?.reference_top_selection()
}
fn evidence(j: &mut Journey, name: &str) {
    let out = PathBuf::from(std::env::var_os("SHIN_HIGURASHI_PATCHLESS_CAPTURE").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    for size in [(1280, 720), (1920, 1080)] {
        let pixels = j.gpu.pixels(&j.runtime, size, false);
        assert!(pixels.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]));
        std::fs::write(
            out.join(format!("{name}-{}x{}.rgba", size.0, size.1)),
            pixels,
        )
        .unwrap();
    }
}

#[test]
#[ignore = "actual base/update ROM discovery, supplied v1 save, normal startup/TIPS/Config/story;offscreen Metal/exact Loopback Audio; isolated temporary storage"]
fn patchless_runtime_old_save_boot_menus_and_first_authored_message() {
    let old = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../higurashi/OLD_SAVEDATA"
    ))
    .unwrap();
    let decoded = SaveData::decode(&old).unwrap();
    let expected = crate::app::higurashi::runtime_saves::Slots::loaded(&decoded);
    for (patch, append, partial) in [
        (false, false, false),
        (false, true, false),
        (true, false, false),
        (true, true, false),
        (true, true, true),
    ] {
        let mut j = Journey::with_archives(patch, append, partial, &old);
        let viewport = viewport((1280, 720));
        let deadline = Instant::now() + Duration::from_secs(120);
        j.boot_title(viewport, deadline);
        let globals = j.runtime.globals().unwrap().unwrap();
        assert_eq!(
            globals.common.scenario.variant,
            if patch {
                HigurashiVariant::Updated
            } else {
                HigurashiVariant::Original
            }
        );
        assert_eq!(
            globals.common.message.cursor_regular.is_some(),
            patch && !partial
        );
        assert_eq!(
            globals.common.message.cursor_last.is_some(),
            patch && !partial
        );
        assert_eq!(
            serde_json::to_vec(&globals.data.settings).unwrap(),
            serde_json::to_vec(&decoded.settings).unwrap()
        );
        let actual = globals.runtime_saves.unwrap();
        assert_eq!(actual.auto.data, expected.auto.data);
        assert_eq!(actual.auto.valid, expected.auto.valid);
        for (a, b) in actual.manual.iter().zip(&expected.manual) {
            assert_eq!(a.valid, b.valid);
            assert_eq!(a.data, b.data);
        }
        assert_eq!(top(&j.runtime), Some(1)); // The real old autosave remains valid.
        evidence(
            &mut j,
            &format!("title-p{patch}-a{append}-partial{partial}"),
        );
        // Native grid: Load1→Chart3→TIPS6.
        j.tap(KeyCode::ArrowDown, viewport);
        j.tap(KeyCode::ArrowDown, viewport);
        assert_eq!(top(&j.runtime), Some(6));
        j.tap(KeyCode::Enter, viewport);
        j.until(viewport, deadline, "TIPS unavailable", |r| {
            r.slot()
                .and_then(|s| s.tips.as_ref())
                .is_some_and(|t| t.reference_input_ready())
        });
        let tips = j.runtime.slot().unwrap().tips.as_ref().unwrap();
        assert!(!tips.motion.model.alternate);
        assert_eq!(tips.motion.model.rows.len(), 188);
        assert!(tips.motion.model.rows.iter().all(|r| r.artwork_available));
        evidence(&mut j, &format!("tips-p{patch}-a{append}-partial{partial}"));
        j.tap(KeyCode::Escape, viewport);
        j.until(viewport, deadline, "TIPS return", |r| ready_menu(r, "top"));
        j.tap(KeyCode::ArrowUp, viewport);
        j.tap(KeyCode::ArrowUp, viewport);
        j.tap(KeyCode::ArrowLeft, viewport);
        assert_eq!(top(&j.runtime), Some(0));
        j.tap(KeyCode::ArrowDown, viewport);
        assert_eq!(top(&j.runtime), Some(2));
        j.tap(KeyCode::Enter, viewport);
        j.until(viewport, deadline, "Config unavailable", |r| {
            r.slot()
                .and_then(|s| s.title.as_ref())
                .is_some_and(|t| t.reference_config_input_ready())
        });
        evidence(
            &mut j,
            &format!("config-p{patch}-a{append}-partial{partial}"),
        );
        j.tap(KeyCode::Escape, viewport);
        j.until(viewport, deadline, "Config return", |r| {
            ready_menu(r, "top")
        });
        j.no_writes(Some(&old));
        j.tap(KeyCode::ArrowUp, viewport);
        assert_eq!(top(&j.runtime), Some(0));
        j.tap(KeyCode::Enter, viewport);
        j.until(viewport, deadline, "base/updated category unavailable", |r| ready_menu(r, "cat"));
        j.tap(KeyCode::Enter, viewport);
        let mut last_state = None;
        let mut selected_chapter = false;
        let story_deadline = Instant::now() + Duration::from_secs(60);
        loop {
            use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;
            // This supplied old save has completed progress. Its authored
            // Start entry opens SNRSEL instead of replaying the fresh prologue.
            let picker_ready = j.runtime.slot().and_then(|s| s.scenario_select.as_ref()).is_some_and(|p| {
                let owner = p.reference_owner();
                let state = owner.reference_state();
                state.0 && state.2 && state.3.is_none() && owner.reference_model().ready && !owner.reference_model().closing
            });
            if picker_ready {
                assert!(!selected_chapter, "first selected chapter returned to picker before any message");
                evidence(&mut j, &format!("picker-p{patch}-a{append}-partial{partial}"));
                eprintln!("PATCHLESS_PICKER_CONFIRM patch={patch} append={append} partial={partial}");
                j.tap(KeyCode::Enter, viewport);
                selected_chapter = true;
            }
            let host = j.runtime.slot().and_then(|s| s.game.as_ref()).and_then(|g| g.host());
            let state = host.map(|h| (h.runner.completed_commands(), format!("{:?}", h.runner.pending_command())));
            if state != last_state {
                eprintln!("PATCHLESS_STORY_PROGRESS patch={patch} append={append} partial={partial} state={state:?}");
                last_state = state;
            }
            if host.is_some_and(|h| h.runner.frame().message.glyphs().is_some()
                && matches!(h.runner.pending_command(), Some(Cmd::MSGWAIT { .. } | Cmd::MSGSET { auto_wait: 1, .. }))) { break; }
            assert!(Instant::now() < story_deadline, "authored message unavailable; last state={last_state:?}");
            assert!(matches!(j.step(&RawInputState::new(), viewport), Frame::Advanced(_)));
        }
        for _ in 0..8 { j.step(&RawInputState::new(), viewport); }
        evidence(
            &mut j,
            &format!("message-p{patch}-a{append}-partial{partial}"),
        );
        assert!(
            j.runtime
                .slot()
                .unwrap()
                .game
                .as_ref()
                .unwrap()
                .host()
                .unwrap()
                .runner
                .completed_commands()
                > 0
        );
        j.no_writes(Some(&old));
        eprintln!(
            "PATCHLESS_RUNTIME_PASS patch={patch} append={append} partial={partial} old_save_preserved=true tips188=true config=true authored_message=true"
        );
    }
}
