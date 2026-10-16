//! ConfigMenu async artwork, child ownership and explicit save-task handoff.
use crate::adv::{
    characters::view::CharacterTextures,
    config::{ConfigInput, ConfigModel, ConfigRequest},
    config::dialog::{ConfigDialog, ConfigDialogKind},
    config::view,
    config::widgets::ConfigEdit,
    input_area::{InputArea, PointerFrame},
};
use crate::{app::AppAction, asset::system::AssetServer, render::PreRenderContext};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::{format::save::Settings, time::Ticks};
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

type Atlases = (Arc<CharacterTextures>, Arc<CharacterTextures>);
pub struct ConfigScreen {
    loading: Option<shin_tasks::AsyncTask<anyhow::Result<Atlases>>>,
    atlases: Option<Atlases>,
    model: ConfigModel,
    dialog: Option<ConfigDialog>,
    area: InputArea,
    extent: (u16, u16),
    pointer_held: bool,
    save_requested: bool,
    sounds: Vec<&'static str>,
    server: Arc<AssetServer>,
}
impl ConfigScreen {
    pub(crate) fn suspend_pointer(&mut self, held: bool) {
        self.pointer_held = held;
        self.set_pointer_extent(self.extent);
    }
    pub fn new(server: Arc<AssetServer>, settings: Settings) -> Self {
        let assets = server.clone();
        Self {
            loading: Some(shin_tasks::async_io::spawn(async move {
                futures::try_join!(assets.load("/config.txa"), assets.load("/systex.txa"))
            })),
            atlases: None,
            model: ConfigModel::new(settings),
            dialog: None,
            area: InputArea::canvas(),
            extent: (1920, 1080),
            pointer_held: false,
            save_requested: false,
            sounds: Vec::new(),
            server,
        }
    }
    pub fn model(&self) -> Option<&ConfigModel> {
        self.atlases.as_ref().map(|_| &self.model)
    }
    pub fn settings(&self) -> &Settings {
        &self.model.settings
    }
    pub fn set_pointer_extent(&mut self, extent: (u16, u16)) {
        self.area.disable(&mut self.model);
        self.area = InputArea::new(extent.0, extent.1);
        self.extent = extent;
        if let Some(dialog) = &mut self.dialog {
            dialog.set_pointer_extent(extent, self.pointer_held);
        }
    }
    pub fn pointer_state(&mut self, position: Vec2, held: bool, elapsed: f64) {
        self.pointer_held = held;
        if let Some(dialog) = &mut self.dialog {
            dialog.pointer_state(position, held, elapsed);
        }
    }
    pub fn input_frame(
        &mut self,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        axis: f32,
    ) {
        if self.atlases.is_none() {
            return;
        }
        if let Some(dialog) = &mut self.dialog {
            self.area.disable(&mut self.model);
            dialog.keyboard(state);
            return;
        }
        self.model.input_frame(
            &mut self.area,
            frame,
            ConfigInput {
                previous_tab: state[AppAction::PageUp].is_clicked,
                next_tab: state[AppAction::PageDown].is_clicked,
                up: state[AppAction::Up].is_clicked_or_repeated,
                down: state[AppAction::AnyDown].is_clicked_or_repeated,
                reset: state[AppAction::ConfigReset].is_clicked,
                close: state[AppAction::Cancel].is_clicked,
                left: state[AppAction::Left].is_clicked_or_repeated,
                right: state[AppAction::Right].is_clicked_or_repeated,
                axis,
            },
        );
    }
    pub fn update(&mut self, delta: Ticks) -> anyhow::Result<Option<i32>> {
        if let Some(task) = &mut self.loading {
            if let Some(result) = task.poll_naive() {
                self.loading = None;
                let (config, system) = result?;
                config.require_names(
                    "/config.txa",
                    &[
                        "bg",
                        "obi",
                        "window",
                        "caption",
                        "close",
                        "default",
                        "tab_on",
                        "tab_off",
                        "left",
                        "right",
                        "button_on",
                        "button_off",
                        "slider",
                        "focus",
                        "line",
                        "help_config",
                        "key",
                    ],
                )?;
                self.atlases = Some((config, system));
            }
            // Native asset-completion frame still has opening progress0.
            return Ok(None);
        }
        if let Some(dialog) = &mut self.dialog {
            let kind = dialog.kind;
            let mut finished = false;
            for response in dialog.update(delta) {
                match (kind, response.final_flag) {
                    (ConfigDialogKind::Reset, false) => self.model.reset_choice(response.button, 0),
                    (ConfigDialogKind::Reset, true) => self.model.reset_finished(),
                    (ConfigDialogKind::AutoSleep, true) => self.model.notice_finished(),
                    _ => {}
                }
                finished |= response.final_flag;
            }
            if finished {
                self.sounds.extend(dialog.take_sounds());
                self.dialog = None;
            }
        }
        self.model.update(delta.as_f32());
        for request in self.model.take_requests() {
            match request {
                ConfigRequest::Save => self.save_requested = true,
                ConfigRequest::ResetConfirmation | ConfigRequest::AutoSleepNotice => {
                    let kind = if request == ConfigRequest::ResetConfirmation {
                        ConfigDialogKind::Reset
                    } else {
                        ConfigDialogKind::AutoSleep
                    };
                    let mut dialog = ConfigDialog::load(&self.server, kind, self.pointer_held)?;
                    dialog.set_pointer_extent(self.extent, self.pointer_held);
                    self.area.disable(&mut self.model);
                    self.dialog = Some(dialog);
                }
            }
        }
        // A save completion can start closing before this update consumes a
        // large delta. Keep the final callback if both responses are queued.
        Ok(self.model.take_response())
    }
    /// The ADV persistence adapter calls this after its SaveGame task completes.
    pub fn save_finished(&mut self) {
        self.model.save_finished();
    }
    pub(crate) fn save_failed(&mut self) {
        self.model.save_failed();
        self.suspend_pointer(self.pointer_held);
    }
    pub fn take_save_request(&mut self) -> bool {
        std::mem::take(&mut self.save_requested)
    }
    pub fn take_edits(&mut self) -> Vec<ConfigEdit> {
        self.model.take_edits()
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        let mut sounds = std::mem::take(&mut self.sounds);
        sounds.extend(self.model.take_sounds());
        if let Some(dialog) = &mut self.dialog {
            sounds.extend(dialog.take_sounds());
        }
        sounds
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) {
        if let Some(dialog) = &mut self.dialog {
            dialog.prepare(context);
        }
    }
    pub fn render(&self, pass: &mut RenderPass) {
        if let Some((atlas, _)) = &self.atlases {
            view::render(
                &self.model,
                atlas,
                pass,
                crate::render::top_left_projection_matrix(),
            );
            if let Some(dialog) = &self.dialog {
                dialog.render(pass);
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_dialog_is_open(&self) -> bool {
        self.dialog
            .as_ref()
            .is_some_and(ConfigDialog::reference_is_open)
    }
    #[cfg(test)]
    pub(crate) fn wait_reference_assets(&mut self) -> anyhow::Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while self.atlases.is_none() {
            assert_eq!(self.update(Ticks::from_f32(1000.0))?, None);
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "Config asset load timed out"
            );
            std::thread::yield_now();
        }
        assert_eq!(self.model.progress(), 0.0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::adv::config::ConfigPhase;
    use super::*;
    fn send(screen: &mut ConfigScreen, number: &mut u32, action: AppAction) {
        *number += 1;
        let mut state = EnumMap::default();
        state[action] = ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            ..Default::default()
        };
        screen.input_frame(
            &PointerFrame {
                number: *number,
                time_seconds: *number as f64 / 60.0,
                events: &[],
            },
            &state,
            0.0,
        );
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_config_screen_async_input_dialogs_settings_save_and_close() {
        use crate::asset::system::{
            AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache,
        };
        use shin_render::{
            dynamic_buffer::DynamicBuffer,
            pipelines::PipelineStorage,
            resize::{SurfaceResizeSource, ViewportParams},
            shaders::types::{
                buffer::BytesAddress,
                texture::{
                    DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind,
                },
            },
        };
        crate::init_test_task_pools();
        let instance = wgpu::Instance::default();
        let adapter =
            shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
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
        io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        let server = std::sync::Arc::new(AssetServer::new(
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
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            1920, 1080,
        )));
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Config rows reference depth"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: shin_render::DEPTH_STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Config rows reference target"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let samplers = TextureSamplerStore::new(&device);
        let mut pipelines = PipelineStorage::new(device.clone(), format);
        let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut context = PreRenderContext {
            breakup_texture: None,
            device: &device,
            queue: &queue,
            resize_source: &resize,
            sampler_store: &samplers,
            depth_stencil: DepthStencilTarget { view: &depth_view },
            pipeline_storage: &mut pipelines,
            dynamic_buffer: &mut dynamic,
            encoder: &mut encoder,
        };

        let mut frame = 0;
        let mut settings = Settings::default();
        settings.v18_showsongtitle = false;
        let mut screen = ConfigScreen::new(server.clone(), settings.clone());
        screen.set_pointer_extent((960, 540));
        send(&mut screen, &mut frame, AppAction::Right);
        assert!(screen.settings().serialized_eq(&settings));
        screen.wait_reference_assets().unwrap();
        screen.update(Ticks::from_f32(10.0)).unwrap();
        send(&mut screen, &mut frame, AppAction::Right);
        assert_eq!(screen.settings().v3_sysvol, settings.v3_sysvol + 5);
        send(&mut screen, &mut frame, AppAction::ConfigReset);
        screen.update(Ticks::from_f32(7.0)).unwrap();
        screen.update(Ticks::ZERO).unwrap();
        assert_eq!(screen.model().unwrap().phase(), ConfigPhase::ResetDialog);
        screen.update(Ticks::from_f32(12.0)).unwrap();
        screen.update(Ticks::ZERO).unwrap();
        screen.prepare(&mut context);
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Config screen reset child",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(128),
                Some(1.0),
            );
            screen.render(&mut pass);
        }
        std::fs::write(
            "/private/tmp/shin-config-screen-reset.rgba",
            crate::layer::user::readback_test_texture(&mut context, &target),
        )
        .unwrap();
        send(&mut screen, &mut frame, AppAction::Left);
        send(&mut screen, &mut frame, AppAction::Enter);
        screen.update(Ticks::from_f32(7.0)).unwrap();
        screen.update(Ticks::ZERO).unwrap();
        assert!(screen.settings().serialized_eq(&Settings::default()));
        assert_eq!(screen.model().unwrap().phase(), ConfigPhase::ResetDialog);
        screen.update(Ticks::from_f32(12.0)).unwrap();
        screen.update(Ticks::from_f32(5.0)).unwrap();
        assert_eq!(screen.model().unwrap().phase(), ConfigPhase::Active);
        send(&mut screen, &mut frame, AppAction::PageDown);
        screen.update(Ticks::from_f32(4.0)).unwrap();
        send(&mut screen, &mut frame, AppAction::PageDown);
        screen.update(Ticks::from_f32(4.0)).unwrap();
        for _ in 0..4 {
            send(&mut screen, &mut frame, AppAction::AnyDown);
            screen.update(Ticks::from_f32(4.0)).unwrap();
        }
        assert_eq!(screen.model().unwrap().selected_tab(), 2);
        assert_eq!(screen.model().unwrap().selected_row(), 4);
        if screen.settings().v18_showsongtitle {
            send(&mut screen, &mut frame, AppAction::Left);
        }
        send(&mut screen, &mut frame, AppAction::Right);
        screen.update(Ticks::ZERO).unwrap();
        assert_eq!(screen.model().unwrap().phase(), ConfigPhase::NoticeDialog);
        screen.update(Ticks::from_f32(12.0)).unwrap();
        screen.update(Ticks::ZERO).unwrap();
        send(&mut screen, &mut frame, AppAction::Cancel);
        assert_eq!(screen.model().unwrap().phase(), ConfigPhase::NoticeDialog);
        screen.prepare(&mut context);
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(context.depth_stencil),
                "Config screen notice child",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(128),
                Some(1.0),
            );
            screen.render(&mut pass);
        }
        std::fs::write(
            "/private/tmp/shin-config-screen-notice.rgba",
            crate::layer::user::readback_test_texture(&mut context, &target),
        )
        .unwrap();
        send(&mut screen, &mut frame, AppAction::Enter);
        for _ in 0..40 {
            screen.update(Ticks::from_f32(1.0)).unwrap();
        }
        assert!(
            screen
                .take_sounds()
                .iter()
                .filter(|&&cue| cue == "decide")
                .count()
                >= 2
        );
        assert_eq!(screen.model().unwrap().phase(), ConfigPhase::Active);
        send(&mut screen, &mut frame, AppAction::Cancel);
        screen.update(Ticks::from_f32(7.0)).unwrap();
        screen.update(Ticks::ZERO).unwrap();
        assert_eq!(screen.model().unwrap().phase(), ConfigPhase::WaitingSave);
        assert!(screen.take_save_request());
        assert!(!screen.take_save_request());
        assert_eq!(screen.update(Ticks::from_f32(100.0)).unwrap(), None);
        assert_eq!(screen.model().unwrap().progress(), 1.0);
        let path = std::env::temp_dir().join(format!(
            "shin-config-screen-save-{}.dat",
            std::process::id()
        ));
        let mut vm = crate::adv::VmState::new();
        vm.settings = screen.settings().clone();
        let mut store = crate::adv::persistence::SaveStore::open(path.clone(), None).unwrap();
        store.flush_config(&vm).unwrap();
        let reloaded =
            shin_core::format::save::SaveData::decode(&std::fs::read(&path).unwrap()).unwrap();
        assert!(reloaded.settings.serialized_eq(screen.settings()));
        std::fs::remove_file(path).unwrap();
        screen.save_finished();
        assert_eq!(screen.update(Ticks::from_f32(10.0)).unwrap(), Some(0));
        assert_eq!(screen.update(Ticks::ZERO).unwrap(), Some(1));
        assert_eq!(screen.update(Ticks::from_f32(100.0)).unwrap(), None);
        let mut unchanged = ConfigScreen::new(server, settings);
        unchanged.wait_reference_assets().unwrap();
        unchanged.update(Ticks::from_f32(10.0)).unwrap();
        send(&mut unchanged, &mut frame, AppAction::Cancel);
        unchanged.update(Ticks::from_f32(7.0)).unwrap();
        assert_eq!(unchanged.update(Ticks::ZERO).unwrap(), Some(0));
        assert!(!unchanged.take_save_request());
        assert_eq!(unchanged.update(Ticks::from_f32(10.0)).unwrap(), Some(1));
    }
}
