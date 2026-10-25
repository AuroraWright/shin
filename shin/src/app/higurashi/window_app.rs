//! Game-specific ShinApp adapter and read-only normal-launch preflight.
use super::{
    load_game::Paths,
    runtime::{Frame, InputViewport, Runtime},
};
use crate::{
    asset::system::{
        AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache, locate_assets_for_game,
    },
    audio::SystemSoundPlayer,
    cli::{Cli, Game},
    render::PreRenderContext,
};
use anyhow::{Context, Result, ensure};
use enum_map::EnumMap;
use parking_lot::Mutex;
use shin_input::{ActionState, DummyAction, RawInputState};
use shin_render::render_pass::RenderPass;
use shin_window::{AppContext, RenderContext, ShinApp};
use std::{path::PathBuf, sync::Arc, time::Duration};

fn validate_cli(cli: &Cli) -> Result<()> {
    ensure!(
        cli.game == Game::Higurashi && !cli.inspect_assets(),
        "Higurashi application requires a normal Higurashi launch"
    );
    Ok(())
}
fn progress_path(cli: &Cli) -> Result<PathBuf> {
    Ok(cli.save_file())
}

/// Own the mounted assets across the CLI/window boundary. Progress is only a
/// path here; the asynchronous startup owner reads it and handles recovery.
#[derive(Debug)]
pub(crate) struct PreparedLaunch {
    progress: PathBuf,
    assets: LayeredAssetIo,
}

pub(crate) fn prepare_launch(cli: &mut Cli) -> Result<()> {
    // Validate before even trying an asset path. Inspection and unsupported
    // direct-entry options must not create a window or start any worker.
    validate_cli(cli)?;
    if cli.higurashi_launch.is_none() {
        let progress = progress_path(cli)?;
        ensure!(
            !progress.as_os_str().is_empty(),
            "Higurashi progress path is empty"
        );
        let assets = locate_assets_for_game(Game::Higurashi, &cli.assets_dir())
            .context("Locating Higurashi assets for normal launch")?;
        cli.higurashi_launch = Some(PreparedLaunch { progress, assets });
    }
    Ok(())
}

fn take_launch(cli: &mut Cli) -> Result<PreparedLaunch> {
    // Main supplies a prepared mount. Existing direct ShinApp/probe callers
    // still pass an ordinary parsed Cli, so resolve it once on that path.
    prepare_launch(cli)?;
    cli.higurashi_launch
        .take()
        .context("Higurashi launch preflight did not retain its resources")
}

