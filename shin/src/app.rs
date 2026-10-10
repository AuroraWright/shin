mod first_boot;
mod scene_host;
mod shutdown;
mod touch_effect;
pub(crate) use scene_host::SceneHost;
use std::{sync::Arc, time::Duration};

use anyhow::Context;
use enum_map::{Enum, EnumMap};
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::instruction_elements::CodeAddress, primitives::update::FrameId, time::Ticks,
    vm::Scripter,
};
use shin_input::{
    Action, ActionState, RawInputState,
    inputs::{GamepadAxis, GamepadButton, MouseButton, VirtualGamepadButton},
};
use shin_render::render_pass::RenderPass;
use shin_window::{AppContext, RenderContext, ShinApp};
use tracing::debug;
use winit::keyboard::KeyCode;

use crate::{
    adv::{Adv, assets::AdvAssets},
    asset::system::{AssetLoadContext, AssetServer, cache::AssetCache, locate_assets},
    cli::Cli,
    render::PreRenderContext,
    update::UpdateContext,
};

#[derive(Debug, Clone, Copy, Enum)]
pub enum AppAction {
    Act,
    Enter,
    Cancel,
    AnyDown,
    Up,
    Left,
    Right,
    // Native low held bits0..3 exclude synthesized stick-direction bits16..19.
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    PageUp,
    PageDown,
    // Desktop scroll keys are separate from native shoulder-button actions.
    KeyboardScrollUp,
    KeyboardScrollDown,
    CharacterPageUp,
    CharacterPageDown,
    ScenarioTabPrevious,
    ScenarioTabNext,
    CharacterState,
    CharacterVariant,
    ViewerPanUp,
    ViewerPanDown,
    ViewerPanLeft,
    ViewerPanRight,
    TextScrollUp,
    TextScrollDown,
    PointerClick,
    HideMessage,
    SystemMenu,
    UnlockScenarios,
    ConfigReset,
    DeleteSave,
    Backlog,
    PreviousScene,
    ReplayVoice,
    ConfirmSceneJump,
    SceneJumpMenu,
    HoldSkip,
    ToggleSkip,
    ToggleAuto,
}

