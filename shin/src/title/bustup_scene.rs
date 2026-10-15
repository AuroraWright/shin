//! Live native BustupMode37c70: asynchronous publications, opening and shared viewer.
use super::{
    bustup_catalog::BustupCatalog,
    bustup_input::BustupInput,
    bustup_menu::{BustupMenu, Request},
    bustup_resources::{BustupResources, Publication},
    bustup_view,
    bustup_viewer::{BustupViewer, BustupViewerInput},
    captured_scene::CapturedScene,
};
use crate::{
    adv::{GlobalData, character_view::CharacterTextures, input_area::PointerFrame},
    app::AppAction,
    asset::{mask::MaskTexture, system::AssetServer},
    audio::GlobalAudio,
    layer::{
        DrawableLayer, Layer, ScreenLayer, render_layer_without_bg, render_params::TransformParams,
    },
    update::{AdvUpdatable, AdvUpdateContext, UpdateContext},
};
use enum_map::EnumMap;
use shin_core::{
    format::scenario::{Scenario, info::BgmId, instruction_elements::FromNumber},
    time::{Ticks, Tween},
    vm::command::types::{LayerProperty, PlaneId, Volume},
};
use shin_input::ActionState;
use shin_render::{render_pass::RenderPass, render_texture::RenderTexture};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Loading,
    Opening,
    InstallingInput,
    Active,
}
pub(crate) struct BustupScene {
    catalog: BustupCatalog,
    menu: BustupMenu,
    input: BustupInput,
    screen: ScreenLayer,
    resources: BustupResources,
    atlas: Option<Arc<CharacterTextures>>,
    mask: Option<Arc<MaskTexture>>,
    previous: CapturedScene,
    phase: Phase,
    elapsed: f32,
    viewer: Option<(BustupViewer, BustupViewerInput)>,
    server: Arc<AssetServer>,
    scenario: Arc<Scenario>,
    extent: (u16, u16),
    destination: Option<i32>,
    input_enabled: bool,
    #[cfg(test)]
    resource_failures: usize,
}
impl BustupScene {
    pub fn new(
        server: Arc<AssetServer>,
        scenario: Arc<Scenario>,
        previous: RenderTexture,
        extent: (u16, u16),
    ) -> anyhow::Result<Self> {
        let catalog = BustupCatalog::new(&scenario.info_tables().character_box_info)?;
        let mut menu = BustupMenu::new(&catalog);
        let resources = BustupResources::new(
            server.clone(),
            scenario.clone(),
            menu.selection().background(&catalog),
            menu.selection().bustup(&catalog),
        );
        let mut input = BustupInput::new();
        input.resize(&mut menu, &catalog, extent);
        let mut screen = ScreenLayer::new(1);
        screen
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(-200.0);
        Ok(Self {
            catalog,
            menu,
            input,
            screen,
            resources,
            atlas: None,
            mask: None,
            previous: CapturedScene::new(previous),
            phase: Phase::Loading,
            elapsed: 0.0,
            viewer: None,
            server,
            scenario,
            extent,
            destination: None,
            input_enabled: false,
            #[cfg(test)]
            resource_failures: 0,
        })
    }
    pub fn resize(&mut self, extent: (u16, u16)) {
        if self.extent == extent {
            return;
        }
        self.input.resize(&mut self.menu, &self.catalog, extent);
        if let Some((viewer, input)) = &mut self.viewer {
            input.resize(viewer, &mut self.menu, &mut self.screen, extent);
        }
        self.extent = extent;
    }
    pub fn update(
        &mut self,
        ctx: &mut UpdateContext,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        globals: &GlobalData,
        audio: &mut GlobalAudio,
        axes: glam::Vec2,
    ) -> anyhow::Result<()> {
        //1000b0 retires dead tasks before snapshotting. Completion frames retain
        //their blocking flag; a newly constructed child misses this snapshot.
        if self.viewer.as_ref().is_some_and(|(v, _)| v.is_finished()) {
            self.viewer = None;
        }
        let resource_blocked = self.resources.has_blocking_loads();
        let menu_blocked = self.menu.has_blocking_transition();
        let mut eligible = matches!(self.phase, Phase::InstallingInput | Phase::Active)
            && !resource_blocked
            && !menu_blocked;
        self.input_enabled = eligible && self.viewer.is_none();
        for event in self.resources.poll()? {
            match event {
                Publication::Atlas(atlas) => self.atlas = Some(atlas),
                Publication::Mask(mask) => self.mask = Some(mask),
                Publication::Layer(layer) => {
                    layer.install(self.screen.page_layer_mut().get_plane_mut(PlaneId::new(0)))
                }
                Publication::InitialComplete => {
                    self.phase = Phase::Opening;
                    audio.bgm_player.stop(Tween::linear(Ticks::from_f32(60.0)));
                }
                Publication::InitialFailed(error) => {
                    tracing::warn!(%error, "Returning to title after character sprite gallery load failed");
                    self.destination = Some(super::TitleItem::Bustup as i32);
                    return Ok(());
                }
                Publication::ReplacementFailed(error) => {
                    tracing::warn!(%error, "Keeping previous character sprite gallery layers after replacement failed");
                    #[cfg(test)]
                    {
                        self.resource_failures += 1;
                    }
                }
            }
        }
        if self.phase == Phase::Opening && self.elapsed >= 60.0 {
            let id = BgmId::from_number(7);
            audio.bgm_player.request(
                self.server.clone(),
                self.scenario.info_tables().bgm_info(id).path(),
                id,
                true,
                Volume(1.0),
                Tween::linear(Ticks::ZERO),
            );
            self.phase = Phase::InstallingInput;
        } else if self.phase == Phase::InstallingInput {
            self.phase = Phase::Active;
            self.previous.retire();
            self.mask = None;
        }
        if let Some((viewer, input)) = &mut self.viewer {
            viewer.prepare_frame(&mut self.menu, &self.screen, eligible);
            input.input_frame(
                viewer,
                &mut self.menu,
                &mut self.screen,
                &mut eligible,
                frame,
                state,
                axes,
            );
            viewer.advance(ctx.delta_ticks, &mut self.screen);
        }
        self.menu.prepare_frame();
        self.input
            .input_frame(&mut self.menu, &self.catalog, &mut eligible, frame, state);
        for request in self.menu.take_requests() {
            match request {
                Request::OpenViewer => {
                    self.viewer = Some((
                        BustupViewer::new(&mut self.menu, &mut self.screen),
                        BustupViewerInput::new(self.extent),
                    ));
                }
                Request::ReturnTitle {
                    argument,
                    music_stop_ticks,
                } => {
                    audio
                        .bgm_player
                        .stop(Tween::linear(Ticks::from_u32(music_stop_ticks)));
                    self.destination = Some(i32::from(argument));
                }
                request => self.resources.request(request),
            }
        }
        audio
            .system_player
            .set_settings_volume(globals.save().settings.v3_sysvol);
        for sound in self
            .menu
            .take_sounds()
            .into_iter()
            .chain(self.viewer.iter_mut().flat_map(|(v, _)| v.take_sounds()))
        {
            audio
                .system_player
                .play(&self.server, sound, Default::default());
        }
        self.screen.update(&AdvUpdateContext {
            frame_id: ctx.frame_id,
            delta_ticks: ctx.delta_ticks,
            asset_server: &self.server,
            device: ctx.pre_render.device,
            queue: ctx.pre_render.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        });
        self.screen
            .pre_render(ctx.pre_render, &TransformParams::default());
        self.menu.update(ctx.delta_ticks);
        if matches!(self.phase, Phase::Opening | Phase::InstallingInput) {
            self.elapsed = (self.elapsed + ctx.delta_ticks.as_f32()).min(60.0);
        }
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) {
        render_layer_without_bg(pass, &TransformParams::default(), &self.screen, 0);
        if let Some(atlas) = &self.atlas {
            bustup_view::render_artwork(
                &self.menu,
                &self.catalog,
                self.menu.controls_visibility(),
                atlas,
                pass,
            );
        }
        let opacity = match self.phase {
            Phase::Loading => 1.0,
            Phase::Opening | Phase::InstallingInput => 1.0 - self.elapsed / 60.0,
            Phase::Active => 0.0,
        };
        self.previous.render(pass, opacity);
    }
    pub fn take_destination(&mut self) -> Option<i32> {
        self.destination.take()
    }
    #[cfg(test)]
    pub fn reference_load_blocked(&self) -> bool {
        self.resources.has_blocking_loads()
    }
    #[cfg(test)]
    pub fn reference_opacity(&self) -> f32 {
        match self.phase {
            Phase::Loading => 1.0,
            Phase::Opening | Phase::InstallingInput => 1.0 - self.elapsed / 60.0,
            Phase::Active => 0.0,
        }
    }
    #[cfg(test)]
    pub fn reference_presentation(&self) -> (f32, f32, f32, usize) {
        let props = self
            .screen
            .page_layer()
            .get_plane(PlaneId::new(0))
            .properties();
        (
            self.screen
                .properties()
                .get_value(LayerProperty::TranslateX),
            props.get_value(LayerProperty::CameraPositionX),
            props.get_value(LayerProperty::CameraPositionY),
            self.menu.selection().indices().0,
        )
    }
    #[cfg(test)]
    pub fn reference_state(&self) -> (bool, bool, bool, bool, f32) {
        (
            self.atlas.is_some(),
            self.input_enabled,
            self.viewer.is_some(),
            self.previous.is_owned() || self.mask.is_some(),
            self.menu.controls_visibility(),
        )
    }
    #[cfg(test)]
    pub fn reference_catalog_paths(
        scenario: &Scenario,
    ) -> (Vec<(String, Option<String>)>, Vec<String>, usize) {
        let catalog = BustupCatalog::new(&scenario.info_tables().character_box_info).unwrap();
        let pictures = &scenario.info_tables().picture_info;
        let bustups = &scenario.info_tables().bustup_info;
        (
            catalog
                .backgrounds
                .iter()
                .map(|bg| {
                    (
                        pictures[bg.primary.into_i32() as usize].path(),
                        bg.secondary
                            .map(|id| pictures[id.into_i32() as usize].path()),
                    )
                })
                .collect(),
            catalog
                .characters
                .iter()
                .map(|c| bustups[c.outfits[0].poses[0].expressions[0].into_i32() as usize].path())
                .collect(),
            catalog.backgrounds.len(),
        )
    }
    #[cfg(test)]
    pub fn reference_resource_state(&self) -> (usize, [usize; 3]) {
        use crate::layer::user::UserLayer;
        use shin_core::vm::command::types::LayerbankId;
        let group = self.screen.page_layer().get_plane(PlaneId::new(0));
        (
            self.resource_failures,
            [0, 1, 10].map(|slot| {
                group
                    .get_layer(LayerbankId::new(slot))
                    .map_or(0, |layer| match &*layer {
                        UserLayer::Picture(p) => p.reference_resource_identity(),
                        UserLayer::Bustup(b) => b.reference_resource_identity(),
                        _ => 0,
                    })
            }),
        )
    }
}
