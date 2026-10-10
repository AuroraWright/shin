//! Native title resource worker113310 and background layer-group owner.
use super::{
    TitleItem, TitleSetup, TitleStage,
    captured_scene::CapturedScene,
    child::{LoadResources, TitleChild},
    input::TitleInput,
    motion::TitleMotion,
    view,
};
use crate::{
    adv::{character_view::CharacterTextures, input_area::PointerFrame},
    app::AppAction,
    asset::{mask::MaskTexture, picture::Picture, system::AssetServer},
    layer::{
        DrawableLayer, Layer, LayerGroup, render_layer_without_bg, render_params::TransformParams,
        user::PictureLayer,
    },
    render::PreRenderContext,
};
use anyhow::Result;
use enum_map::EnumMap;
use shin_core::{
    primitives::color::UnormColor,
    time::Ticks,
    vm::command::types::{LayerProperty, LayerbankId},
};
use shin_input::ActionState;
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::{Arc, mpsc};

struct BackgroundAssets {
    mask: Arc<MaskTexture>,
    picture: Arc<Picture>,
}
struct ReturnCapture {
    frame: CapturedScene,
    elapsed: f32,
    duration: f32,
}
impl ReturnCapture {
    fn opacity(&self) -> f32 {
        1.0 - self.elapsed / self.duration
    }
}
pub(crate) struct TitleScene {
    setup: TitleSetup,
    pub motion: TitleMotion,
    input: TitleInput,
    atlas: Option<Arc<CharacterTextures>>,
    atlas_received: mpsc::Receiver<Arc<CharacterTextures>>,
    loading: Option<shin_tasks::AsyncTask<Result<BackgroundAssets>>>,
    // Native worker retains default.msk separately from the background group.
    // It is not a layer mask: title10c238 binds the generated screen target.
    default_mask: Option<Arc<MaskTexture>>,
    background: Option<LayerGroup<PictureLayer>>,
    rendered_background: Option<RenderTexture>,
    stage_music: Option<TitleStage>,
    server: Arc<AssetServer>,
    children: Vec<TitleChild>,
    load_resources: Option<LoadResources>,
    load_entry: Option<shin_core::format::save::GameData>,
    extent: (u16, u16),
    return_capture: Option<ReturnCapture>,
}
impl TitleScene {
    pub(crate) fn suspend_shutdown_input(&mut self, held: bool) {
        self.input.suspend(&mut self.motion);
        for child in &mut self.children {
            child.suspend_pointer(held);
        }
    }
    pub fn new(server: Arc<AssetServer>, setup: TitleSetup) -> Self {
        let (send, receive) = mpsc::channel();
        let path = setup.background_path();
        // Publish atlas before the worker completes mask/picture loading,
        // matching113380. The native renderer can show UI during loading.
        let assets = server.clone();
        let loading = shin_tasks::async_io::spawn(async move {
            let atlas = assets.load::<CharacterTextures, _>("/title.txa").await?;
            let _ = send.send(atlas);
            let mask = assets.load::<MaskTexture, _>("/default.msk").await?;
            let picture = assets.load::<Picture, _>(&path).await?;
            Ok(BackgroundAssets { mask, picture })
        });
        Self {
            motion: TitleMotion::new(setup.clone()),
            setup,
            input: TitleInput::new(),
            atlas: None,
            atlas_received: receive,
            loading: Some(loading),
            default_mask: None,
            background: None,
            rendered_background: None,
            stage_music: None,
            server,
            children: Vec::new(),
            load_resources: None,
            load_entry: None,
            extent: (1920, 1080),
            return_capture: None,
        }
    }
    pub fn with_return_capture(mut self, frame: RenderTexture) -> Self {
        self.return_capture = Some(ReturnCapture {
            frame: CapturedScene::new(frame),
            elapsed: 0.0,
            duration: if self.setup.returning_from_child {
                60.0
            } else {
                90.0
            },
        });
        self
    }
    pub fn is_loaded(&self) -> bool {
        self.background.is_some()
    }
    pub(crate) fn has_save_task(&self) -> bool {
        self.children
            .iter()
            .any(|child| matches!(child, TitleChild::Config { save: Some(_), .. }))
    }
    #[cfg(test)]
    pub(crate) fn reference_begin_config_save(&mut self, globals: &mut crate::adv::GlobalData) {
        let mut child =
            TitleChild::new(TitleItem::Config, self.server.clone(), globals, None, 0).unwrap();
        let mut number = 0;
        let mut step = |child: &mut TitleChild,
                        globals: &mut crate::adv::GlobalData,
                        delta: f32,
                        action: Option<AppAction>| {
            let mut input = EnumMap::default();
            if let Some(action) = action {
                input[action] = ActionState {
                    is_clicked: true,
                    is_clicked_or_repeated: true,
                    ..Default::default()
                };
            }
            let frame = PointerFrame {
                number,
                time_seconds: number as f64 / 60.0,
                events: &[],
            };
            number += 1;
            child.input(
                &frame,
                &input,
                glam::Vec2::ZERO,
                false,
                0.0,
                0.0,
                (1920, 1080),
            );
            assert!(!child.update(Ticks::from_f32(delta), globals).unwrap());
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while child.config().and_then(|screen| screen.model()).is_none() {
            step(&mut child, globals, 0.0, None);
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        step(&mut child, globals, 10.0, None);
        step(&mut child, globals, 0.0, Some(AppAction::Right));
        step(&mut child, globals, 7.0, Some(AppAction::Cancel));
        step(&mut child, globals, 0.0, None);
        assert_eq!(
            child.config().unwrap().model().unwrap().phase(),
            super::super::adv::config::ConfigPhase::WaitingSave
        );
        self.children.push(child);
        assert!(self.has_save_task());
    }
    #[cfg(test)]
    pub(crate) fn reference_save_notice_open(&self) -> bool {
        self.children.iter().any(|child| matches!(child, TitleChild::Config { save:Some(save), .. } if save.reference_failure_is_open()))
    }
    #[cfg(test)]
    pub fn update_frame(
        &mut self,
        delta: Ticks,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
    ) -> Result<()> {
        self.update_with_globals(
            delta,
            frame,
            state,
            &mut crate::adv::GlobalData::default(),
            glam::Vec2::ZERO,
            false,
            0.0,
            0.0,
        )
    }
    pub fn update_with_globals(
        &mut self,
        delta: Ticks,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        globals: &mut crate::adv::GlobalData,
        pointer: glam::Vec2,
        held: bool,
        elapsed: f64,
        axis: f32,
    ) -> Result<()> {
        if let Ok(atlas) = self.atlas_received.try_recv() {
            self.atlas = Some(atlas);
        }
        if let Some(assets) = self.loading.as_mut().and_then(|task| task.poll_naive()) {
            self.loading = None;
            let assets = assets?;
            // Final task publication can race the channel poll above. Install
            // the already-published atlas before enabling background/title.
            if self.atlas.is_none() {
                self.atlas = self.atlas_received.try_recv().ok();
            }
            let mut group = LayerGroup::new(Some("Title background".into()));
            group.add_layer(
                LayerbankId::new(0),
                PictureLayer::new(assets.picture, Some(self.setup.background_path())),
            );
            self.default_mask = Some(assets.mask);
            self.background = Some(group);
            self.motion.assets_ready();
            self.stage_music = Some(*self.setup.stage());
        }
        // Native1000b0 snapshots tasks and polls newest first. A child inserted
        // by a confirmation is rendered now but first updates next frame.
        self.children.retain(|manual| !manual.is_finished());
        let had_child = !self.children.is_empty();
        let mut finished = false;
        let mut child_input = true;
        for index in (0..self.children.len()).rev() {
            let child = &mut self.children[index];
            if child_input {
                child.input(frame, state, pointer, held, elapsed, axis, self.extent);
            }
            if child.update(delta, globals)? {
                finished = true;
                self.motion.child_finished(false);
            }
            if let Some(entry) = child.take_load_game_data() {
                self.load_entry = Some(entry);
            }
            for request in child.take_parent_requests() {
                match request {
                    super::episode_chooser_scene::ParentRequest::Dim(value) => {
                        self.motion.viewer_dim(value)
                    }
                    super::episode_chooser_scene::ParentRequest::RestoreRows => {
                        self.motion.open_rows(true)
                    }
                }
            }
            child_input = false; // Manual859d0, Config5b59c and SaveMenu d70c0 return false to older tasks.
        }
        if finished && self.children.iter().all(TitleChild::is_finished) {
            self.motion.child_finished(true);
        }
        self.motion.prepare_frame();
        for item in self.motion.take_child_requests() {
            let mut child = TitleChild::new(
                item,
                self.server.clone(),
                globals,
                self.load_resources.as_ref(),
                self.setup.completed_episodes,
            )?;
            child.resize(self.extent);
            self.children.push(child);
        }
        if !had_child && self.children.is_empty() {
            self.input.input_frame(&mut self.motion, frame, state);
        } else {
            self.input.suspend(&mut self.motion);
        }
        self.motion.advance_clocks(delta);
        // Native110a10 holds the outgoing frame until Title resources publish,
        // then62ea0/62f40 fades it for60ticks on submenu return,90ticks on
        // scenario return. Retirement polls before addition.
        if self.loading.is_none() {
            if let Some(capture) = &mut self.return_capture {
                if capture.elapsed >= capture.duration {
                    self.return_capture = None;
                } else {
                    capture.elapsed = (capture.elapsed + delta.as_f32()).min(capture.duration);
                }
            }
        }
        Ok(())
    }
    pub fn configure_load(
        &mut self,
        scenario: Arc<shin_core::format::scenario::Scenario>,
        audio: Arc<shin_audio::AudioManager>,
        canvas: (u32, u32),
    ) {
        self.load_resources = Some(LoadResources {
            scenario,
            audio,
            canvas,
        });
    }
    pub fn take_load_game_data(&mut self) -> Option<shin_core::format::save::GameData> {
        self.load_entry.take()
    }
    pub fn resize(&mut self, extent: (u16, u16)) {
        self.input.resize(&mut self.motion, extent);
        self.extent = extent;
        for child in &mut self.children {
            child.resize(extent);
        }
    }
    /// Native10d788..10d7c0 starts the authored stage BGM with60tick transition.
    pub fn take_stage_music(&mut self) -> Option<TitleStage> {
        self.stage_music.take()
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        let mut sounds = self.motion.take_sounds();
        for child in &mut self.children {
            sounds.extend(child.sounds());
        }
        sounds
    }
    pub fn take_destination(&mut self) -> Option<TitleItem> {
        self.motion.take_destination()
    }
    /// Use the host's global music/cue players; never allocate scene-local buses.
    pub fn update_audio(
        &mut self,
        server: &Arc<AssetServer>,
        scenario: &shin_core::format::scenario::Scenario,
        settings: &shin_core::format::save::Settings,
        bgm: &mut crate::audio::BgmPlayer,
        system: &mut crate::audio::SystemSoundPlayer,
        delta: Ticks,
    ) {
        use shin_core::{
            format::scenario::instruction_elements::FromNumber, time::Tween,
            vm::command::types::Volume,
        };
        bgm.set_settings_volume(settings.v0_bgmvol);
        system.set_settings_volume(settings.v3_sysvol);
        if let Some(stage) = self.take_stage_music() {
            let id = shin_core::format::scenario::info::BgmId::from_number(stage.bgm as i32);
            bgm.request(
                server.clone(),
                scenario.info_tables().bgm_info(id).path(),
                id,
                true,
                Volume(stage.bgm_volume as f32 * 0.01),
                Tween::linear(Ticks::from_f32(60.0)),
            );
        }
        for duration in self.motion.take_music_stops() {
            bgm.stop(Tween::linear(duration));
        }
        for name in self.take_sounds() {
            system.play(server, name, Volume::default());
        }
        // Factory337e0 restores target gain1. Subsequent global33ec0 updates
        // ramp the retained music player back from ADV voice attenuation.
        bgm.update_voice_focus(delta, false, false);
        bgm.update();
        system.update();
    }
    pub fn prepare(&mut self, context: &mut PreRenderContext) {
        for child in &mut self.children {
            child.prepare(context);
        }
        if self.rendered_background.is_none() {
            self.rendered_background =
                Some(context.new_render_texture("Title rendered background".into()));
        }
        let transform = TransformParams::default();
        if let Some(background) = &mut self.background {
            background
                .properties_mut()
                .property_tweener_mut(LayerProperty::BlurRadius)
                .fast_forward_to(self.motion.background_reveal());
            background.pre_render(context, &transform);
        }
        let target = self.rendered_background.as_mut().unwrap();
        let mut pass = context.begin_pass(
            target.as_texture_target(),
            Some(context.depth_stencil),
            "Title background capture",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        if let Some(background) = &self.background {
            render_layer_without_bg(&mut pass, &transform, background, 0);
        }
    }
    pub fn render(&self, pass: &mut RenderPass) {
        self.render_components(pass, true);
    }
    #[cfg(test)]
    pub(crate) fn reference_render_without_return_capture(&self, pass: &mut RenderPass) {
        self.render_components(pass, false);
    }
    fn render_components(&self, pass: &mut RenderPass, with_capture: bool) {
        if let (Some(atlas), Some(background)) = (&self.atlas, &self.rendered_background) {
            view::render(
                &self.setup,
                &self.motion.frame(),
                atlas,
                background.as_texture_source(),
                pass,
                crate::render::top_left_projection_matrix(),
            );
            //10c6a4 renders task children after Title's black overlay.
            for child in &self.children {
                child.render(atlas, background.as_texture_source(), pass);
            }
        }
        // Keep the outgoing scene visible even before the new atlas arrives.
        if let Some(capture) = self.return_capture.as_ref().filter(|_| with_capture) {
            capture.frame.render(pass, capture.opacity());
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_return_capture(&self) -> Option<f32> {
        self.return_capture.as_ref().map(ReturnCapture::opacity)
    }
    #[cfg(test)]
    pub(super) fn reference_episodes(
        &self,
    ) -> Option<&super::episode_chooser_scene::EpisodeChooserScene> {
        self.children.last().and_then(TitleChild::episodes)
    }
    #[cfg(test)]
    pub(crate) fn reference_load(&self) -> Option<&crate::adv::save_menu_screen::SaveMenuScreen> {
        self.children.last().and_then(TitleChild::load)
    }
    #[cfg(test)]
    pub(crate) fn reference_config(&self) -> Option<&crate::adv::config_screen::ConfigScreen> {
        self.children.last().and_then(TitleChild::config)
    }
    #[cfg(test)]
    pub(crate) fn reference_child_task_count(&self) -> usize {
        self.children.len()
    }
    #[cfg(test)]
    pub(crate) fn reference_manual(&self) -> Option<&crate::adv::manual_screen::ManualScreen> {
        self.children.last().and_then(TitleChild::manual)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_title_resource_worker_and_background_owner() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x113380, 0x94000f80),
            (0x11338c, 0x9102c108),
            (0x113474, 0x97fe3413),
            (0x11347c, 0x910082c8),
            (0x113630, 0x97fec6ac),
            (0x110ab0, 0x2a1f03e1),
            (0x110ab8, 0x97fda70a),
            (0x110b30, 0x52800841),
            (0x110b34, 0xd63f0100),
            (0x110b40, 0x97fea9e8),
            (0x110b58, 0x97feabaa),
            (0x110b60, 0x97fff294),
            (0x10c11c, 0x4ea81d00),
            (0x10c1c4, 0x320003e1),
            (0x10c238, 0x97ff0c26),
            (0x10c24c, 0x97ff106d),
            (0x10c2c0, 0x97fd9388),
            (0x10d7b8, 0x321e0fe3),
            (0x10db90, 0x91032260),
            (0x10db94, 0x97feb623),
            (0x10db9c, 0xbd457101),
            (0x10dba0, 0x1e2c1000),
            (0x10dba4, 0x321a03e1),
            (0x10dbac, 0x97feb795),
            (0x10dbcc, 0x97fde54d),
            (0x10dbd8, 0x97ffc926),
            (0x10c6a8, 0x97ffcf32),
            (0x111cb4, 0x7100051f),
            (0x111cb8, 0x540001a1),
            (0x111ccc, 0x97fea5d5),
            (0x111cd4, 0xbd457101),
            (0x111cdc, 0x321a03e1),
            (0x111ce4, 0x97fea747),
            (0x859f0, 0x9401e9b0),
            (0x85a14, 0x2a1f03e0),
            (0x877d8, 0xb900c27f),
            (0x877f4, 0xd63f0100),
            (0x10dc08, 0x91032260),
            (0x10dc0c, 0x97feb605),
            (0x10dc14, 0xbd457101),
            (0x10dc18, 0x1e2c1000),
            (0x10dc1c, 0x321a03e1),
            (0x10dc24, 0x97feb777),
            (0x10dc44, 0x97fd415f),
            (0x10dc50, 0x97ffc908),
            (0x111bb4, 0x7100051f),
            (0x111bb8, 0x540001a1),
            (0x111bcc, 0x97fea615),
            (0x111bd4, 0xbd457101),
            (0x111bdc, 0x321a03e1),
            (0x111be4, 0x97fea787),
            (0x5b59c, 0x2a1f03e0),
            (0x60bf0, 0xf9400400),
            (0x60bf4, 0x17ffefe3),
            (0x10dac4, 0x91032260),
            (0x10dac8, 0x97feb656),
            (0x10dad0, 0xbd457101),
            (0x10dad4, 0x1e2c1000),
            (0x10dad8, 0x321a03e1),
            (0x10dae0, 0x97feb7c8),
            (0x10db00, 0x97ff331c),
            (0x10db0c, 0x97ffc959),
            (0x111ab4, 0x7100051f),
            (0x111ab8, 0x540001a1),
            (0x111acc, 0x97fea655),
            (0x111ad4, 0xbd457101),
            (0x111adc, 0x321a03e1),
            (0x111ae4, 0x97fea7c7),
            (0xda798, 0xaa1f03e2),
            (0xda79c, 0x97ffed09),
            (0xdbf24, 0x340001e8),
            (0xdbf30, 0x2a1f03e1),
            (0xdbf38, 0x97fce936),
            (0xdbf40, 0x97fd2d0c),
            (0xd70c0, 0x2a1f03e0),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        assert_eq!(
            f32::from_le_bytes(exe[0x159570 + 256..0x159574 + 256].try_into().unwrap()),
            0.1
        );
        assert_eq!(
            0x10da98 + usize::from(exe[0x18662d + 256 + 5]) * 4,
            0x10dc08
        );
        assert_eq!(
            0x10da98 + usize::from(exe[0x18662d + 256 + 2]) * 4,
            0x10dac4
        );
        // Native item9 jump-table entry reaches the Manual child branch.
        assert_eq!(
            0x10da98 + usize::from(exe[0x18662d + 256 + 9]) * 4,
            0x10db90
        );
        assert_eq!(
            shin_core::vm::command::types::LayerProperty::BlurRadius as u16,
            0x42
        );
    }
}
