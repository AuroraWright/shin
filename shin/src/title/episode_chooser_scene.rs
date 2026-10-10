//! Retained10e7e0 task owner and10fee0 viewer callbacks.
use super::{
    TitleItem,
    episode_chooser::{ChooserPhase, ChooserRequest, EpisodeChooser},
    episode_chooser_view,
};
use crate::{
    adv::{
        character_screen::CharacterScreen,
        character_view::CharacterTextures,
        input_area::{InputArea, PointerFrame},
        tips_screen::TipsScreen,
    },
    app::AppAction,
    asset::system::AssetServer,
};
use enum_map::EnumMap;
use shin_core::{format::scenario::Scenario, time::Ticks};
use shin_input::ActionState;
use shin_render::{render_pass::RenderPass, shaders::types::texture::TextureSource};
use std::sync::Arc;

pub(super) enum ParentRequest {
    Dim(f32),
    RestoreRows,
}

#[cfg(test)]
pub(super) fn reference_title_chooser_roundtrips(
    server: Arc<AssetServer>,
    context: &mut crate::render::PreRenderContext,
    target: &wgpu::Texture,
    target_view: &wgpu::TextureView,
) {
    use super::{TitleSetup, motion::Phase, scene::TitleScene};
    use shin_core::format::save::SaveData;
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let scenario: Arc<Scenario> = server.load_sync("/main.snr").unwrap();
    for item in [TitleItem::Characters, TitleItem::Tips] {
        let mut save = SaveData::default();
        for episode in [0, 2, 7] {
            save.persist_data.set(17 + episode, 3);
        }
        let mut globals = crate::adv::GlobalData::open(None, Some(save.clone())).unwrap();
        let initial = globals.save().clone();
        let setup = TitleSetup::from_save(&save, item as i32);
        let mut scene = TitleScene::new(server.clone(), setup.clone());
        scene.configure_load(
            scenario.clone(),
            Arc::new(shin_audio::AudioManager::new_for_tests()),
            (1920, 1080),
        );
        let mut number = 0u32;
        let mut step = |scene: &mut TitleScene, delta: f32, action: Option<AppAction>| {
            let mut state: EnumMap<AppAction, ActionState> = EnumMap::default();
            if let Some(action) = action {
                state[action].is_clicked = true;
                state[action].is_clicked_or_repeated = true;
            }
            scene
                .update_with_globals(
                    Ticks::from_f32(delta),
                    &PointerFrame {
                        number,
                        time_seconds: number as f64 / 60.0,
                        events: &[],
                    },
                    &state,
                    &mut globals,
                    glam::Vec2::new(900.0, 40.0),
                    matches!(action, Some(AppAction::PointerClick)),
                    1.0 / 60.0,
                    0.0,
                )
                .unwrap();
            number += 1;
            scene.prepare(context);
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: target_view,
                    },
                    Some(context.depth_stencil),
                    "Title chooser roundtrip",
                );
                scene.render(&mut pass);
            }
            crate::layer::user::readback_test_texture(context, target)
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !scene.is_loaded() {
            step(&mut scene, 0.0, None);
            assert!(
                std::time::Instant::now() < deadline,
                "Chooser Title assets timeout"
            );
        }
        step(&mut scene, 60.0, None);
        for _ in 0..3 {
            step(&mut scene, 0.0, None);
        }
        assert_eq!(scene.motion.phase(), Phase::Active);
        let selected = scene.motion.selected();
        step(&mut scene, 100.0, Some(AppAction::Enter));
        assert_eq!(scene.motion.phase(), Phase::Retreating);
        assert!(scene.reference_episodes().is_none());
        step(&mut scene, 0.0, None);
        assert_eq!(scene.motion.phase(), Phase::Child);
        assert_eq!(scene.motion.frame().black_alpha, 0.0);
        assert_eq!(scene.motion.frame().rows[selected].y, 228.0);
        let chooser = scene.reference_episodes().unwrap();
        assert_eq!(chooser.model.phase(), ChooserPhase::Opening);
        assert_eq!(
            chooser.model.rows().count(),
            if item == TitleItem::Characters { 3 } else { 2 }
        );
        step(&mut scene, 18.0, Some(AppAction::Cancel));
        assert_eq!(
            scene.reference_episodes().unwrap().model.phase(),
            ChooserPhase::Opening
        );
        step(&mut scene, 0.0, Some(AppAction::Cancel));
        assert_eq!(
            scene.reference_episodes().unwrap().model.phase(),
            ChooserPhase::Active
        );
        // Newly installed input task ignored that frame's Cancel.
        let chooser_pixels = step(&mut scene, 5.0, Some(AppAction::AnyDown));
        assert_eq!(
            scene.reference_episodes().unwrap().model.selected_episode(),
            2
        );
        step(&mut scene, 7.0, Some(AppAction::Enter));
        assert!(scene.reference_episodes().unwrap().viewers.is_empty());
        step(&mut scene, 0.0, None);
        assert_eq!(scene.reference_episodes().unwrap().viewers.len(), 1);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let loaded = match &scene.reference_episodes().unwrap().viewers[0] {
                Viewer::Characters { screen, .. } => screen.is_some(),
                Viewer::Tips { screen, .. } => screen.model().is_some(),
            };
            if loaded {
                break;
            }
            // Cancel during asynchronous atlas loading cannot dismiss viewer.
            step(&mut scene, 0.0, Some(AppAction::Cancel));
            assert!(
                std::time::Instant::now() < deadline,
                "Chooser viewer atlas timeout"
            );
        }
        match &scene.reference_episodes().unwrap().viewers[0] {
            Viewer::Characters {
                screen: Some(screen),
                episode,
                ..
            } => {
                assert_eq!(*episode, 2);
                assert_eq!(screen.model.characters.page, 0);
                assert_eq!(screen.model.transition.progress(), 0.0);
            }
            Viewer::Tips { screen, .. } => assert_eq!(screen.model().unwrap().progress(), 0.0),
            _ => unreachable!(),
        }
        let viewer_pixels = step(&mut scene, 10.0, Some(AppAction::Cancel));
        assert_eq!(scene.motion.frame().black_alpha, 0.5);
        assert!(
            viewer_pixels
                .iter()
                .zip(&chooser_pixels)
                .filter(|(a, b)| a != b)
                .count()
                > 10000
        );
        assert_eq!(
            scene.reference_episodes().unwrap().model.phase(),
            ChooserPhase::Active
        );
        if item == TitleItem::Characters {
            // Physical half-size pointer must reach the canvas close button.
            scene.resize((960, 540));
            step(&mut scene, 0.0, Some(AppAction::PointerClick));
            step(&mut scene, 7.0, None);
        } else {
            step(&mut scene, 7.0, Some(AppAction::Cancel));
        }
        let mut polls = 0;
        while !scene.reference_episodes().unwrap().viewers[0].finished() {
            step(&mut scene, 1.0, Some(AppAction::AnyDown));
            polls += 1;
            assert!(polls < 40, "Viewer closing did not complete");
            assert_eq!(
                scene.reference_episodes().unwrap().model.selected_episode(),
                2
            );
        }
        assert_eq!(scene.reference_episodes().unwrap().viewers.len(), 1);
        assert_eq!(scene.motion.phase(), Phase::Child);
        step(&mut scene, 5.0, None);
        scene.resize((1920, 1080));
        assert!(scene.reference_episodes().unwrap().viewers.is_empty());
        assert_eq!(scene.motion.frame().black_alpha, 0.0);
        step(&mut scene, 18.0, Some(AppAction::Cancel));
        assert_eq!(
            scene.reference_episodes().unwrap().model.phase(),
            ChooserPhase::Closing
        );
        step(&mut scene, 0.0, Some(AppAction::AnyDown));
        assert_eq!(
            scene.reference_episodes().unwrap().model.phase(),
            ChooserPhase::Closed
        );
        assert_eq!(scene.motion.phase(), Phase::RowsOpening);
        assert_eq!(scene.reference_child_task_count(), 1);
        step(&mut scene, 70.0, Some(AppAction::AnyDown));
        assert_eq!(scene.reference_child_task_count(), 0);
        assert_eq!(scene.motion.selected(), selected);
        step(&mut scene, 0.0, None);
        assert_eq!(scene.motion.phase(), Phase::Active);
        assert_eq!(
            scene.motion.frame().rows[selected].y,
            setup.row_origin(selected).unwrap().y
        );
        assert!(scene.take_destination().is_none());
        drop(step);
        assert_eq!(globals.save().encode(), initial.encode());
        println!(
            "Live Title {item:?}: chooser, episode2 viewer, retained close and restored rows pass"
        );
    }
}
enum Viewer {
    Characters {
        loading: Option<shin_tasks::AsyncTask<anyhow::Result<Arc<CharacterTextures>>>>,
        screen: Option<CharacterScreen>,
        episode: u8,
        finished: bool,
    },
    Tips {
        screen: TipsScreen,
        finished: bool,
    },
}
impl Viewer {
    fn finished(&self) -> bool {
        match self {
            Self::Characters { finished, .. } | Self::Tips { finished, .. } => *finished,
        }
    }
}
pub(super) struct EpisodeChooserScene {
    pub model: EpisodeChooser,
    area: InputArea,
    server: Arc<AssetServer>,
    scenario: Arc<Scenario>,
    viewers: Vec<Viewer>,
    parent: Vec<ParentRequest>,
    sounds: Vec<&'static str>,
    extent: (u16, u16),
    pending: Option<(
        u32,
        f64,
        Vec<crate::adv::input_area::PointerEvent>,
        EnumMap<AppAction, ActionState>,
        glam::Vec2,
        bool,
        f64,
        f32,
    )>,
}
impl EpisodeChooserScene {
    pub fn new(
        item: TitleItem,
        completed: u16,
        server: Arc<AssetServer>,
        scenario: Arc<Scenario>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            model: EpisodeChooser::new(item, completed)?,
            area: InputArea::canvas(),
            server,
            scenario,
            viewers: Vec::new(),
            parent: Vec::new(),
            sounds: Vec::new(),
            extent: (1920, 1080),
            pending: None,
        })
    }
    pub fn is_finished(&self) -> bool {
        self.model.phase() == ChooserPhase::Closed && self.viewers.iter().all(Viewer::finished)
    }
    pub fn resize(&mut self, extent: (u16, u16)) {
        self.area.disable(&mut self.model);
        self.area = InputArea::new(extent.0, extent.1);
        self.extent = extent;
        for viewer in &mut self.viewers {
            if let Viewer::Tips { screen, .. } = viewer {
                screen.set_pointer_extent(extent);
            }
        }
    }
    pub fn input(
        &mut self,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        pointer: glam::Vec2,
        held: bool,
        elapsed: f64,
        axis: f32,
    ) {
        self.pending = Some((
            frame.number,
            frame.time_seconds,
            frame.events.to_vec(),
            state.clone(),
            pointer,
            held,
            elapsed,
            axis,
        ));
    }
    pub fn update(&mut self, delta: Ticks) -> anyhow::Result<()> {
        // Completed task objects are retired at the following group poll.
        self.viewers.retain(|viewer| !viewer.finished());
        let had_viewer = !self.viewers.is_empty();
        let pending = self.pending.take();
        let viewer_count = self.viewers.len();
        for (index, viewer) in self.viewers.iter_mut().enumerate().rev() {
            if index + 1 == viewer_count {
                if let Some((number, time, events, state, pointer, held, elapsed, axis)) = &pending
                {
                    match viewer {
                        Viewer::Tips { screen, .. } => screen.input_frame(
                            &PointerFrame {
                                number: *number,
                                time_seconds: *time,
                                events,
                            },
                            state,
                            *axis,
                        ),
                        Viewer::Characters {
                            screen: Some(screen),
                            ..
                        } => {
                            let canvas_pointer = *pointer
                                * glam::Vec2::new(
                                    1920.0 / self.extent.0 as f32,
                                    1080.0 / self.extent.1 as f32,
                                );
                            screen.pointer_state(canvas_pointer, *held, *elapsed);
                            if !screen.consumes_keyboard() {
                                screen.model.handle_controller_input(state, *axis);
                            }
                        }
                        _ => {}
                    }
                }
            }
            match viewer {
                Viewer::Tips { screen, finished } => {
                    *finished = screen.update(delta)? == Some(1);
                    self.sounds.extend(screen.take_sounds());
                }
                Viewer::Characters {
                    loading,
                    screen,
                    episode,
                    finished,
                } => {
                    if let Some(task) = loading {
                        if let Some(result) = task.poll_naive() {
                            *screen = Some(CharacterScreen::with_atlas(
                                &self.server,
                                &self.scenario,
                                *episode as i32,
                                0,
                                result?,
                            ));
                            *loading = None;
                        }
                    } else if let Some(screen) = screen {
                        screen.update(delta, &self.server);
                        self.sounds.extend(screen.model.take_sounds());
                        *finished = screen.model.is_closed();
                    }
                }
            }
            if viewer.finished() {
                self.parent.push(ParentRequest::Dim(0.0));
            }
        }
        let ready = self.model.prepare_frame();
        // Input task inserted after opening waits is first eligible next poll.
        if ready && !had_viewer {
            if let Some((number, time, events, state, ..)) = &pending {
                self.area.input_frame(
                    &mut self.model,
                    &PointerFrame {
                        number: *number,
                        time_seconds: *time,
                        events,
                    },
                    state,
                );
            }
        } else {
            self.area.disable(&mut self.model);
        }
        // Confirmations snapshot before input. A task inserted by input waits
        // until next update; newly created viewer similarly starts next frame.
        for request in self.model.take_requests() {
            match request {
                ChooserRequest::RestoreTitleRows => self.parent.push(ParentRequest::RestoreRows),
                ChooserRequest::Viewer { item, episode } => {
                    self.parent.push(ParentRequest::Dim(0.5));
                    let viewer = match item {
                        TitleItem::Tips => {
                            let mut screen = TipsScreen::new(
                                self.server.clone(),
                                &self.scenario,
                                episode as i32,
                                None,
                            )?;
                            screen.set_pointer_extent(self.extent);
                            Viewer::Tips {
                                screen,
                                finished: false,
                            }
                        }
                        TitleItem::Characters => {
                            let assets = self.server.clone();
                            Viewer::Characters {
                                loading: Some(shin_tasks::async_io::spawn(async move {
                                    assets.load("/chars.txa").await
                                })),
                                screen: None,
                                episode,
                                finished: false,
                            }
                        }
                        _ => unreachable!(),
                    };
                    self.viewers.push(viewer);
                }
            }
        }
        self.model.advance_clocks(delta);
        Ok(())
    }
    pub fn take_parent_requests(&mut self) -> Vec<ParentRequest> {
        std::mem::take(&mut self.parent)
    }
    pub fn sounds(&mut self) -> Vec<&'static str> {
        let mut sounds = std::mem::take(&mut self.sounds);
        sounds.extend(self.model.take_sounds());
        sounds
    }
    pub fn render(
        &self,
        atlas: &CharacterTextures,
        background: TextureSource<'_>,
        pass: &mut RenderPass,
    ) {
        episode_chooser_view::render(&self.model, atlas, background, pass);
        for viewer in &self.viewers {
            match viewer {
                Viewer::Tips { screen, .. } => screen.render(pass),
                Viewer::Characters {
                    screen: Some(screen),
                    ..
                } => screen.render(pass),
                _ => {}
            }
        }
    }
}
