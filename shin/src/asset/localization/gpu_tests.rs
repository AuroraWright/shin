//! Real font attachment through cached and uncached AssetServer loads.
//! No audio, story dispatch, external saves, or native reference process.
use crate::{
    adv::DialogText,
    asset::{
        font::{FontGlyphCache, GpuFontLazy},
        system::{AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache},
    },
    cli::Game,
};
use shin_core::layout::{
    LayoutParams, MessageTextLayouter, MessageTextLayouterDefaults, commands::Command,
    font::FontMetrics,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

struct DictionaryDirectory(PathBuf);
impl DictionaryDirectory {
    fn new(value: Option<&str>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "shin-localization-font-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        if let Some(value) = value {
            std::fs::write(
                path.join("localized.toml"),
                format!("\"確認\" = \"{value}\"\n"),
            )
            .unwrap();
        }
        Self(path)
    }
    fn server(
        &self,
        root: &Path,
        game: Game,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cache: Arc<FontGlyphCache>,
    ) -> AssetServer {
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(root.join(match game {
            Game::Higurashi => "game-data/higurashi/data.rom",
            Game::Umineko => "game-data/umineko/data.rom",
            Game::Gerokasu2 => "game-data/gerokasu2/data.rom",
            Game::Gerokasu => "game-data/gerokasu/data.rom",
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
                font_cache: cache,
            },
        )
    }
}
impl Drop for DictionaryDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn ui_points(font: Arc<GpuFontLazy>, text: &str) -> String {
    MessageTextLayouter::new(
        font.clone(),
        font,
        LayoutParams::default(),
        MessageTextLayouterDefaults {
            color: 999,
            draw_speed: 50,
            fade: 200,
        },
    )
    .parse(text)
    .0
    .into_iter()
    .filter_map(|command| match command {
        Command::Char(ch) => Some(ch.codepoint),
        _ => None,
    })
    .collect()
}
fn assert_ui_font(server: &AssetServer, font: Arc<GpuFontLazy>, expected: &str) {
    assert_eq!(font.localize("確認").as_ref(), expected);
    assert_eq!(ui_points(font.clone(), "確認"), expected);
    let translated = DialogText::load_with_width(server, &font, "確認", 44., 1440.);
    let literal = DialogText::load_with_width(server, &font, expected, 44., 1440.);
    assert_eq!(translated.size, literal.size);
    assert_eq!(
        translated
            .reference_glyphs()
            .iter()
            .map(|glyph| glyph.codepoint)
            .collect::<String>(),
        expected
    );
    assert_eq!(
        translated
            .reference_glyphs()
            .iter()
            .map(|glyph| glyph.rect)
            .collect::<Vec<_>>(),
        literal
            .reference_glyphs()
            .iter()
            .map(|glyph| glyph.rect)
            .collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "actual Umineko/Higurashi ROM fonts and offscreen GPU; cached/uncached per-server localization isolation, no audio"]
fn localization_real_fonts_attach_per_server_for_cached_and_uncached_ui_loads() {
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter = shin_tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
            adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
        } else {
            wgpu::Features::empty()
        },
        ..Default::default()
    }))
    .unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let shared_cache = Arc::new(FontGlyphCache::default());
    for game in [Game::Umineko, Game::Higurashi] {
        let a_directory = DictionaryDirectory::new(Some("CONFIRM_A"));
        let b_directory = DictionaryDirectory::new(Some("CONFIRM_B"));
        let original_directory = DictionaryDirectory::new(None);
        let a = a_directory.server(root, game, &device, &queue, shared_cache.clone());
        let b = b_directory.server(root, game, &device, &queue, shared_cache.clone());
        let original = original_directory.server(root, game, &device, &queue, shared_cache.clone());
        let path = if game == Game::Higurashi {
            "/newrodin.fnt"
        } else {
            "/system.fnt"
        };
        let a_font = a.load_sync::<GpuFontLazy>(path).unwrap();
        let b_font = b.load_sync::<GpuFontLazy>(path).unwrap();
        assert!(
            Arc::ptr_eq(&a_font, &a.load_sync::<GpuFontLazy>(path).unwrap()),
            "repeat loads must actually exercise the asset cache"
        );
        assert!(!Arc::ptr_eq(&a_font, &b_font));
        assert_ui_font(&a, a_font.clone(), "CONFIRM_A");
        assert_ui_font(&b, b_font.clone(), "CONFIRM_B");
        for (server, expected) in [(&a, "CONFIRM_A"), (&b, "CONFIRM_B")] {
            let uncached = Arc::new(
                shin_tasks::block_on(server.load_uncached_with_args::<GpuFontLazy, _>(path, ()))
                    .unwrap(),
            );
            assert_ui_font(server, uncached, expected);
        }
        // Both fonts share the glyph cache, while dictionaries remain tied to
        // their separate font owners after another server has loaded/rendered.
        assert_ui_font(&a, a_font, "CONFIRM_A");
        assert_ui_font(&b, b_font, "CONFIRM_B");
        let original_font = original.load_sync::<GpuFontLazy>(path).unwrap();
        assert_eq!(ui_points(original_font.clone(), "確認"), "確認");
        assert_eq!(original_font.localize("確認").as_ref(), "確認");
        if game == Game::Higurashi {
            use crate::adv::higurashi::question::view::View;
            for (server, expected) in [(&a, "CONFIRM_A"), (&b, "CONFIRM_B")] {
                let (translated, _view) =
                    shin_tasks::block_on(View::load(server, "確認", &["確認"], 0, true)).unwrap();
                let (literal, _literal_view) =
                    shin_tasks::block_on(View::load(server, expected, &[expected], 0, true))
                        .unwrap();
                assert_eq!(translated.inner(), literal.inner());
                assert_eq!(translated.outer(), literal.outer());
                assert_eq!(
                    translated.buttons()[0].text_size,
                    literal.buttons()[0].text_size
                );
            }
        }
    }
}