pub(crate) struct HigurashiApp {
    runtime: Runtime,
    manager: Arc<shin_audio::AudioManager>,
    platform_suspended: bool,
    last_fault: Option<String>,
}
impl HigurashiApp {
    fn trace_platform_state(&self, event: &'static str, focused: Option<bool>) {
        self.runtime.trace_platform_state(
            event,
            focused,
            self.platform_suspended,
            self.manager.suspension().is_suspended(),
        );
    }
    #[cfg(feature = "higurashi-window-probe")]
    pub(super) fn probe_snapshot(&self) -> Result<super::window_probe::Snapshot> {
        ensure!(
            self.last_fault.is_none(),
            "Higurashi callback fault: {:?}",
            self.last_fault
        );
        self.runtime.probe_snapshot()
    }
    #[cfg(feature = "higurashi-window-probe")]
    pub(super) fn probe_suspended(&self) -> bool {
        self.manager.suspension().is_suspended()
    }
    fn fault(&mut self, error: anyhow::Error) {
        let error = format!("{error:#}");
        if self.last_fault.as_ref() != Some(&error) {
            tracing::error!(%error, "Higurashi application callback failed; retaining source and composed frame");
        }
        self.last_fault = Some(error);
    }
    fn sync_audio_suspension(&self) {
        self.manager
            .set_suspended(self.platform_suspended || self.runtime.freezes_scene());
        self.runtime.sync_movie_suspension();
    }
}
impl ShinApp for HigurashiApp {
    const WINDOW_TITLE: &'static str = "ひぐらしのなく頃に奉";
    const FULLSCREEN_SHORTCUT: Option<winit::keyboard::KeyCode> =
        Some(winit::keyboard::KeyCode::KeyF);
    type Parameters = Cli;
    type EventType = ();
    // raw_input owns the one canonical projection/native repeat clock.
    type ActionType = DummyAction;
    fn init(context: AppContext<Self>, mut cli: Cli) -> Result<Self> {
        let PreparedLaunch { progress, assets } = take_launch(&mut cli)?;
        let manager = Arc::new(shin_audio::AudioManager::new());
        let server = Arc::new(AssetServer::new(
            assets.into(),
            AssetLoadContext {
                game: Game::Higurashi,
                wgpu_device: context.wgpu.device.clone(),
                wgpu_queue: context.wgpu.queue.clone(),
                bustup_cache: AssetCache::new(),
                higurashi_bustup_cache: Default::default(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));
        let sounds = Arc::new(Mutex::new(SystemSoundPlayer::new(manager.clone())));
        let storage = Arc::new(Paths {
            current: Some(progress),
            trial: None,
        });
        Ok(Self {
            runtime: Runtime::new(server, manager.clone(), sounds, storage),
            manager,
            platform_suspended: false,
            last_fault: None,
        })
    }
    fn custom_event(&mut self, _: AppContext<Self>, _: ()) {}
    fn raw_input(&mut self, raw: &RawInputState) {
        self.runtime.raw_input(raw);
    }
    fn focus_lost(&mut self) {
        self.trace_platform_state("focus_lost_before", Some(false));
        if let Err(error) = self.runtime.focus_lost() {
            self.fault(error);
        }
        self.trace_platform_state("focus_lost_after", Some(false));
    }
    fn set_suspended(&mut self, suspended: bool) {
        self.trace_platform_state(
            if suspended {
                "suspend_enter_before"
            } else {
                "suspend_leave_before"
            },
            None,
        );
        self.platform_suspended = suspended;
        if suspended {
            if let Err(error) = self.runtime.cancel_input() {
                self.fault(error);
            }
        }
        self.sync_audio_suspension();
        self.trace_platform_state(
            if suspended {
                "suspend_enter_after"
            } else {
                "suspend_leave_after"
            },
            None,
        );
    }
    fn close_requested(&mut self) -> bool {
        self.trace_platform_state("close_before", None);
        if let Err(error) = self.runtime.request_close() {
            self.fault(error);
        }
        self.trace_platform_state("close_after", None);
        false
    }
    fn update(
        &mut self,
        context: AppContext<Self>,
        _: EnumMap<DummyAction, ActionState>,
        elapsed: Duration,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let focused = context.winit.window.has_focus();
        self.runtime.set_focused(focused);
        let viewport = Self::map_canvas_size(context.winit.window.inner_size());
        let viewport = InputViewport::new(
            glam::Vec2::new(
                viewport.canvas_offset.x as f32,
                viewport.canvas_offset.y as f32,
            ),
            glam::Vec2::new(
                viewport.canvas_size.width.max(1) as f32,
                viewport.canvas_size.height.max(1) as f32,
            ),
        );
        let mut pre_render = PreRenderContext {
            device: &context.wgpu.device,
            queue: &context.wgpu.queue,
            breakup_texture: None,
            resize_source: &context.winit.resize_source,
            sampler_store: &context.render.sampler_store,
            depth_stencil: context.render.canvas_depth_stencil_buffer.get_target_view(),
            pipeline_storage: &mut context.render.pipelines,
            dynamic_buffer: &mut context.render.dynamic_buffer,
            encoder,
        };
        let output = viewport.and_then(|viewport| {
            shin_tasks::block_on(self.runtime.update(
                &mut pre_render,
                elapsed,
                viewport,
                focused,
                crate::adv::higurashi::runner::Policy {
                    animations_enabled: true,
                    paused: false,
                    fast_skip: false,
                    suppress_transient_audio: false,
                    read_flags: 0,
                    text_speed: 50,
                    outline_mode: 0,
                },
                crate::adv::higurashi::audio::AudioFramePolicy {
                    focus_mode_active: true,
                    voice_focus_enabled: true,
                    effects_update_enabled: true,
                    music_rate: 1.,
                },
            ))
        });
        match output {
            Ok(Frame::Exit) => context.event_loop.exit(),
            Ok(_) => self.last_fault = None,
            Err(error) => self.fault(error),
        }
        self.sync_audio_suspension();
        self.trace_platform_state("progress", Some(focused));
    }
    fn render(&mut self, _: RenderContext, pass: &mut RenderPass) {
        self.runtime.render(pass);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn each_game_window_is_titled_with_the_game_name() {
        use shin_window::ShinApp;
        assert_eq!(HigurashiApp::WINDOW_TITLE, "ひぐらしのなく頃に奉");
        assert_eq!(crate::app::App::WINDOW_TITLE, "うみねこのなく頃に咲 〜猫箱と夢想の交響曲〜");
        assert_eq!(
            crate::app::gerokasu2::Gerokasu2App::WINDOW_TITLE,
            "幻想牢獄のカレイドスコープ 2"
        );
    }
    use std::{
        ffi::OsString,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct LaunchFiles(PathBuf);
    impl LaunchFiles {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "higurashi-launch-preflight-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn cli(&self, extra: &[&str]) -> Cli {
            let mut args = vec![
                OsString::from("shin"),
                "--game".into(),
                "higurashi".into(),
                "--assets-dir".into(),
                self.0.join("assets").into_os_string(),
                "--save-file".into(),
                self.0.join("new-profile/progress.dat").into_os_string(),
            ];
            args.extend(extra.iter().map(|arg| OsString::from(*arg)));
            Cli::try_parse_from(args).unwrap()
        }
    }
    impl Drop for LaunchFiles {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn higurashi_window_cli_keeps_inspection_separate_and_defaults_progress() {
        assert!(validate_cli(&Cli::try_parse_from(["shin"]).unwrap()).is_err());
        assert!(
            validate_cli(
                &Cli::try_parse_from(["shin", "--game", "higurashi", "--inspect-assets"]).unwrap()
            )
            .is_err()
        );
        let cli = Cli::try_parse_from([
            "shin",
            "--game",
            "higurashi",
            "--save-file",
            "higu-progress.dat",
        ])
        .unwrap();
        validate_cli(&cli).unwrap();
        assert_eq!(
            progress_path(&cli).unwrap(),
            PathBuf::from("higu-progress.dat")
        );
        let cli = Cli::try_parse_from(["shin", "--game", "higurashi", "-a", "higu"]).unwrap();
        validate_cli(&cli).unwrap();
        assert!(progress_path(&cli).unwrap().ends_with("shin/higurashi/SAVEDATA"));
    }

    #[test]
    fn higurashi_launch_reports_invalid_assets_before_window_initialization() {
        let files = LaunchFiles::new();
        let mut cli = files.cli(&[]);
        let error = format!("{:#}", prepare_launch(&mut cli).unwrap_err());
        assert!(error.contains("Locating Higurashi assets for normal launch"));
        assert!(error.contains("asset directory does not exist"), "{error}");
        assert!(error.contains(&files.0.join("assets").display().to_string()));
        assert!(cli.higurashi_launch.is_none());
        std::fs::create_dir(files.0.join("assets")).unwrap();
        let error = format!("{:#}", prepare_launch(&mut cli).unwrap_err());
        assert!(error.contains("No Higurashi asset layers"), "{error}");
        assert!(cli.higurashi_launch.is_none());
        assert!(!files.0.join("new-profile").exists());
    }

    #[test]
    fn higurashi_launch_retains_prepared_assets_without_touching_progress() {
        let files = LaunchFiles::new();
        let assets = files.0.join("assets");
        std::fs::create_dir_all(assets.join("data")).unwrap();
        std::fs::write(assets.join("data/preflight.txt"), b"mounted assets").unwrap();
        let mut cli = files.cli(&[]);
        assert!(cli.higurashi_launch.is_none());
        prepare_launch(&mut cli).unwrap();
        let prepared = cli.higurashi_launch.as_ref().unwrap();
        assert_eq!(prepared.progress, files.0.join("new-profile/progress.dat"));
        assert_eq!(
            prepared
                .assets
                .read_file("/preflight.txt")
                .unwrap()
                .read_all_sync()
                .unwrap(),
            b"mounted assets",
        );
        assert!(!files.0.join("new-profile").exists());

        // Initialization consumes the prepared mount instead of looking up
        // the path again. No window, GPU, audio or task pools are needed here.
        std::fs::remove_dir_all(&assets).unwrap();
        let prepared = take_launch(&mut cli).unwrap();
        assert!(!prepared.assets.is_empty());
        assert_eq!(prepared.progress, files.0.join("new-profile/progress.dat"));
        assert!(cli.higurashi_launch.is_none());
        assert!(!files.0.join("new-profile").exists());

        // A corrupt existing file belongs to asynchronous LoadGame recovery,
        // and must also survive this CPU-only asset/path preflight untouched.
        std::fs::create_dir_all(&assets.join("data")).unwrap();
        let saved = files.0.join("existing-progress.dat");
        std::fs::write(&saved, b"not a decoded save").unwrap();
        cli.save_file = Some(saved.clone());
        prepare_launch(&mut cli).unwrap();
        assert_eq!(std::fs::read(saved).unwrap(), b"not a decoded save");
    }
}