impl Action for AppAction {
    fn lower(raw_input_state: &RawInputState) -> EnumMap<Self, bool> {
        EnumMap::from_fn(|action| match action {
            AppAction::Act => {
                raw_input_state.keyboard.contains(&KeyCode::Space)
                    || raw_input_state.gamepads.is_held(GamepadButton::A)
                    || raw_input_state.mouse.buttons[MouseButton::Left]
            }
            AppAction::Enter => {
                raw_input_state.keyboard.contains(&KeyCode::Space)
                    | raw_input_state.keyboard.contains(&KeyCode::Enter)
                    | raw_input_state.gamepads.is_held(GamepadButton::A)
            }
            AppAction::Cancel => {
                raw_input_state.keyboard.contains(&KeyCode::Backspace)
                    || raw_input_state.keyboard.contains(&KeyCode::Escape)
                    || raw_input_state.gamepads.is_held(GamepadButton::B)
            }
            AppAction::AnyDown => {
                raw_input_state.keyboard.contains(&KeyCode::ArrowDown)
                    || raw_input_state.gamepads.is_held(GamepadButton::Down)
                    || raw_input_state
                        .gamepads
                        .is_vheld(VirtualGamepadButton::StickLDown)
            }
            AppAction::DpadUp => {
                raw_input_state.keyboard.contains(&KeyCode::ArrowUp)
                    || raw_input_state.gamepads.is_held(GamepadButton::Up)
            }
            AppAction::DpadDown => {
                raw_input_state.keyboard.contains(&KeyCode::ArrowDown)
                    || raw_input_state.gamepads.is_held(GamepadButton::Down)
            }
            AppAction::DpadLeft => {
                raw_input_state.keyboard.contains(&KeyCode::ArrowLeft)
                    || raw_input_state.gamepads.is_held(GamepadButton::Left)
            }
            AppAction::DpadRight => {
                raw_input_state.keyboard.contains(&KeyCode::ArrowRight)
                    || raw_input_state.gamepads.is_held(GamepadButton::Right)
            }
            AppAction::Left => {
                raw_input_state.keyboard.contains(&KeyCode::ArrowLeft)
                    || raw_input_state.gamepads.is_held(GamepadButton::Left)
                    || raw_input_state
                        .gamepads
                        .is_vheld(VirtualGamepadButton::StickLLeft)
            }
            AppAction::Right => {
                raw_input_state.keyboard.contains(&KeyCode::ArrowRight)
                    || raw_input_state.gamepads.is_held(GamepadButton::Right)
                    || raw_input_state
                        .gamepads
                        .is_vheld(VirtualGamepadButton::StickLRight)
            }
            AppAction::PageUp => {
                raw_input_state.keyboard.contains(&KeyCode::KeyK)
                    || raw_input_state.gamepads.is_held(GamepadButton::L)
            }
            AppAction::PageDown => {
                raw_input_state.keyboard.contains(&KeyCode::KeyL)
                    || raw_input_state.gamepads.is_held(GamepadButton::R)
            }
            AppAction::KeyboardScrollUp => raw_input_state.keyboard.contains(&KeyCode::KeyK),
            AppAction::KeyboardScrollDown => raw_input_state.keyboard.contains(&KeyCode::KeyL),
            AppAction::CharacterPageUp => {
                raw_input_state.keyboard.contains(&KeyCode::KeyH)
                    || raw_input_state.gamepads.is_held(GamepadButton::L)
            }
            AppAction::CharacterPageDown => {
                raw_input_state.keyboard.contains(&KeyCode::KeyJ)
                    || raw_input_state.gamepads.is_held(GamepadButton::R)
            }
            AppAction::ScenarioTabPrevious => {
                raw_input_state.keyboard.contains(&KeyCode::KeyH)
                    || raw_input_state.gamepads.is_held(GamepadButton::Left)
                    || raw_input_state
                        .gamepads
                        .is_vheld(VirtualGamepadButton::StickLLeft)
            }
            AppAction::ScenarioTabNext => {
                raw_input_state.keyboard.contains(&KeyCode::KeyJ)
                    || raw_input_state.gamepads.is_held(GamepadButton::Right)
                    || raw_input_state
                        .gamepads
                        .is_vheld(VirtualGamepadButton::StickLRight)
            }
            AppAction::CharacterState => {
                raw_input_state.keyboard.contains(&KeyCode::Tab)
                    || raw_input_state.gamepads.is_held(GamepadButton::Y)
            }
            AppAction::CharacterVariant => {
                raw_input_state.keyboard.contains(&KeyCode::KeyV)
                    || raw_input_state.gamepads.is_held(GamepadButton::X)
            }
            AppAction::ViewerPanUp => {
                raw_input_state.keyboard.contains(&KeyCode::ArrowUp)
                    || raw_input_state.gamepads.is_held(GamepadButton::Up)
            }
            AppAction::ViewerPanDown => {
                raw_input_state.keyboard.contains(&KeyCode::ArrowDown)
                    || raw_input_state.gamepads.is_held(GamepadButton::Down)
            }
            AppAction::ViewerPanLeft => {
                raw_input_state.keyboard.contains(&KeyCode::ArrowLeft)
                    || raw_input_state.gamepads.is_held(GamepadButton::Left)
            }
            AppAction::ViewerPanRight => {
                raw_input_state.keyboard.contains(&KeyCode::ArrowRight)
                    || raw_input_state.gamepads.is_held(GamepadButton::Right)
            }
            AppAction::TextScrollUp => {
                raw_input_state.keyboard.contains(&KeyCode::Home)
                    || raw_input_state.mouse.buttons[MouseButton::WheelUp]
            }
            AppAction::TextScrollDown => {
                raw_input_state.keyboard.contains(&KeyCode::End)
                    || raw_input_state.mouse.buttons[MouseButton::WheelDown]
            }
            AppAction::Up => {
                raw_input_state.keyboard.contains(&KeyCode::ArrowUp)
                    || raw_input_state.gamepads.is_held(GamepadButton::Up)
                    || raw_input_state
                        .gamepads
                        .is_vheld(VirtualGamepadButton::StickLUp)
            }
            AppAction::Backlog => {
                raw_input_state.keyboard.contains(&KeyCode::KeyB)
                    || raw_input_state.gamepads.is_held(GamepadButton::X)
            }
            AppAction::PreviousScene => {
                raw_input_state.keyboard.contains(&KeyCode::Backspace)
                    || raw_input_state.keyboard.contains(&KeyCode::ArrowUp)
                    || raw_input_state.gamepads.is_held(GamepadButton::Up)
                    || raw_input_state
                        .gamepads
                        .is_vheld(VirtualGamepadButton::StickLUp)
            }
            AppAction::ReplayVoice => raw_input_state.keyboard.contains(&KeyCode::KeyV),
            AppAction::SceneJumpMenu => {
                raw_input_state.keyboard.contains(&KeyCode::KeyG)
                    || raw_input_state.gamepads.is_held(GamepadButton::X)
            }
            AppAction::ConfirmSceneJump => {
                raw_input_state.keyboard.contains(&KeyCode::KeyY)
                    || raw_input_state.gamepads.is_held(GamepadButton::Y)
            }
            AppAction::HideMessage => {
                raw_input_state.keyboard.contains(&KeyCode::KeyH)
                    || raw_input_state.mouse.buttons[MouseButton::Right]
                    || raw_input_state.gamepads.is_held(GamepadButton::B)
            }
            AppAction::SystemMenu => {
                raw_input_state.keyboard.contains(&KeyCode::KeyM)
                    || raw_input_state.gamepads.is_held(GamepadButton::Y)
            }
            AppAction::UnlockScenarios => {
                raw_input_state.keyboard.contains(&KeyCode::KeyU)
                    || raw_input_state.gamepads.is_held(GamepadButton::Plus)
            }
            AppAction::DeleteSave => {
                raw_input_state.keyboard.contains(&KeyCode::Delete)
                    || raw_input_state.keyboard.contains(&KeyCode::KeyY)
                    || raw_input_state.gamepads.is_held(GamepadButton::Y)
            }
            AppAction::ConfigReset => {
                raw_input_state.keyboard.contains(&KeyCode::KeyY)
                    || raw_input_state.gamepads.is_held(GamepadButton::Y)
            }
            AppAction::PointerClick => raw_input_state.mouse.buttons[MouseButton::Left],
            AppAction::ToggleSkip => {
                raw_input_state.keyboard.contains(&KeyCode::KeyS)
                    || raw_input_state.gamepads.is_held(GamepadButton::R)
            }
            AppAction::ToggleAuto => {
                raw_input_state.keyboard.contains(&KeyCode::KeyA)
                    || raw_input_state.gamepads.is_held(GamepadButton::L)
            }
            AppAction::HoldSkip => raw_input_state.keyboard.contains(&KeyCode::ControlLeft),
        })
    }
}

