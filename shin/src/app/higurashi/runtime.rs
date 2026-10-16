//! One normal Higurashi platform callback: deferred factories, input, scene
//! clocks, whole-scene composition and desktop close. Presentation is a getter.
use super::{
    gameplay::title_slot::{ActiveGlobals, TitleSlot},
    input_bridge::Input,
    load_game::Storage,
    persistence::FrozenProgress,
    scene_host::SceneHost,
    shutdown::{self, Shutdown},
};
use crate::{
    adv::{
        higurashi::{
            audio::AudioFramePolicy,
            runner::{Policy, Status},
        },
        input_area::PointerSampler,
    },
    asset::system::AssetServer,
    audio::SystemSoundPlayer,
    render::PreRenderContext,
};
use anyhow::{Context, Result, ensure};
use glam::{Vec2, Vec4};
use parking_lot::Mutex;
use shin_core::{
    primitives::{
        color::{FloatColor4, UnormColor},
        update::FrameId,
    },
    time::Ticks,
};
use shin_input::RawInputState;
use shin_render::{
    DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind, RenderProgramWithArguments,
    RenderRequestBuilder,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::{buffer::VertexSource, texture::DepthStencilTarget, vertices::PosTexVertex},
};
use std::{sync::Arc, time::Duration};

/// Original cc9c0/cc9d0 touch domain. InputArea subsequently maps these physical
/// touch coordinates to authored1920x1080, exactly once within each owner.
const TOUCH_EXTENT: (u16, u16) = (1280, 720);
const COMPOSED_EXTENT: (u32, u32) = (1920, 1080);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct InputViewport {
    pub offset: Vec2,
    pub size: Vec2,
}
impl InputViewport {
    pub fn new(offset: Vec2, size: Vec2) -> Result<Self> {
        ensure!(
            offset.is_finite() && size.is_finite() && size.x > 0. && size.y > 0.,
            "Invalid Higurashi input viewport"
        );
        Ok(Self { offset, size })
    }
    fn touch_position(self, physical: Vec2) -> Vec2 {
        (physical - self.offset) * Vec2::new(TOUCH_EXTENT.0 as f32, TOUCH_EXTENT.1 as f32)
            / self.size
    }
    fn contains(self, physical: Vec2) -> bool {
        let local = physical - self.offset;
        local.x >= 0. && local.y >= 0. && local.x < self.size.x && local.y < self.size.y
    }
}
struct Composition {
    resize: SurfaceResizeSource,
    color: RenderTexture,
    // Scene factories/save previews retain the clean color above. Global touch
    // feedback and desktop notices belong only to the displayed composition.
    presentation: Option<RenderTexture>,
    _depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
}
impl Composition {
    fn new(context: &mut PreRenderContext) -> Self {
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            COMPOSED_EXTENT.0,
            COMPOSED_EXTENT.1,
        )));
        let mut color = RenderTexture::new(
            context.device.clone(),
            resize.handle(),
            "Higurashi whole platform composition".into(),
        );
        let depth = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Higurashi whole platform depth"),
            size: wgpu::Extent3d {
                width: COMPOSED_EXTENT.0,
                height: COMPOSED_EXTENT.1,
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
        context
            .begin_pass(
                color.as_texture_target(),
                None,
                "Initial Higurashi platform black",
            )
            .clear(Some(UnormColor::BLACK), None, None);
        Self {
            resize,
            color,
            presentation: None,
            _depth: depth,
            depth_view,
        }
    }
    fn displayed(&self) -> &RenderTexture {
        self.presentation.as_ref().unwrap_or(&self.color)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Frame {
    Advanced(Option<Status>),
    Closing,
    Exit,
}

pub(super) struct Runtime {
    // Bootstrap and a temporarily vacant Slot coexist during idle-demo, so the
    // Slot's preserved histories/clock/seed continuation survives its return.
    bootstrap: Option<SceneHost>,
    slot: Option<Box<TitleSlot>>,
    composition: Option<Composition>,
    closing_base: Option<RenderTexture>,
    input: Input,
    /// Read-only observation before scene-requested end-of-frame quarantine.
    #[cfg(test)]
    reference_sampled_buttons: crate::adv::higurashi::input::Buttons,
    pointer: PointerSampler,
    pointer_contact: bool,
    touch_effect: crate::app::touch_effect::TouchEffect,
    last_diagnostic_phase: Option<&'static str>,
    last_diagnostic_save_selection: Option<usize>,
    viewport: Option<InputViewport>,
    frame_id: FrameId,
    shutdown: Shutdown,
    server: Arc<AssetServer>,
    frames: u64,
    exited: bool,
}
impl Runtime {
    pub fn new(
        server: Arc<AssetServer>,
        manager: Arc<shin_audio::AudioManager>,
        sounds: Arc<Mutex<SystemSoundPlayer>>,
        storage: Arc<dyn Storage>,
    ) -> Self {
        Self {
            bootstrap: Some(SceneHost::new(
                server.clone(),
                manager,
                sounds,
                Arc::new(super::load_game::DesktopStorage(storage)),
            )),
            slot: None,
            composition: None,
            closing_base: None,
            input: Input::default(),
            #[cfg(test)]
            reference_sampled_buttons: Default::default(),
            pointer: PointerSampler::default(),
            pointer_contact: false,
            touch_effect: Default::default(),
            last_diagnostic_phase: None,
            last_diagnostic_save_selection: None,
            viewport: None,
            frame_id: FrameId::default(),
            shutdown: Shutdown::default(),
            server,
            frames: 0,
            exited: false,
        }
    }
    pub fn raw_input(&mut self, raw: &RawInputState) {
        self.input.raw_input(raw);
    }
    pub fn set_focused(&mut self, focused: bool) {
        self.input.set_focused(focused);
        if !focused {
            self.touch_effect.cancel_contact();
        }
    }
    fn flush_sampled_input(&mut self) {
        self.touch_effect.cancel_contact();
        self.pointer.cancel_contact(self.input.mouse_left_held());
        self.pointer_contact = false;
        self.input.flush();
    }
    pub fn cancel_input(&mut self) -> Result<()> {
        self.flush_sampled_input();
        if let Some(bootstrap) = &mut self.bootstrap {
            bootstrap.cancel_input();
        }
        if self.bootstrap.is_none() {
            if let Some(slot) = &mut self.slot {
                slot.cancel_input()?;
            }
        }
        Ok(())
    }
    pub fn focus_lost(&mut self) -> Result<()> {
        self.cancel_input()?;
        self.input.focus_lost();
        Ok(())
    }
    pub fn request_close(&mut self) -> Result<()> {
        self.shutdown.request();
        self.cancel_input()
    }
    pub fn close_active(&self) -> bool {
        self.shutdown.active()
    }
    pub fn freezes_scene(&self) -> bool {
        self.shutdown.freezes_scene()
    }
    /// The window updates AudioManager's suspension gate first, including on
    /// callbacks which stop scene updates. Notify silent Movie clocks now.
    pub fn sync_movie_suspension(&self) {
        if let Some(slot) = &self.slot {
            slot.sync_movie_suspension();
        }
    }
    #[cfg(test)]
    pub fn failure(&self) -> Option<&str> {
        self.shutdown.failure()
    }
    pub fn globals(&self) -> Result<Option<ActiveGlobals<'_>>> {
        if let Some(bootstrap) = &self.bootstrap {
            return Ok(bootstrap.globals().map(ActiveGlobals::from));
        }
        self.slot
            .as_ref()
            .map(|slot| slot.active_globals())
            .transpose()
    }
    fn has_save_task(&self) -> bool {
        self.bootstrap.is_none() && self.slot.as_ref().is_some_and(|slot| slot.has_save_task())
    }
    fn freeze_progress(&self) -> Result<Option<FrozenProgress>> {
        if self.bootstrap.is_none() {
            if let Some(game) = self.slot.as_ref().and_then(|slot| slot.game.as_ref()) {
                return game.freeze_progress().map(Some);
            }
        }
        let Some(globals) = self.globals()? else {
            return Ok(None);
        };
        let mut data = globals.data.clone();
        data.play_seconds = globals.clock.saved_seconds();
        Ok(Some(FrozenProgress {
            data,
            slots: globals.runtime_saves.cloned(),
            storage: globals
                .storage
                .cloned()
                .context("Higurashi progress has no storage owner")?,
        }))
    }
    fn quiesce_progress(&mut self) -> Result<()> {
        if self.bootstrap.is_none() {
            if let Some(slot) = &mut self.slot {
                if let Some(game) = &mut slot.game {
                    game.quiesce_progress()?;
                }
                if let Some(title) = &mut slot.title {
                    title.quiesce_progress()?;
                }
            }
        }
        Ok(())
    }
    /// Called for background callbacks too. Dynamic uploads must be submitted
    /// before this encoder, as shin-window already does for ordinary updates.
    pub async fn update(
        &mut self,
        context: &mut PreRenderContext<'_, '_, '_, '_>,
        elapsed: Duration,
        viewport: InputViewport,
        permission: bool,
        policy: Policy,
        audio_policy: AudioFramePolicy,
    ) -> Result<Frame> {
        ensure!(!self.exited, "Higurashi runtime has already retired");
        let elapsed_ticks = Ticks::from_duration(elapsed);
        let delta = crate::adv::higurashi::host::frame_delta(elapsed_ticks)?;
        if self.viewport.is_some_and(|old| old != viewport) {
            self.cancel_input()?;
        }
        self.viewport = Some(viewport);
        let pending_factory = self
            .bootstrap
            .as_ref()
            .is_some_and(SceneHost::pending_scene_factory)
            || (self.bootstrap.is_none()
                && self
                    .slot
                    .as_ref()
                    .is_some_and(|slot| slot.pending_scene_factory()));
        if pending_factory {
            self.cancel_input()?;
        }
        if self.shutdown.take_input_flush() {
            self.flush_sampled_input();
        }
        let buttons = self.input.update(elapsed);
        #[cfg(test)]
        {
            self.reference_sampled_buttons = buttons;
        }
        let analog = self.input.analog();
        let held = self.input.mouse_left_held();
        if held && !self.pointer_contact && !viewport.contains(self.input.pointer_position()) {
            // A hold beginning in a letterbox cannot turn into a new tap by
            // entering the canvas without a release. Existing drags may leave.
            self.pointer.cancel_contact(true);
        }
        self.pointer.sample(
            viewport.touch_position(self.input.pointer_position()),
            held,
            elapsed.as_secs_f64(),
        );
        self.pointer_contact =
            held && (self.pointer_contact || viewport.contains(self.input.pointer_position()));
        self.frame_id.advance();
        if self.shutdown.requested() && !self.has_save_task() {
            self.quiesce_progress()?;
            self.closing_base = self
                .composition
                .as_ref()
                .map(|frame| frame.color.copy_snapshot(&mut context.render_clone_ctx()));
            self.shutdown.begin(self.freeze_progress()?);
        }
        let close = self.shutdown.update_with_permission(
            &self.server,
            delta,
            permission,
            buttons,
            &self.pointer.frame(),
        )?;
        if close == shutdown::Frame::Exit {
            // Decoder/private-loader retirement precedes releasing the window's
            // global AudioManager, and startup cannot overwrite unknown progress.
            drop(self.bootstrap.take());
            drop(self.slot.take());
            self.exited = true;
            return Ok(Frame::Exit);
        }
        // A close waiting for an existing save must let that modal resolve its
        // Retry/Return choice. The save scope consumes permission before any
        // underlying menu/VM; shutdown's own writer still freezes the scene.
        let scene_permission = permission
            && (!self.shutdown.active() || (self.shutdown.requested() && self.has_save_task()));
        let composition = self
            .composition
            .get_or_insert_with(|| Composition::new(context));
        let mut native = PreRenderContext {
            device: context.device,
            queue: context.queue,
            breakup_texture: context.breakup_texture,
            resize_source: &composition.resize,
            sampler_store: context.sampler_store,
            depth_stencil: DepthStencilTarget {
                view: &composition.depth_view,
            },
            pipeline_storage: context.pipeline_storage,
            dynamic_buffer: context.dynamic_buffer,
            encoder: context.encoder,
        };
        let mut status = None;
        if close == shutdown::Frame::Continue {
            if self.bootstrap.is_none() {
                if let Some(slot) = &mut self.slot {
                    if let Some(transfer) =
                        slot.take_title_demo_transition(&mut native, &composition.color)?
                    {
                        // TitleDemo itself has no previous-scene overlay field.
                        // Its title source is already retired at this boundary.
                        drop(transfer.capture);
                        self.bootstrap =
                            Some(SceneHost::from_demo(transfer.globals, transfer.request));
                    }
                }
            }
            if let Some(bootstrap) = &mut self.bootstrap {
                let transition = bootstrap.update_with_permission(
                    self.frame_id,
                    elapsed_ticks,
                    scene_permission,
                    buttons,
                    &self.pointer.frame(),
                    native.queue,
                )?;
                if let Some(transition) = transition {
                    let capture = Some(
                        composition
                            .color
                            .copy_snapshot(&mut native.render_clone_ctx()),
                    );
                    drop(self.bootstrap.take());
                    if let Some(slot) = &mut self.slot {
                        slot.install_title(transition, capture)?;
                    } else {
                        self.slot = Some(Box::new(TitleSlot::new(transition, capture)));
                    }
                    // The transferred bootstrap returned before its clock tail.
                    // update_frame alone updates the new title and that tail.
                    self.slot.as_mut().unwrap().set_analog_input(analog);
                    self.slot
                        .as_mut()
                        .unwrap()
                        .set_temporary_skip(self.input.temporary_skip_held());
                    self.slot
                        .as_mut()
                        .unwrap()
                        .set_desktop_mouse_release(self.pointer.mouse_release());
                    status = self
                        .slot
                        .as_mut()
                        .unwrap()
                        .update_frame(
                            &mut native,
                            &composition.color,
                            TOUCH_EXTENT,
                            elapsed_ticks,
                            scene_permission,
                            buttons,
                            &self.pointer.frame(),
                            policy,
                            audio_policy,
                        )
                        .await?;
                }
            } else if let Some(slot) = &mut self.slot {
                slot.set_analog_input(analog);
                slot.set_temporary_skip(self.input.temporary_skip_held());
                slot.set_desktop_mouse_release(self.pointer.mouse_release());
                status = slot
                    .update_frame(
                        &mut native,
                        &composition.color,
                        TOUCH_EXTENT,
                        elapsed_ticks,
                        scene_permission,
                        buttons,
                        &self.pointer.frame(),
                        policy,
                        audio_policy,
                    )
                    .await?;
            }
        }
        if close == shutdown::Frame::Continue {
            if let Some(bootstrap) = &mut self.bootstrap {
                bootstrap.prepare(&mut native);
            } else if let Some(slot) = &mut self.slot {
                slot.prepare(&mut native)?;
            }
        }
        self.shutdown.prepare(&mut native);
        // Native271d0 clamps the callback delta, then27210 updates TouchEffect
        // after the active scene. Config's e7aa4 setter has already published
        // platform_flag here; Higurashi binds it to common16 (Umineko uses17).
        let globals = if let Some(bootstrap) = &self.bootstrap {
            bootstrap.globals().map(ActiveGlobals::from)
        } else {
            self.slot
                .as_ref()
                .map(|slot| slot.active_globals())
                .transpose()?
        };
        let touch_enabled = globals.as_ref().is_some_and(|g| g.platform_flag);
        let touch_art = globals.map(|g| g.common.system.clone());
        self.touch_effect.update_with_pointer_frame(
            delta,
            touch_enabled && permission && close == shutdown::Frame::Continue,
            &self.pointer.frame(),
            TOUCH_EXTENT,
        );
        {
            let mut pass = native.begin_pass(
                composition.color.as_texture_target(),
                Some(DepthStencilTarget {
                    view: &composition.depth_view,
                }),
                "Higurashi whole platform frame",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
            if close == shutdown::Frame::Consumed {
                if let Some(base) = &self.closing_base {
                    Self::render_texture(base, &mut pass);
                }
            } else if let Some(bootstrap) = &mut self.bootstrap {
                bootstrap.render(&mut pass)?;
            } else if let Some(slot) = &mut self.slot {
                slot.render(&mut pass)?;
            }
        }
        if self.touch_effect.has_ripples()
            || self.touch_effect.has_stars()
            || self.shutdown.active()
        {
            let presentation = composition.presentation.get_or_insert_with(|| {
                RenderTexture::new(
                    native.device.clone(),
                    composition.resize.handle(),
                    "Higurashi touch feedback presentation".into(),
                )
            });
            let mut pass = native.begin_pass(
                presentation.as_texture_target(),
                Some(DepthStencilTarget {
                    view: &composition.depth_view,
                }),
                "Higurashi global touch feedback",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
            Self::render_texture(&composition.color, &mut pass);
            self.touch_effect
                .render_ripples(&mut pass, composition.color.as_texture_source());
            if let Some(art) = touch_art {
                self.touch_effect.render(&mut pass, &art);
            }
            self.shutdown.render(&mut pass);
        } else {
            composition.presentation = None;
        }
        self.frames += 1;
        let scene_flush = self
            .slot
            .as_mut()
            .is_some_and(|slot| slot.take_input_flush());
        let shutdown_flush = self.shutdown.take_input_flush();
        if scene_flush || shutdown_flush {
            self.flush_sampled_input();
        }
        if !self.shutdown.active() {
            self.closing_base.take();
        }
        if tracing::enabled!(tracing::Level::DEBUG) {
            let phase = self.touch_diagnostic_phase();
            let save_selection = self.touch_diagnostic_save_selection();
            if phase != self.last_diagnostic_phase
                || save_selection != self.last_diagnostic_save_selection
            {
                self.last_diagnostic_phase = phase;
                self.last_diagnostic_save_selection = save_selection;
                if let Some((selected, valid, row_fnv, remembered)) =
                    self.touch_diagnostic_save_slot()
                {
                    tracing::debug!(selected, valid, row_fnv, remembered, "HIGURASHI_SAVE_SLOT");
                }
                if let Some((pc, completed)) = self.touch_diagnostic_story_position() {
                    tracing::debug!(
                        phase = phase.unwrap_or("transition"),
                        pc,
                        completed,
                        "HIGURASHI_TOUCH_READY"
                    );
                } else {
                    tracing::debug!(
                        phase = phase.unwrap_or("transition"),
                        "HIGURASHI_TOUCH_READY"
                    );
                }
            }
        }
        Ok(if close == shutdown::Frame::Consumed {
            Frame::Closing
        } else {
            Frame::Advanced(status)
        })
    }
    /// Pure presentation of the last completed platform callback. Repeated
    /// draws cannot re-prepare ADV children or advance any scene/cache clocks.
    pub fn render(&self, pass: &mut RenderPass) {
        let Some(composition) = &self.composition else {
            return;
        };
        Self::render_texture(composition.displayed(), pass);
    }
    fn render_texture(texture: &RenderTexture, pass: &mut RenderPass) {
        let vertices = [(0., 0.), (1., 0.), (0., 1.), (1., 1.)].map(|(x, y)| PosTexVertex {
            position: Vec2::new(x, y),
            texture_position: Vec2::new(x, y),
        });
        // A completed platform frame is copied unconditionally. The window
        // supplies a zero-cleared stencil attachment, so the native layer
        // shorthand (reference 0 > stencil 0) would reject the whole image.
        pass.run(RenderRequestBuilder::new().build(
            RenderProgramWithArguments::Layer {
                output_kind: LayerShaderOutputKind::Layer,
                fragment_shader: LayerFragmentShader::Default,
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture: texture.as_texture_source(),
                transform: crate::render::normalized_projection_matrix(),
                color_multiplier: FloatColor4::WHITE,
                fragment_shader_param: Vec4::ZERO,
            },
            DrawPrimitive::TrianglesStrip,
        ));
    }
    #[cfg(test)]
    pub fn capture_composed(&self, context: &mut PreRenderContext) -> Option<RenderTexture> {
        self.composition.as_ref().map(|frame| {
            frame
                .displayed()
                .copy_snapshot(&mut context.render_clone_ctx())
        })
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub fn slot(&self) -> Option<&TitleSlot> {
        self.slot.as_deref().filter(|_| self.bootstrap.is_none())
    }
    #[cfg(feature = "higurashi-window-probe")]
    pub(super) fn probe_snapshot(&self) -> Result<super::window_probe::Snapshot> {
        use shin_core::vm::higurashi::command::HigurashiRuntimeCommand as Cmd;
        let globals = self.globals()?;
        let game = self.slot().and_then(|slot| slot.game.as_ref());
        let runner = game.and_then(|game| game.host()).map(|host| &host.runner);
        let chart = self
            .slot()
            .and_then(|slot| slot.title.as_ref())
            .and_then(|title| title.reference_chart_owner())
            .map(|owner| (0, owner))
            .or_else(|| {
                game.and_then(|game| game.reference_chart_owner())
                    .map(|owner| (1, owner))
            });
        let cg = self
            .slot()
            .and_then(|slot| slot.cg_gallery.as_ref())
            .map(|cg| {
                use super::window_probe::cg_gallery::State;
                let owner = cg.reference_owner();
                let model = owner.reference_model();
                let (atlas, first_complete, input, token) = owner.reference_readiness();
                let selected = model.page * 12 + model.cell();
                let entry = model.entries.get(selected);
                State {
                    atlas,
                    atlas_identity: token.unwrap_or(0),
                    first_complete,
                    input,
                    closing: model.closing,
                    frame: model.frame,
                    page: model.page,
                    pages: model.pages,
                    selected,
                    pictures: entry.map_or_else(Vec::new, |entry| entry.pictures.clone()),
                    unlocked: entry.map_or_else(Vec::new, |entry| entry.unlocked.clone()),
                    viewer: owner
                        .reference_viewer()
                        .map(|viewer| viewer.reference_window_state()),
                }
            });
        let music = self
            .slot()
            .and_then(|slot| slot.music_gallery.as_ref())
            .map(|music| music.reference_window_state());
        let movie = self
            .slot()
            .and_then(|slot| slot.movie_gallery.as_ref())
            .map(|movie| {
                use super::window_probe::movie_gallery::State;
                let owner = movie.reference_owner();
                let model = owner.reference_model();
                let selected = model.selected();
                let row = model
                    .rows
                    .get(selected)
                    .expect("Movie selected resident row");
                let (atlas, mask, first_complete, input, _, _) = owner.reference_state();
                State {
                    atlas,
                    mask,
                    first_complete,
                    input,
                    closing: model.closing,
                    selected,
                    frame: model.frame,
                    rows: model.rows.len(),
                    scroll_bits: model.scroll.value().to_bits(),
                    scroll_busy: !model.scroll.is_idle(),
                    close_bits: model.close_flash.value().to_bits(),
                    selected_movie: row.movie_id,
                    selected_locked: row.locked,
                    scope: owner.reference_scope_state(),
                    viewer: owner.reference_viewer_state(),
                    cues: movie.reference_cues(),
                }
            });
        Ok(super::window_probe::Snapshot {
            frames: self.frames,
            clock: globals.as_ref().map(|globals| globals.clock.ticks()),
            notice: self
                .bootstrap
                .as_ref()
                .and_then(SceneHost::reference_notice),
            demo_ready: self
                .bootstrap
                .as_ref()
                .is_some_and(SceneHost::reference_demo_ready),
            menu: self
                .slot()
                .and_then(|slot| slot.title.as_ref())
                .and_then(|title| title.reference_menu_state()),
            top_selected: self
                .slot()
                .and_then(|slot| slot.title.as_ref())
                .and_then(|title| title.reference_top_selection()),
            config: self
                .slot()
                .and_then(|slot| slot.title.as_ref())
                .and_then(|title| title.reference_config())
                .or_else(|| game.and_then(|game| game.reference_config())),
            config_question: self
                .slot()
                .and_then(|slot| slot.title.as_ref())
                .and_then(|title| title.reference_config_question())
                .or_else(|| game.and_then(|game| game.reference_config_question())),
            manual: self
                .slot()
                .and_then(|slot| slot.title.as_ref())
                .and_then(|title| title.reference_manual())
                .or_else(|| game.and_then(|game| game.reference_manual())),
            chart: chart.map(|(source, owner)| {
                let (atlas, completed, input, progress, group, selected) = owner.reference_state();
                (source, atlas, completed, input, progress, group, selected)
            }),
            chart_cursor: chart.map(|(_, owner)| owner.reference_cursor()),
            cg,
            music,
            movie,
            category: self
                .slot()
                .and_then(|slot| slot.title.as_ref())
                .and_then(|title| title.reference_category()),
            title_initial: self
                .slot()
                .and_then(|slot| slot.title.as_ref())
                .map(|title| title.reference_initial_selection()),
            picker: self
                .slot()
                .and_then(|slot| slot.scenario_select.as_ref())
                .map(|picker| {
                    let owner = picker.reference_owner();
                    let model = owner.reference_model();
                    (
                        model.kind,
                        model.selected,
                        owner.reference_state().2,
                        model.contact,
                    )
                }),
            picker_close_idle: self
                .slot()
                .and_then(|slot| slot.scenario_select.as_ref())
                .map(|picker| {
                    picker
                        .reference_owner()
                        .reference_model()
                        .close_flash
                        .is_idle()
                }),
            tips: self.slot().is_some_and(|slot| slot.tips.is_some()),
            system_menu: game
                .and_then(|game| game.host())
                .and_then(|host| host.reference_menu_control()),
            input_diagnostics: game
                .and_then(|game| game.host())
                .map(|host| host.reference_input_diagnostics()),
            commands: runner.map_or(0, |runner| runner.completed_commands()),
            message: runner.is_some_and(|runner| {
                matches!(
                    runner.pending_command(),
                    Some(Cmd::MSGWAIT { .. } | Cmd::MSGSET { auto_wait: 1, .. })
                )
            }),
            buttons: self.input.buttons(),
            closing: self.close_active(),
            frozen: self.freezes_scene(),
            exited: self.exited,
        })
    }
}

mod touch_diagnostics;

#[cfg(test)]
mod tests;
