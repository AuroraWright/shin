//! Config reset60250 and auto-sleep information5fe70 generic dialog children.
use crate::adv::{
    backlog::rollback::{Confirmation, Outcome},
    config::widgets::AUTO_SLEEP_NOTICE,
    dialog::input::DialogInput,
    dialog::view::DialogView,
};
use crate::{app::AppAction, asset::system::AssetServer, render::PreRenderContext};
use enum_map::EnumMap;
use glam::Vec2;
use shin_core::time::Ticks;
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;

pub const RESET_PROMPT: &str = "設定を初期値に戻します。よろしいですか？";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigDialogKind {
    Reset,
    AutoSleep,
}
impl ConfigDialogKind {
    pub fn prompt(self) -> &'static str {
        match self {
            Self::Reset => RESET_PROMPT,
            Self::AutoSleep => AUTO_SLEEP_NOTICE,
        }
    }
    pub fn labels(self) -> &'static [&'static str] {
        match self {
            Self::Reset => &["はい", "いいえ"],
            Self::AutoSleep => &["確認"],
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigDialogResponse {
    pub final_flag: bool,
    pub button: i32,
}
pub struct ConfigDialog {
    pub kind: ConfigDialogKind,
    confirmation: Confirmation,
    input: DialogInput,
    view: DialogView,
}
impl ConfigDialog {
    #[cfg(test)]
    pub(crate) fn reference_is_open(&self) -> bool {
        self.confirmation.is_open()
    }
    pub fn load(server: &AssetServer, kind: ConfigDialogKind, held: bool) -> anyhow::Result<Self> {
        let view = DialogView::load_text(server, kind.prompt(), kind.labels())?;
        let confirmation = match kind {
            ConfigDialogKind::Reset => Confirmation::new(2, 1, true),
            ConfigDialogKind::AutoSleep => Confirmation::new(1, 0, false),
        };
        let mut input = DialogInput::default();
        input.ignore_pointer(held);
        Ok(Self {
            kind,
            confirmation,
            input,
            view,
        })
    }
    pub fn set_pointer_extent(&mut self, extent: (u16, u16), held: bool) {
        self.input.ignore_pointer(held);
        if extent.0 > 0 && extent.1 > 0 {
            self.input.set_pointer_scale(Vec2::new(
                1920.0 / extent.0 as f32,
                1080.0 / extent.1 as f32,
            ));
        }
    }
    pub fn pointer_state(&mut self, position: Vec2, held: bool, elapsed: f64) {
        self.input.pointer_timed(
            &mut self.confirmation,
            &mut self.view.geometry,
            position,
            held,
            elapsed,
        );
    }
    pub fn keyboard(&mut self, state: &EnumMap<AppAction, ActionState>) {
        self.input
            .keyboard(&mut self.confirmation, self.view.geometry.vertical, state);
    }
    pub fn update(&mut self, delta: Ticks) -> Vec<ConfigDialogResponse> {
        self.input.update_motion(&mut self.view.geometry, delta);
        self.confirmation.update(delta);
        let mut result = Vec::new();
        for (final_flag, outcome) in [
            (false, self.confirmation.take_intermediate_outcome()),
            (true, self.confirmation.take_outcome()),
        ] {
            if let Some(outcome) = outcome {
                result.push(ConfigDialogResponse {
                    final_flag,
                    button: match outcome {
                        Outcome::Yes => 0,
                        Outcome::No => 1,
                        Outcome::Cancel => -1,
                    },
                });
            }
        }
        result
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        self.input.take_sounds()
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) {
        self.view.prepare(context, &self.confirmation);
    }
    pub fn render(&self, pass: &mut RenderPass) {
        self.view.render(pass, &self.confirmation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adv::{
        config::{ConfigInput, ConfigModel, ConfigPhase},
        dialog::view::DialogGeometry,
    };
    fn open(dialog: &mut Confirmation) {
        dialog.update(Ticks::from_f32(12.0));
        dialog.update(Ticks::ZERO);
        assert!(dialog.is_open());
    }
    fn key(action: AppAction) -> EnumMap<AppAction, ActionState> {
        let mut state = EnumMap::default();
        state[action] = ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            ..Default::default()
        };
        state
    }
    #[test]
    fn information_single_button_navigation_and_disabled_cancel_do_not_create_a_second_choice() {
        let mut dialog = Confirmation::new(1, 0, false);
        let mut input = DialogInput::default();
        open(&mut dialog);
        for action in [
            AppAction::Left,
            AppAction::Right,
            AppAction::Up,
            AppAction::AnyDown,
            AppAction::Cancel,
        ] {
            input.keyboard(&mut dialog, false, &key(action));
            assert_eq!(dialog.selected(), Some(0));
            assert!(dialog.is_open());
        }
        assert!(input.take_sounds().is_empty());
        assert!(!dialog.select(1));
        assert!(!dialog.hold(1));
        let geometry = DialogGeometry::single(Vec2::new(500.0, 124.0), Vec2::new(98.0, 58.0));
        assert_eq!(geometry.button_count, 1);
        assert_eq!(geometry.inner, Vec2::new(600.0, 214.0));
        assert_eq!(geometry.buttons[0], glam::vec4(210.0, 132.0, 180.0, 82.0));
        assert_eq!(geometry.buttons[1], glam::Vec4::ZERO);
        input.keyboard(&mut dialog, false, &key(AppAction::Enter));
        assert_eq!(input.take_sounds(), ["decide"]);
        dialog.update(Ticks::from_f32(7.0));
        assert_eq!(dialog.take_intermediate_outcome(), None);
        dialog.update(Ticks::ZERO);
        assert_eq!(dialog.take_intermediate_outcome(), Some(Outcome::Yes));
        assert_eq!(dialog.take_outcome(), None);
        dialog.update(Ticks::from_f32(12.0));
        dialog.update(Ticks::from_f32(4.0));
        assert_eq!(dialog.take_outcome(), None);
        dialog.update(Ticks::from_f32(1.0));
        assert_eq!(dialog.take_outcome(), Some(Outcome::Yes));
        assert_eq!(dialog.take_outcome(), None);
        assert_eq!(dialog.take_intermediate_outcome(), None);
    }
    #[test]
    fn reset_defaults_apply_at_intermediate_callback_and_input_waits_for_final() {
        let mut model = ConfigModel::new(shin_core::format::save::Settings::default());
        model.update(10.0);
        model.controller_input(ConfigInput {
            right: true,
            ..Default::default()
        });
        model.controller_input(ConfigInput {
            reset: true,
            ..Default::default()
        });
        model.update(7.0);
        model.update(0.0);
        assert_eq!(model.phase(), ConfigPhase::ResetDialog);
        let mut dialog = Confirmation::new(2, 1, true);
        open(&mut dialog);
        assert_eq!(dialog.selected(), Some(1));
        dialog.select(0);
        dialog.confirm();
        dialog.update(Ticks::from_f32(7.0));
        dialog.update(Ticks::ZERO);
        assert_eq!(dialog.take_intermediate_outcome(), Some(Outcome::Yes));
        model.reset_choice(0, 0);
        assert!(model.settings.serialized_eq(&Default::default()));
        assert_eq!(model.phase(), ConfigPhase::ResetDialog);
        model.controller_input(ConfigInput {
            right: true,
            ..Default::default()
        });
        assert!(model.settings.serialized_eq(&Default::default()));
        dialog.update(Ticks::from_f32(12.0));
        dialog.update(Ticks::from_f32(5.0));
        assert_eq!(dialog.take_outcome(), Some(Outcome::Yes));
        model.reset_finished();
        assert_eq!(model.phase(), ConfigPhase::Active);
    }

    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_config_dialog_prompts_choices_callbacks_and_cancel_flags() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, expected) in [
            (0x60268, 0x9116a260),
            (0x6026c, 0x94016de1),
            (0x60270, 0x36000060),
            (0x60294, 0x910182a0),
            (0x602a8, 0x320003e1),
            (0x602ac, 0x97ff0809),
            (0x60304, 0x97ff12cb),
            (0x60310, 0x94027f58),
            (0x60560, 0xb9400028),
            (0x60564, 0xb9400049),
            (0x60568, 0x2a080128),
            (0x6056c, 0x34000048),
            (0x6058c, 0x9401c39d),
            (0x605a0, 0xa90ba289),
            (0x605a8, 0xf9005a88),
            (0x605ac, 0x97ffe261),
            (0x605bc, 0x97ff1b51),
            (0x5fe84, 0x7100051f),
            (0x5fe88, 0x54000361),
            (0x5fea4, 0x91333821),
            (0x5feb0, 0x97ff093c),
            (0x5fec8, 0x390293ff),
            (0x5ff24, 0x97ff13c3),
            (0x5ff30, 0x94028050),
            (0x5ff58, 0x39030909),
            (0x223b8, 0x91338c21),
            (0x223bc, 0x9403e9f1),
            (0x223c0, 0xb900a27f),
            (0x23b7c, 0x94025f9d),
            (0x23b80, 0x37000700),
            (0x23bbc, 0xb9000bff),
            (0x23bdc, 0xd63f0100),
            (0x23c24, 0x320003e9),
            (0x23b08, 0x39431268),
            (0x23b0c, 0x34002128),
        ] {
            assert_eq!(word(a), expected, "Native Config dialog{a:x}");
        }
        for (a, text) in [
            (0x15e89a, ConfigDialogKind::Reset.prompt()),
            (0x160cce, ConfigDialogKind::AutoSleep.prompt()),
            (0x160990, ConfigDialogKind::Reset.labels()[0]),
            (0x160c97, ConfigDialogKind::Reset.labels()[1]),
            (0x162ce3, ConfigDialogKind::AutoSleep.labels()[0]),
        ] {
            assert_eq!(&exe[a + 256..a + 256 + text.len()], text.as_bytes());
            assert_eq!(exe[a + 256 + text.len()], 0);
        }
    }

    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_config_dialog_owner_gpu_and_once_only_callbacks() {
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

        for kind in [ConfigDialogKind::Reset, ConfigDialogKind::AutoSleep] {
            let mut dialog = ConfigDialog::load(&server, kind, true).unwrap();
            dialog.set_pointer_extent((960, 540), true);
            dialog.pointer_state(Vec2::ZERO, false, 1.0 / 60.0);
            dialog.keyboard(&EnumMap::default());
            assert!(dialog.update(Ticks::from_f32(12.0)).is_empty());
            assert!(dialog.update(Ticks::ZERO).is_empty());
            assert_eq!(dialog.view.geometry.button_count, kind.labels().len());
            if kind == ConfigDialogKind::AutoSleep {
                dialog.keyboard(&key(AppAction::Cancel));
                assert!(dialog.confirmation.is_open());
                assert!(dialog.take_sounds().is_empty());
            }
            dialog.prepare(&mut context);
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    "Config native dialog owner",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(128),
                    Some(1.0),
                );
                dialog.render(&mut pass);
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            assert!(
                pixels
                    .chunks_exact(4)
                    .filter(|p| p[0] > 32 || p[1] > 32 || p[2] > 32)
                    .count()
                    > 1000
            );
            let path = match kind {
                ConfigDialogKind::Reset => "/private/tmp/shin-config-reset-dialog.rgba",
                ConfigDialogKind::AutoSleep => "/private/tmp/shin-config-notice-dialog.rgba",
            };
            std::fs::write(path, &pixels).unwrap();
            if kind == ConfigDialogKind::Reset {
                dialog.keyboard(&key(AppAction::Left));
            }
            dialog.keyboard(&key(AppAction::Enter));
            let mut responses = Vec::new();
            for _ in 0..40 {
                responses.extend(dialog.update(Ticks::from_f32(1.0)));
            }
            assert_eq!(
                responses,
                [
                    ConfigDialogResponse {
                        final_flag: false,
                        button: 0
                    },
                    ConfigDialogResponse {
                        final_flag: true,
                        button: 0
                    }
                ]
            );
            for _ in 0..40 {
                assert!(dialog.update(Ticks::from_f32(1.0)).is_empty());
            }
        }
    }
}