pub struct App {
    shutdown: shutdown::Shutdown,
    platform_suspended: bool,
    frame_id: FrameId,
    effect_textures: Option<Arc<crate::asset::effects::EffectTextures>>,
    audio_manager: Arc<AudioManager>,
    asset_server: Arc<AssetServer>,
    scenes: SceneHost,
    pointer_position: glam::Vec2,
    character_scroll_axis: f32,
    touch_effect: touch_effect::TouchEffect,
    touch_background: Option<shin_render::render_texture::RenderTexture>,
    system_textures: Option<Arc<crate::adv::character_view::CharacterTextures>>,
}

impl App {
    fn shutdown_frame(
        &mut self,
        ctx: &mut PreRenderContext,
        delta: Ticks,
        input: &EnumMap<AppAction, ActionState>,
        pointer: glam::Vec2,
        extent: (u16, u16),
        elapsed: f64,
    ) -> shutdown::Frame {
        let result = self.shutdown.update(
            &mut self.scenes,
            &self.asset_server,
            ctx,
            delta,
            input,
            pointer,
            extent,
            elapsed,
        );
        self.audio_manager
            .set_suspended(self.platform_suspended || self.shutdown.pauses_scene());
        match result {
            Ok(frame) => frame,
            Err(error) => {
                tracing::error!(%error, "Shutdown notice could not load; closure cancelled");
                shutdown::Frame::Continue
            }
        }
    }
}

impl ShinApp for App {
    type Parameters = Cli;
    type EventType = ();
    type ActionType = AppAction;
    const FULLSCREEN_SHORTCUT: Option<KeyCode> = Some(KeyCode::KeyF);

    fn run_in_background(&self) -> bool {
        self.shutdown.active() || self.scenes.run_in_background()
    }

    fn focus_lost(&mut self) {
        self.scenes.suspend_event_input();
        self.touch_effect.cancel_contact();
    }

    fn set_suspended(&mut self, suspended: bool) {
        if suspended {
            self.scenes.suspend_event_input();
            self.touch_effect.cancel_contact();
        }
        self.platform_suspended = suspended;
        self.audio_manager
            .set_suspended(suspended || self.shutdown.pauses_scene());
    }

    fn close_requested(&mut self) -> bool {
        if !self.shutdown.active() {
            self.shutdown.request();
            self.scenes.suspend_event_input();
            self.touch_effect.cancel_contact();
        }
        false
    }

