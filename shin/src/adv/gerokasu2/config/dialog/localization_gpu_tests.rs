//! Ger code-defined dialog text through a real `localized.toml` asset server.
//! Real ROM font and offscreen GPU; no audio, story dispatch or saves.
use super::*;
use crate::{
    asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
    cli::Game,
};
use std::path::{Path, PathBuf};

struct Dictionary(PathBuf);
impl Dictionary {
    fn new(contents: Option<&str>) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "shin-ger-localization-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        if let Some(contents) = contents {
            std::fs::write(path.join("localized.toml"), contents).unwrap();
        }
        Self(path)
    }
    fn server(
        &self,
        root: &Path,
        game: Game,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> AssetServer {
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(root.join(match game {
            Game::Gerokasu => "game-data/gerokasu/data.rom",
            _ => "game-data/gerokasu2/data.rom",
        }))
        .unwrap();
        io.with_localized_file(self.0.join("localized.toml"));
        AssetServer::new(
            io.into(),
            AssetLoadContext {
                game,
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: AssetCache::new(),
                higurashi_bustup_cache: Default::default(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        )
    }
}
impl Drop for Dictionary {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn points(text: &Text) -> String {
    text.glyphs.iter().map(|(h, _)| h.codepoint).collect()
}

#[test]
#[ignore = "requires supplied Ger ROM font and offscreen GPU; localization only, no audio"]
fn ger_dialog_lays_out_localized_code_text_and_keeps_missing_keys() {
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for game in [Game::Gerokasu2, Game::Gerokasu] {
        check_dialog(root, game, &device, &queue);
    }
}

fn check_dialog(root: &Path, game: Game, device: &wgpu::Device, queue: &wgpu::Queue) {
    let translated = Dictionary::new(Some(
        "\"ロードします。よろしいですか？\" = \"Load this save?@rAre you sure?\"\n\"はい\" = \"Yes\"\n",
    ));
    let original = Dictionary::new(None);
    let translated = translated.server(root, game, device, queue);
    let original = original.server(root, game, device, queue);
    let prompt = "ロードします。よろしいですか？";
    let load = |server: &AssetServer, prompt: &str, yes: &str| {
        GerDialog::load(server, prompt, &[yes, "いいえ"], 0, true, false).unwrap()
    };
    let localized = load(&translated, prompt, "はい");
    let literal = load(&original, "Load this save?@rAre you sure?", "Yes");
    assert_eq!(points(&localized.prompt), "Load this save?Are you sure?");
    assert_eq!(points(&localized.answers[0]), "Yes");
    // Missing keys keep the source.
    assert_eq!(points(&localized.answers[1]), "いいえ");
    assert_eq!(localized.prompt.size, literal.prompt.size);
    assert_eq!(localized.answers[0].size, literal.answers[0].size);
    // Without localized.toml the Japanese source is laid out.
    let source = load(&original, prompt, "はい");
    assert_eq!(points(&source.prompt), "ロードします。よろしいですか？");
    assert_eq!(points(&source.answers[0]), "はい");
    println!(
        "{game:?} LOCALIZED_DIALOG prompt={:?} source={:?} yes={:?}",
        localized.prompt.size, source.prompt.size, localized.answers[0].size
    );
}
