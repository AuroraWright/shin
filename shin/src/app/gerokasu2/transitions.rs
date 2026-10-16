//! Publication and presentation shared by the window adapter and offscreen
//! journeys. Fallible preparation keeps the outgoing scene and its audio live.
use super::*;
use crate::adv::gerokasu2::exit_mode::ExitAction;
use shin_core::format::save::gerokasu2::GameData;

impl Gerokasu2App {
    pub(super) fn poll_title_destination(&mut self) {
        let Some(result) = self
            .loading_title
            .as_mut()
            .and_then(|task| task.poll_naive())
        else {
            return;
        };
        self.loading_title = None;
        match result {
            Ok(mut title) => {
                if self.host.is_some() {
                    self.retire_story_effects();
                }
                self.reset_system_for_publication();
                title.cancel_input();
                self.title = Some(title);
                self.first_boot = None;
                self.title_demo = None;
                self.gallery = None;
                self.title_child = None;
                self.host = None;
                self.adv_frame = None;
            }
            Err(error) => {
                tracing::error!(%error,"Ger Title preparation failed; retaining source");
                if let Some(gallery) = self.gallery.as_mut() {
                    gallery.recover_return();
                }
                if self.system_title_exit {
                    // Reversible desktop destination failure. The old ADV,
                    // counted scope, backdrop and paused sources still belong
                    // to this same SystemMenu; do not retire or resume them.
                    self.system_title_exit = false;
                    if let Some(system) = self.system.as_mut() {
                        system.recover_child();
                    }
                } else if self.title_demo.is_some() || self.first_boot.is_some() {
                    self.error = Some(format!("Ger boot Title preparation failed: {error:#}"));
                } else if self.host.is_some() {
                    self.error = Some(format!("Ger EXIT Title preparation failed: {error:#}"));
                }
            }
        }
    }
    pub(super) fn poll_game_destination(&mut self) {
        let Some(result) = self
            .loading_game
            .as_mut()
            .and_then(|task| task.poll_naive())
        else {
            return;
        };
        self.loading_game = None;
        let result = result.and_then(|mut host| {
            host.scene.check_audio_publication()?;
            if self.host.is_some() {
                self.retire_story_effects();
            }
            host.scene
                .publish_global_audio(self.menu_bgm.lock().shared_audio())?;
            Ok(host)
        });
        match result {
            Ok(mut host) => {
                self.reset_system_for_publication();
                host.suspended = self.suspended;
                host.cancel_input();
                self.host = Some(host);
                self.title_child = None;
                self.title = None;
            }
            Err(error) => {
                tracing::error!(%error,"Ger ADV preparation failed; retaining source");
                if let Some(backlog) = self.backlog.as_mut() {
                    backlog.recover_failed_jump();
                } else if let Some(child) = self.title_child.as_mut() {
                    child.load_failed();
                } else if let Some(title) = self.title.as_mut() {
                    title.model.recover_handoff();
                    title.cancel_input();
                }
            }
        }
    }
    pub(super) fn complete_exit_save(&mut self, pre: &mut PreRenderContext) -> Result<bool> {
        if !self.exit_save_ready {
            return Ok(false);
        }
        self.exit_save_ready = false;
        let actions = if self.system_title_exit {
            vec![
                ExitAction::RestoreBgmFocus,
                ExitAction::ReturnTitle { requested_id: -1 },
            ]
        } else {
            self.host
                .as_mut()
                .context("Ger EXIT has no source ADV")?
                .scene
                .exit_save_completed()
        };
        for action in actions {
            match action {
                ExitAction::RestoreBgmFocus => self.menu_bgm.lock().restore_voice_focus_target(),
                ExitAction::ReturnTitle { requested_id } => {
                    let source = self
                        .adv_frame
                        .as_ref()
                        .context("Ger EXIT has no composed outgoing frame")?;
                    let snapshot = source.copy_snapshot(&mut pre.render_clone_ctx());
                    self.start_gallery_title(requested_id, snapshot);
                }
                ExitAction::PrepareExit | ExitAction::RequestSave => {}
            }
        }
        Ok(true)
    }
    pub(super) fn begin_exit_write(&mut self) -> Result<()> {
        if self.waiting_for_exit_save || self.exit_save_ready {
            return Ok(());
        }
        let host = self.host.as_ref().context("Ger EXIT has no live ADV")?;
        if let Some(store) = self.save_store.as_mut() {
            host.scene.export_save_globals(&mut store.data);
            if let Some(auto) = GameData::from_memory(&host.scene.save_session().autosave) {
                store.data.auto_save_slot = Some(auto);
            }
            store.mark_dirty();
        }
        self.waiting_for_exit_save = true;
        self.begin_write_wait(write_wait::Purpose::Exit)
    }
    /// Export live globals without adding file writes to ordinary memory
    /// captures, and publish scene-title/BGM notifications.
    pub(super) fn collect_scene_progress(&mut self) {
        let Some(host) = self.host.as_mut() else {
            return;
        };
        if let Some(store) = self.save_store.as_mut() {
            host.scene.export_save_globals(&mut store.data);
            store.mark_dirty();
        }
        for event in host.scene.take_events() {
            match event {
                SceneEvent::SceneTitle(title) => host.show_scene_title(&title),
                SceneEvent::BgmChanged(id) => host.show_bgm_title(id),
                SceneEvent::AutosaveCaptured(memory) => {
                    if let Some(store) = self.save_store.as_mut() {
                        store.data.auto_save_slot = GameData::from_memory(&memory);
                        host.scene.export_save_globals(&mut store.data);
                        store.mark_dirty();
                    }
                }
                _ => {}
            }
        }
    }
    pub(super) fn compose_adv_frame(&mut self, pre: &mut PreRenderContext) -> Result<()> {
        let Some(host) = self.host.as_mut() else {
            return Ok(());
        };
        if self.modal.pause_depth == 0 {
            host.pre_render(pre)?;
        }
        if let Some(system) = self.system.as_mut() {
            system.prepare(pre)?;
        }
        if let Some(child) = self.title_child.as_mut() {
            child.prepare(pre)?;
        }
        if let Some(backlog) = self.backlog.as_mut() {
            backlog.prepare(pre)?;
        }
        let source = self
            .adv_frame
            .get_or_insert_with(|| pre.new_render_texture("Ger composed ADV frame".into()));
        let mut pass = pre.begin_pass(
            source.as_texture_target(),
            Some(pre.depth_stencil),
            "Ger composed ADV frame",
        );
        let draw_root = crate::adv::gerokasu2::modal::backdrop::backdrop_plan(
            self.backdrop.is_some(),
            self.modal.fade.value(),
        )
        .render_root;
        if draw_root {
            host.render(&mut pass)?;
        } else {
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.),
            );
        }
        if let Some(backdrop) = self.backdrop.as_ref() {
            backdrop.render(&mut pass, self.modal.fade.value());
        }
        // Native11E58..11E64 draws both notifications after the root and
        // modal dim, below the retained modal children.
        // 11E68..12618: indicators, navi toolbar and event hint follow.
        if let Some(store) = self.save_store.as_ref() {
            host.render_notifications(&mut pass, &store.data.settings)?;
            host.render_root_ui(&mut pass, &store.data.settings);
        }
        host.render_chapter_question(&mut pass)?;
        if let Some(system) = self.system.as_ref() {
            system.render(&mut pass)?;
        }
        if let Some(child) = self.title_child.as_ref() {
            child.render(&mut pass)?;
        }
        if let Some(backlog) = self.backlog.as_ref() {
            backlog.render(&mut pass)?;
        }
        Ok(())
    }
    /// Native TouchEffect updates after the active owner, with the current
    /// Config value; ripples refract a capture of the presented frame.
    pub(super) fn prepare_touch_frame(
        &mut self,
        pre: &mut PreRenderContext,
        delta: Ticks,
        held: bool,
        window: winit::dpi::PhysicalSize<u32>,
    ) {
        use shin_window::ShinApp;
        let viewport = Self::map_canvas_size(window);
        let map = Viewport {
            offset: Vec2::new(
                viewport.canvas_offset.x as f32,
                viewport.canvas_offset.y as f32,
            ),
            size: Vec2::new(
                viewport.canvas_size.width as f32,
                viewport.canvas_size.height as f32,
            ),
        };
        // 252C8 constructs TouchEffect disabled (E0324(0)); only lambda
        // 5D0FC, after FirstBoot, and Config's setter 563C8 enable it.
        let enabled = !self.suspended
            && self.error.is_none()
            && self.first_boot.is_none()
            && self
                .save_store
                .as_ref()
                .is_some_and(|store| store.data.settings.v17_showscenetitle);
        self.touch_effect.update_in_viewport(
            delta,
            enabled,
            held,
            map.map(self.point).unwrap_or(Vec2::splat(-1.)),
            (
                viewport.canvas_size.width.clamp(1, u16::MAX as u32) as u16,
                viewport.canvas_size.height.clamp(1, u16::MAX as u32) as u16,
            ),
        );
        if !self.touch_effect.has_ripples() {
            self.touch_background = None;
            return;
        }
        let background = self
            .touch_background
            .get_or_insert_with(|| pre.new_render_texture("Ger touch ripple capture".into()));
        let mut pass = pre.begin_pass(
            background.as_texture_target(),
            Some(pre.depth_stencil),
            "Ger touch ripple capture",
        );
        if let Err(error) = self.present_frame(&mut pass) {
            tracing::error!(%error, "Ger touch ripple capture failed");
        }
    }
    pub(super) fn render_touch_frame(&self, pass: &mut RenderPass) -> Result<()> {
        if let Some(background) = self.touch_background.as_ref() {
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.),
            );
            shin_render::quad_vertices::QuadVertices::new()
                .with_box(0., 0., 1920., 1080.)
                .render_sprite(
                    pass,
                    shin_render::RenderRequestBuilder::new(),
                    background.as_texture_source(),
                    crate::render::top_left_projection_matrix(),
                );
            self.touch_effect
                .render_ripples(pass, background.as_texture_source());
        } else {
            self.present_frame(pass)?;
        }
        if let Some(textures) = self.system_textures.as_ref() {
            self.touch_effect.render(pass, textures);
        }
        Ok(())
    }
    pub(super) fn present_frame(&self, pass: &mut RenderPass) -> Result<()> {
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.),
        );
        if let Some(boot) = self.first_boot.as_ref() {
            boot.render(pass);
        } else if let Some(demo) = self.title_demo.as_ref() {
            if let Some(store) = self.save_store.as_ref() {
                demo.render(pass, &store.data);
            }
        } else if let Some(title) = self.title.as_ref() {
            title.render(pass);
            if let Some(child) = self.title_child.as_ref() {
                child.render(pass)?;
            }
        } else if let Some(gallery) = self.gallery.as_ref() {
            gallery.render(pass)?;
        } else if let Some(host) = self.host.as_ref() {
            if let Some(source) = self.adv_frame.as_ref() {
                crate::adv::scene_load_fade::render_snapshot(source, pass);
            } else {
                host.render(pass)?;
            }
        }
        if let Some(wait) = self.write_wait.as_ref() {
            wait.render(pass)?;
        }
        Ok(())
    }
    pub(super) fn advance_global_audio(&mut self, delta: Ticks) {
        let mut music = self.menu_bgm.lock();
        if let Some(active) = self.backlog.as_ref().and_then(|b| b.voice_focus()) {
            let enabled = self
                .save_store
                .as_ref()
                .is_some_and(|s| s.data.settings.v4_voicefocus);
            music.set_voice_focus_target(enabled, active);
        } else if let Some(host) = self.host.as_ref()
            && self.modal.pause_depth == 0
            && !self.waiting_for_exit_save
            && self.loading_title.is_none()
            && host.scene.focus_policy_ready()
        {
            let enabled = self
                .save_store
                .as_ref()
                .is_some_and(|s| s.data.settings.v4_voicefocus);
            let active = host
                .scene
                .runner()
                .voice()
                .wait_status()
                .contains(shin_core::vm::command::types::AudioWaitStatus::PLAYING);
            music.set_voice_focus_target(enabled, active);
        }
        music.update(delta);
        drop(music);
        self.commit_menu_unlocks();
    }
}