    fn init(context: AppContext<Self>, cli: Self::Parameters) -> anyhow::Result<Self> {
        #[cfg(not(test))]
        let audio_manager = Arc::new(if cli.loopback_audio {
            AudioManager::new_for_tests()
        } else {
            AudioManager::new()
        });
        #[cfg(test)]
        let audio_manager = Arc::new(AudioManager::new_for_tests());

        let asset_io = locate_assets(cli.assets_dir.as_deref()).context("Failed to locate assets. Consult the README for instructions on how to set up the game.")?;

        debug!("Asset IO: {:#?}", asset_io);

        let asset_server = Arc::new(AssetServer::new(
            asset_io.into(),
            AssetLoadContext {
                wgpu_device: context.wgpu.device.clone(),
                wgpu_queue: context.wgpu.queue.clone(),
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));

        if cli.unsafe_entry_point.is_none()
            && cli.fast_forward_to.is_none()
            && cli.load_save.is_none()
        {
            let audio = crate::audio::GlobalAudio::new(audio_manager.clone());
            return Ok(Self {
                shutdown: shutdown::Shutdown::default(),
                platform_suspended: false,
                frame_id: FrameId::default(),
                effect_textures: None,
                audio_manager,
                asset_server: asset_server.clone(),
                scenes: SceneHost::new_first_boot(audio, asset_server, cli.save_file),
                pointer_position: glam::Vec2::ZERO,
                character_scroll_axis: 0.0,
                touch_effect: touch_effect::TouchEffect::default(),
                touch_background: None,
                system_textures: None,
            });
        }

        // Explicit saved/debug story launches prepare their requested ADV.
        let adv_assets = shin_tasks::block_on(AdvAssets::load(&asset_server))?;
        let effect_textures = Some(shin_tasks::block_on(asset_server.load("/effects.txa"))?);
        let system_textures = Some(shin_tasks::block_on(asset_server.load("/systex.txa"))?);

        let mut scripter = Scripter::new(&adv_assets.scenario, 0, 42);

        if let Some(addr) = cli.unsafe_entry_point {
            debug!("Starting execution from 0x{:x}", addr);
            scripter.unsafe_set_position(CodeAddress(addr));
        }

        let mut adv = Adv::new(
            audio_manager.clone(),
            asset_server.clone(),
            adv_assets.clone(),
            scripter,
        );

        let imported_save = cli
            .load_save
            .as_ref()
            .map(|path| {
                let bytes = std::fs::read(path)
                    .with_context(|| format!("Reading save {}", path.display()))?;
                shin_core::format::save::SaveData::decode(&bytes).context("Decoding saved game")
            })
            .transpose()?;
        if let Some(path) = &cli.save_file {
            adv.configure_save_file(path.clone(), imported_save.clone())
                .context("Opening progress save file")?;
        } else if let Some(save) = &imported_save {
            adv.restore_play_clock(save.play_seconds);
        }
        if let Some(save) = &imported_save {
            let slot = if cli.save_slot == 0 {
                save.auto_save_slot.as_ref()
            } else {
                save.manual_save_slots[cli.save_slot as usize - 1].as_ref()
            }
            .context("Selected save slot is empty")?;
            adv.load_game_data(save, slot, asset_server.clone())
                .context("Reconstructing saved scene")?;
        }

        if let Some(addr) = cli.fast_forward_to {
            debug!("Fast forwarding to 0x{:x}", addr);
            adv.fast_forward_to(CodeAddress(addr));
        }

        // let picture_name = "/picture/text001.pic";
        //
        // let picture = asset_server.load_sync::<Picture>(picture_name).unwrap();
        //
        // let mut tile_layer_bottom = TileLayer::new(
        //     FloatColor4::PASTEL_PINK,
        //     vec4(-1088.0, -612.0, 2176.0, 1224.0),
        // );
        // tile_layer_bottom
        //     .properties_mut()
        //     .set_layer_id(LayerId::new(1));
        //
        // let mut tile_layer_top = TileLayer::new(
        //     FloatColor4::PASTEL_GREEN,
        //     vec4(-300.0, -300.0, 600.0, 600.0),
        // );
        // {
        //     let props = tile_layer_top.properties_mut();
        //     props.set_layer_id(LayerId::new(3));
        //     props
        //         .property_tweener_mut(LayerProperty::MulColorAlpha)
        //         .fast_forward_to(200.0);
        // }
        //
        // let mut picture_layer = PictureLayer::new(picture, Some(picture_name.to_string()));
        // {
        //     let props = picture_layer.properties_mut();
        //     props.set_layer_id(LayerId::new(2));
        //     props
        //         .property_tweener_mut(LayerProperty::TranslateZ)
        //         .fast_forward_to(1500.0);
        //     props
        //         .property_tweener_mut(LayerProperty::MulColorAlpha)
        //         .fast_forward_to(800.0);
        // }
        //
        // let mut root_layer_group = RootLayerGroup::new();
        //
        // let screen_layer = root_layer_group.screen_layer_mut();
        //
        // let page_layer = screen_layer.page_layer_mut();
        //
        // let layer_group = page_layer.get_plane_mut(PlaneId::new(0));
        // layer_group.add_layer(LayerbankId::new(1), tile_layer_bottom.into());
        // layer_group.add_layer(LayerbankId::new(0), picture_layer.into());
        // layer_group.add_layer(LayerbankId::new(2), tile_layer_top.into());
        //
        // {
        //     let tweener = page_layer
        //         .properties_mut()
        //         .property_tweener_mut(LayerProperty::MulColorRed);
        //
        //     tweener.enqueue(2000.0, Tween::linear(Ticks::from_seconds(0.5)));
        //     tweener.enqueue(1000.0, Tween::linear(Ticks::from_seconds(0.5)));
        //     tweener.enqueue(2000.0, Tween::linear(Ticks::from_seconds(0.5)));
        //     tweener.enqueue(1000.0, Tween::linear(Ticks::from_seconds(0.5)));
        //
        //     // tweener.enqueue(2000.0, Tween::linear(Ticks::from_seconds(1.0)));
        //     // tweener.enqueue(0.0, Tween::linear(Ticks::from_seconds(1.0)));
        //     // tweener.enqueue(2000.0, Tween::linear(Ticks::from_seconds(1.0)));
        //     // tweener.enqueue(0.0, Tween::linear(Ticks::from_seconds(1.0)));
        //     // tweener.enqueue(1000.0, Tween::linear(Ticks::from_seconds(0.5)));
        // }

        Ok(Self {
            shutdown: shutdown::Shutdown::default(),
            platform_suspended: false,
            frame_id: FrameId::default(),
            effect_textures,
            pointer_position: glam::Vec2::ZERO,
            character_scroll_axis: 0.0,
            touch_effect: touch_effect::TouchEffect::default(),
            touch_background: None,
            system_textures,
            audio_manager,
            asset_server: asset_server.clone(),
            scenes: SceneHost::new(adv, adv_assets, asset_server.clone()),
        })
    }

    fn custom_event(&mut self, _context: AppContext<Self>, _event: Self::EventType) {
        todo!()
    }

    fn update(
        &mut self,
        context: AppContext<Self>,
        input: EnumMap<Self::ActionType, ActionState>,
        elapsed_time: Duration,
        command_encoder: &mut wgpu::CommandEncoder,
    ) {
        let frame_started_at = std::time::Instant::now();
        // if input[AppAction::Act].is_clicked {
        //     let screen_layer = self.root_layer_group.screen_layer_mut();
        //
        //     screen_layer.pageback(false);
        //     screen_layer
        //         .page_layer_mut()
        //         .properties_mut()
        //         .property_tweener_mut(LayerProperty::MulColorGreen)
        //         .fast_forward_to(2000.0);
        //     screen_layer.apply_transition(Some(DefaultWiper::new(Ticks::from_seconds(1.0)).into()));
        // }

        if self.effect_textures.is_none() {
            self.effect_textures = self.scenes.effect_textures().cloned();
        }
        let effect_textures = self.effect_textures.clone();
        let asset_server = self.asset_server.clone();
        let mut pre_render_context = PreRenderContext {
            breakup_texture: effect_textures.as_ref().map(|effects| &effects.breakup),
            device: &context.wgpu.device,
            queue: &context.wgpu.queue,
            resize_source: &context.winit.resize_source,
            sampler_store: &context.render.sampler_store,
            depth_stencil: context.render.canvas_depth_stencil_buffer.get_target_view(),

            pipeline_storage: &mut context.render.pipelines,
            dynamic_buffer: &mut context.render.dynamic_buffer,
            encoder: command_encoder,
        };

        let mut update_context = UpdateContext {
            frame_started_at,
            frame_id: self.frame_id,
            delta_ticks: Ticks::from_duration(elapsed_time),
            asset_server: &asset_server,
            pre_render: &mut pre_render_context,
        };

        let viewport = Self::map_canvas_size(context.winit.window.inner_size());
        let pointer = (self.pointer_position
            - glam::vec2(
                viewport.canvas_offset.x as f32,
                viewport.canvas_offset.y as f32,
            ))
            * glam::vec2(
                1920.0 / viewport.canvas_size.width.max(1) as f32,
                1080.0 / viewport.canvas_size.height.max(1) as f32,
            );
        match self.shutdown_frame(
            update_context.pre_render,
            update_context.delta_ticks,
            &input,
            pointer,
            (
                viewport.canvas_size.width.clamp(1, u16::MAX as u32) as u16,
                viewport.canvas_size.height.clamp(1, u16::MAX as u32) as u16,
            ),
            elapsed_time.as_secs_f64(),
        ) {
            shutdown::Frame::Exit => {
                context.event_loop.exit();
                return;
            }
            shutdown::Frame::Consumed => {
                self.touch_background = None;
                self.touch_effect.cancel_contact();
                return;
            }
            shutdown::Frame::Continue => {}
        }
        if let Err(error) = self.scenes.update(
            &mut update_context,
            input,
            self.pointer_position
                - glam::vec2(
                    viewport.canvas_offset.x as f32,
                    viewport.canvas_offset.y as f32,
                ),
            pointer,
            (
                viewport.canvas_size.width.clamp(1, u16::MAX as u32) as u16,
                viewport.canvas_size.height.clamp(1, u16::MAX as u32) as u16,
            ),
            self.character_scroll_axis,
            elapsed_time.as_secs_f64(),
        ) {
            tracing::error!(%error, "Scene update failed");
        }
        if self.system_textures.is_none() {
            self.system_textures = self.scenes.system_textures().cloned();
        }
        // Use the post-update settings so Config toggles and reset take effect
        // in this frame. Pointer coordinates already exclude letterbox bars.
        self.touch_effect.update_in_viewport(
            Ticks::from_duration(elapsed_time),
            self.scenes.touch_effects_enabled(),
            input[AppAction::PointerClick].is_held,
            pointer,
            (
                viewport.canvas_size.width.clamp(1, u16::MAX as u32) as u16,
                viewport.canvas_size.height.clamp(1, u16::MAX as u32) as u16,
            ),
        );
        if self.touch_effect.has_ripples() {
            let background = self.touch_background.get_or_insert_with(|| {
                update_context
                    .pre_render
                    .new_render_texture("Touch ripple scene capture".into())
            });
            let target = background.as_texture_target();
            let depth = update_context.pre_render.depth_stencil;
            let mut pass = update_context.pre_render.begin_pass(
                target,
                Some(depth),
                "Touch ripple scene capture",
            );
            self.scenes.render(&mut pass);
        } else {
            self.touch_background = None;
        }

        // let update_context = AdvUpdateContext {
        //     delta_time: Ticks::from_duration(elapsed_time),
        //     asset_server: &self.asset_server,
        //     are_animations_allowed: true,
        // };
        //
        // self.root_layer_group.update(&update_context);
        //
        // let transform = TransformParams::default();
        //
        // let mut encoder =
        //     context
        //         .wgpu
        //         .device
        //         .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        //             label: Some("App::update"),
        //         });
        //
        // let mut pre_render_context = PreRenderContext {
        //     breakup_texture: None,
        //     device: &context.wgpu.device,
        //     queue: &context.wgpu.queue,
        //     resize_source: &context.winit.resize_source,
        //     sampler_store: &context.render.sampler_store,
        //     depth_stencil: context.render.canvas_depth_stencil_buffer.get_target_view(),
        //
        //     pipeline_storage: &mut context.render.pipelines,
        //     dynamic_buffer: &mut context.render.dynamic_buffer,
        //     encoder: &mut encoder,
        // };
        //
        // self.root_layer_group
        //     .pre_render(&mut pre_render_context, &transform);

        self.frame_id.advance();
    }

    fn raw_input(&mut self, state: &RawInputState) {
        self.scenes.raw_input(state);
        self.pointer_position = state.mouse.position;
        self.character_scroll_axis = -state.gamepads.unified.axes[GamepadAxis::RightStickY];
    }

    #[tracing::instrument(skip_all)]
    fn render(&mut self, _context: RenderContext, pass: &mut RenderPass) {
        if let Some(background) = &self.touch_background {
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            shin_render::quad_vertices::QuadVertices::new()
                .with_box(0.0, 0.0, 1920.0, 1080.0)
                .render_sprite(
                    pass,
                    shin_render::RenderRequestBuilder::new(),
                    background.as_texture_source(),
                    crate::render::top_left_projection_matrix(),
                );
            self.touch_effect
                .render_ripples(pass, background.as_texture_source());
        } else {
            self.scenes.render(pass);
        }
        if let Some(textures) = &self.system_textures {
            self.touch_effect.render(pass, textures);
        }

        self.shutdown.render(pass);

        // render_layer(pass, &transform, &self.adv, FloatColor4::BLACK, 0);
    }
}

#[cfg(test)]
mod controller_tests {
    use super::*;
    #[test]
    fn viewer_pan_preserves_dpad_and_keyboard_without_combined_stick_actions() {
        let mut raw = RawInputState::new();
        for (button, key, virtual_button, action) in [
            (
                GamepadButton::Up,
                KeyCode::ArrowUp,
                VirtualGamepadButton::StickLUp,
                AppAction::ViewerPanUp,
            ),
            (
                GamepadButton::Down,
                KeyCode::ArrowDown,
                VirtualGamepadButton::StickLDown,
                AppAction::ViewerPanDown,
            ),
            (
                GamepadButton::Left,
                KeyCode::ArrowLeft,
                VirtualGamepadButton::StickLLeft,
                AppAction::ViewerPanLeft,
            ),
            (
                GamepadButton::Right,
                KeyCode::ArrowRight,
                VirtualGamepadButton::StickLRight,
                AppAction::ViewerPanRight,
            ),
        ] {
            raw.gamepads.unified.virtual_keys[virtual_button] = true;
            assert!(!AppAction::lower(&raw)[action]);
            raw.gamepads.unified.buttons[button] = true;
            assert!(AppAction::lower(&raw)[action]);
            raw.gamepads.unified.buttons[button] = false;
            raw.keyboard.insert(key);
            assert!(AppAction::lower(&raw)[action]);
            raw.keyboard.remove(&key);
            raw.gamepads.unified.virtual_keys[virtual_button] = false;
        }
    }
    #[test]
    fn switch_buttons_and_left_stick_lower_to_native_character_actions() {
        let mut raw = RawInputState::new();
        for (button, action) in [
            (GamepadButton::A, AppAction::Enter),
            (GamepadButton::B, AppAction::Cancel),
            (GamepadButton::X, AppAction::CharacterVariant),
            (GamepadButton::Y, AppAction::CharacterState),
            (GamepadButton::Y, AppAction::ConfigReset),
            (GamepadButton::Plus, AppAction::UnlockScenarios),
            (GamepadButton::L, AppAction::PageUp),
            (GamepadButton::R, AppAction::PageDown),
        ] {
            raw.gamepads.unified.buttons[button] = true;
            assert!(AppAction::lower(&raw)[action]);
            raw.gamepads.unified.buttons[button] = false;
            assert!(!AppAction::lower(&raw)[action]);
        }
        raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLLeft] = true;
        assert!(AppAction::lower(&raw)[AppAction::Left]);
        raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickRLeft] = true;
        raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLLeft] = false;
        assert!(!AppAction::lower(&raw)[AppAction::Left]);
    }
    #[test]
    fn quick_return_uses_up_and_left_stick_while_retaining_navigation() {
        let mut raw = RawInputState::new();
        raw.gamepads.unified.buttons[GamepadButton::Up] = true;
        let actions = AppAction::lower(&raw);
        assert!(actions[AppAction::PreviousScene]);
        assert!(actions[AppAction::Up]);
        raw.gamepads.unified.buttons[GamepadButton::Up] = false;
        raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLUp] = true;
        let actions = AppAction::lower(&raw);
        assert!(actions[AppAction::PreviousScene]);
        assert!(actions[AppAction::Up]);
        raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLUp] = false;
        raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickRUp] = true;
        assert!(!AppAction::lower(&raw)[AppAction::PreviousScene]);
        raw.keyboard.insert(KeyCode::ArrowUp);
        assert!(AppAction::lower(&raw)[AppAction::PreviousScene]);
        raw.keyboard.clear();
        raw.keyboard.insert(KeyCode::Backspace);
        let actions = AppAction::lower(&raw);
        assert!(actions[AppAction::PreviousScene]);
        assert!(actions[AppAction::Cancel]);
    }
    #[test]
    fn story_buttons_retain_modal_actions() {
        let mut raw = RawInputState::new();
        raw.gamepads.unified.buttons[GamepadButton::X] = true;
        let actions = AppAction::lower(&raw);
        assert!(actions[AppAction::Backlog]);
        assert!(actions[AppAction::SceneJumpMenu]);
        assert!(actions[AppAction::CharacterVariant]);
        assert!(!actions[AppAction::HideMessage]);
        raw.gamepads.unified.buttons[GamepadButton::X] = false;
        raw.gamepads.unified.buttons[GamepadButton::B] = true;
        let actions = AppAction::lower(&raw);
        assert!(actions[AppAction::HideMessage]);
        assert!(!actions[AppAction::SystemMenu]);
        assert!(actions[AppAction::Cancel]);
        assert!(!actions[AppAction::Backlog]);
        raw.gamepads.unified.buttons[GamepadButton::B] = false;
        raw.gamepads.unified.buttons[GamepadButton::Y] = true;
        let actions = AppAction::lower(&raw);
        assert!(!actions[AppAction::HideMessage]);
        assert!(actions[AppAction::CharacterState]);
        assert!(actions[AppAction::SystemMenu]);
        raw.gamepads.unified.buttons[GamepadButton::Y] = false;
        raw.gamepads.unified.buttons[GamepadButton::L] = true;
        let actions = AppAction::lower(&raw);
        assert!(actions[AppAction::ToggleAuto]);
        assert!(actions[AppAction::PageUp]);
        assert!(!actions[AppAction::ToggleSkip]);
        raw.gamepads.unified.buttons[GamepadButton::L] = false;
        raw.gamepads.unified.buttons[GamepadButton::R] = true;
        let actions = AppAction::lower(&raw);
        assert!(actions[AppAction::ToggleSkip]);
        assert!(actions[AppAction::PageDown]);
        assert!(!actions[AppAction::ToggleAuto]);
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_character_controller_priority_and_axes() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, value) in [
            (0x53bf4, 0x37500409),
            (0x53bf8, 0x36580509),
            (0x53c9c, 0x531b6d2a),
            (0x53ca0, 0x7206013f),
            (0x53ca4, 0x5a9f0155),
            (0x53ca8, 0x5319652a),
            (0x53cac, 0x7208013f),
            (0x53cb0, 0x5a9f0156),
            (0x53cb8, 0x54000f41),
            (0x53cc0, 0x34000f68),
            (0x53eb0, 0x373003e8),
            (0x53eb4, 0x37380488),
            (0x53eb8, 0x37e00548),
            (0x53ebc, 0x37e80588),
            (0x53ec0, 0xbd4027e0),
            (0x53edc, 0x1e261002),
            (0x53ee0, 0x1e220821),
            (0x53f3c, 0x97fffdad),
            (0x53f54, 0x320003e1),
            (0x53f58, 0x97fffd0e),
            (0x53f64, 0x97fffc87),
            (0x53f84, 0x97fffe57),
            (0xc45d8, 0x4b0a03ea),
            (0xc45dc, 0x4b0b03eb),
            (0xc4694, 0xbd002e82),
            (0xc49c0, 0x29452d2a),
            (0xc49c4, 0x29032d0a),
            (0xc4558, 0x330d358d),
            (0xc4750, 0x7210013f),
            (0xc4768, 0x1a9f87ea),
            (0xc476c, 0x2a0a4108),
            (0xc4888, 0x7200811f),
            (0xc4890, 0x32080108),
            (0xc48cc, 0x0a2c010c),
            (0xc48e4, 0x29003288),
            (0xc4908, 0x7100621f),
            (0xc490c, 0x7a402a20),
            (0xc4910, 0x54000060),
            (0xc491c, 0x320003f1),
            (0xc4924, 0x7a4d0200),
            (0xc493c, 0x0a0f0252),
            (0xc4944, 0x2a0a024a),
            (0xc4974, 0x2a0a0129),
            (0xc4978, 0x2900250a),
            (0xc49b0, 0x29412d2a),
            (0xc49b4, 0x29012d0a),
            (0x11f04, 0x910023e8),
            (0x11f08, 0x9402caa6),
            (0x12138, 0x39404fe8),
            (0x1213c, 0x37000168),
            (0x1216c, 0x940005a1),
            (0xc4574, 0x53027d8e),
            (0xc4588, 0x331a01cd),
            (0xc46a4, 0x7200001f),
            (0xc46c4, 0x1ac92108),
            (0xc46d0, 0x1aca22aa),
            (0xc46d8, 0x2a150108),
            (0xc46dc, 0x0a0b014a),
            (0xc46e0, 0x2a0a0108),
            (0x120b0, 0xb9400fe8),
            (0x120b8, 0x37300d08),
            (0x1225c, 0x94000469),
            (0x1348c, 0x94025ee9),
            (0x120c8, 0x39403fe8),
            (0x120cc, 0x37280be8),
            (0x1224c, 0x94000535),
            (0x1bd90, 0xf943ad29),
            (0x1bda8, 0xf900000a),
            (0x1bfdc, 0x9402a271),
            (0x1bfe0, 0xb9400fe8),
            (0x1bfe4, 0x52801809),
            (0x1bfe8, 0x72a60009),
            (0x1bfec, 0x6a09011f),
            (0x1bff0, 0x54fffe40),
            (0x1bff8, 0x39020268),
            (0xc4584, 0x531c6d90),
            (0xc45a8, 0x1216020e),
            (0xc45b0, 0x1215020e),
            (0x11f0c, 0x39557268),
            (0x11f14, 0x394037e8),
            (0x11f18, 0x37100208),
            (0x11f1c, 0x39585268),
            (0x11f24, 0x394037e8),
            (0x11f28, 0x371805a8),
            (0x11f58, 0xb9447e68),
            (0x11f5c, 0x52000109),
            (0x11f68, 0xb9047e69),
            (0x11fdc, 0xb9447e68),
            (0x11fe0, 0x521f0108),
            (0x11fec, 0xb9047e68),
            (0x11f2c, 0xb9447e68),
            (0x11f30, 0x34000288),
            (0x11f34, 0xb9400fe8),
            (0x11f38, 0x52801809),
            (0x11f3c, 0x72a40009),
            (0x11f40, 0x6a09011f),
            (0x11f44, 0x540001e0),
            (0x11f48, 0xb9047e7f),
            (0x11f54, 0x1400002b),
            (0x11160, 0x52922789),
            (0x11164, 0x38696908),
            (0x11178, 0x1a9f07f4),
            (0x11208, 0x2a1403e2),
            (0x1120c, 0x940002f5),
            (0x11218, 0x2a1403e2),
            (0x1121c, 0x940002f1),
            (0x11234, 0xb96ddd08),
            (0x11238, 0x531f7d02),
            (0x1233c, 0xb96ddc28),
            (0x12340, 0x37f80168),
            (0x12474, 0xb96dde68),
            (0x12478, 0x36fffec8),
        ] {
            assert_eq!(word(a), value, "Native controller{a:x}");
        }
        for (a, v) in [
            (0x164d00, 0.4f32),
            (0x164d04, 0.7),
            (0x164d18, -0.4),
            (0x164d1c, -0.7),
        ] {
            assert_eq!(word(a), v.to_bits());
        }
        for (address, expected) in [
            (0x164d82, b"N4shin3ADV12HiddenWindowE".as_slice()),
            (0x164d9c, b"N4shin3ADV10SystemMenuE".as_slice()),
            (0x15d090, b"son".as_slice()),
            (0x160ca1, b"cancel".as_slice()),
        ] {
            assert_eq!(&exe[address + 256..][..expected.len()], expected);
        }
    }
}
