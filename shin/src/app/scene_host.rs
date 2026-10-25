//! Native27370 queues replacement;26f30 retires and constructs next frame.
use super::{
    AppAction,
    first_boot::{CommonAssets, FirstBoot},
};
use crate::{
    adv::{Adv, GlobalData, assets::AdvAssets, input_area::PointerSampler},
    asset::system::AssetServer,
    audio::GlobalAudio,
    title::{
        BustupScene, CgGalleryScene, EpisodeScene, MovieGalleryScene, MusicGalleryScene,
        ScenarioDestination, ScenarioSelectScene, TitleDemoScene, TitleItem, TitleScene,
        TitleSetup,
    },
    update::UpdateContext,
};
use enum_map::EnumMap;
use shin_core::{
    format::save::{GameData, GameDataEntry},
    vm::Scripter,
};
use shin_input::ActionState;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

enum Scene {
    Adv(Box<Adv>),
    Title(TitleRuntime),
    Picker(PickerRuntime),
    Music(Box<MusicRuntime>),
    Movie(Box<MovieRuntime>),
    Cg(Box<CgRuntime>),
    Bustup(Box<BustupRuntime>),
    Episode(EpisodeRuntime),
    Demo(DemoRuntime),
    Boot(BootRuntime),
    PreparingAdv(AdvPreparation),
}
struct BootRuntime {
    scene: FirstBoot,
    globals: GlobalData,
    audio: GlobalAudio,
}
struct AdvPreparation {
    loading: shin_tasks::AsyncTask<anyhow::Result<AdvAssets>>,
    entry: GameData,
    fresh: bool,
    globals: GlobalData,
    audio: GlobalAudio,
    source: Option<shin_render::render_texture::RenderTexture>,
}

#[cfg(all(test, target_os = "macos"))]
mod asset_failure_reference;
#[cfg(test)]
mod bustup_failure_reference;
#[cfg(test)]
mod bustup_reference;
#[cfg(test)]
mod cg_reference;
#[cfg(all(test, target_os = "macos"))]
mod gallery_child_failure_reference;
#[cfg(all(test, target_os = "macos"))]
mod gallery_failure_reference;
#[cfg(test)]
mod gallery_return_reference;
#[cfg(test)]
mod movie_reference;
#[cfg(test)]
pub(crate) mod music_reference;
#[cfg(all(test, unix))]
mod notice_failure_reference;
#[cfg(test)]
mod scenario_unlock_reference;
#[cfg(all(test, unix))]
mod shutdown_reference;
#[cfg(test)]
mod start_failure_reference;
#[cfg(test)]
mod startup_reference;
#[cfg(all(test, unix))]
mod storage_read_reference;
struct DemoRuntime {
    scene: TitleDemoScene,
    globals: GlobalData,
    audio: GlobalAudio,
}
struct TitleRuntime {
    scene: TitleScene,
    globals: GlobalData,
    audio: GlobalAudio,
    extent: (u16, u16),
}
struct PickerRuntime {
    scene: ScenarioSelectScene,
    globals: GlobalData,
    audio: GlobalAudio,
    extent: (u16, u16),
}
struct MusicRuntime {
    scene: MusicGalleryScene,
    globals: GlobalData,
    audio: GlobalAudio,
}
struct BustupRuntime {
    scene: BustupScene,
    globals: GlobalData,
    audio: GlobalAudio,
}
struct CgRuntime {
    scene: CgGalleryScene,
    globals: GlobalData,
    audio: GlobalAudio,
}
struct MovieRuntime {
    scene: MovieGalleryScene,
    globals: GlobalData,
    audio: GlobalAudio,
}
struct EpisodeRuntime {
    scene: EpisodeScene,
    globals: GlobalData,
    audio: GlobalAudio,
}
enum Destination {
    Title(TitleSetup),
    Action(TitleItem),
    PickerBack,
    PickerRefresh,
    MusicBack(i32),
    MovieBack(i32),
    CgBack(i32),
    BustupBack(i32),
    Episode(u8),
    Fresh(GameDataEntry),
    StartupTitle(i32),
    Demo,
    Load(GameData),
}
pub(crate) struct SceneHost {
    scene: Option<Scene>,
    pending: Option<Destination>,
    assets: Option<AdvAssets>,
    common: Option<Arc<CommonAssets>>,
    server: Arc<AssetServer>,
    pointer: PointerSampler,
    error: Option<String>,
    restoring_from_title: bool,
    viewer_axes: glam::Vec2,
}
impl SceneHost {
    pub(super) fn suspend_shutdown_input(&mut self, held: bool) {
        self.pointer.cancel_contact(held);
        match &mut self.scene {
            Some(Scene::Adv(adv)) => adv.suspend_shutdown_input(held),
            Some(Scene::Title(title)) => title.scene.suspend_shutdown_input(held),
            Some(Scene::Picker(picker)) => picker.scene.resize(picker.extent, held),
            _ => {}
        }
    }
    pub(super) fn has_save_task(&self) -> bool {
        match self.scene.as_ref() {
            Some(Scene::Adv(adv)) => adv.has_save_task(),
            Some(Scene::Title(title)) => title.scene.has_save_task(),
            _ => false,
        }
    }
    pub(super) fn flush_shutdown(&mut self) -> anyhow::Result<()> {
        match self.scene.as_mut() {
            Some(Scene::Adv(adv)) => adv.flush_shutdown(),
            Some(Scene::Title(TitleRuntime { globals, .. }))
            | Some(Scene::Picker(PickerRuntime { globals, .. }))
            | Some(Scene::Episode(EpisodeRuntime { globals, .. }))
            | Some(Scene::Demo(DemoRuntime { globals, .. }))
            | Some(Scene::Boot(BootRuntime { globals, .. }))
            | Some(Scene::PreparingAdv(AdvPreparation { globals, .. })) => globals.flush_shutdown(),
            Some(Scene::Music(s)) => s.globals.flush_shutdown(),
            Some(Scene::Movie(s)) => s.globals.flush_shutdown(),
            Some(Scene::Cg(s)) => s.globals.flush_shutdown(),
            Some(Scene::Bustup(s)) => s.globals.flush_shutdown(),
            None => Ok(()),
        }
    }
    pub(super) fn touch_effects_enabled(&self) -> bool {
        match &self.scene {
            Some(Scene::Adv(adv)) => adv.touch_effects_enabled(),
            Some(Scene::Boot(s)) => s.globals.save().settings.v17_showscenetitle,
            Some(Scene::Demo(s)) => s.globals.save().settings.v17_showscenetitle,
            Some(Scene::Title(s)) => s.globals.save().settings.v17_showscenetitle,
            Some(Scene::Picker(s)) => s.globals.save().settings.v17_showscenetitle,
            Some(Scene::Music(s)) => s.globals.save().settings.v17_showscenetitle,
            Some(Scene::Movie(s)) => s.globals.save().settings.v17_showscenetitle,
            Some(Scene::Cg(s)) => s.globals.save().settings.v17_showscenetitle,
            Some(Scene::Bustup(s)) => s.globals.save().settings.v17_showscenetitle,
            Some(Scene::Episode(s)) => s.globals.save().settings.v17_showscenetitle,
            Some(Scene::PreparingAdv(s)) => s.globals.save().settings.v17_showscenetitle,
            None => false,
        }
    }
    pub(super) fn system_textures(
        &self,
    ) -> Option<&Arc<crate::adv::characters::view::CharacterTextures>> {
        self.common.as_ref().map(|common| &common._system)
    }
    pub(super) fn suspend_event_input(&mut self) {
        if let Some(Scene::Adv(adv)) = &mut self.scene {
            adv.cancel_event_contact();
        }
    }

    pub fn new_first_boot(
        audio: GlobalAudio,
        server: Arc<AssetServer>,
        path: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            scene: Some(Scene::Boot(BootRuntime {
                scene: FirstBoot::new(server.clone(), path),
                globals: GlobalData::default(),
                audio,
            })),
            pending: None,
            assets: None,
            common: None,
            server,
            pointer: Default::default(),
            error: None,
            restoring_from_title: false,
            viewer_axes: glam::Vec2::ZERO,
        }
    }
    pub fn raw_input(&mut self, state: &shin_input::RawInputState) {
        self.viewer_axes = glam::Vec2::new(
            state.gamepads.unified.axes[shin_input::inputs::GamepadAxis::LeftStickX],
            -state.gamepads.unified.axes[shin_input::inputs::GamepadAxis::LeftStickY],
        );
    }
    pub fn effect_textures(&self) -> Option<&crate::asset::effects::EffectsAtlasBorrow> {
        self.common.as_ref().map(|common| &common.effects)
    }
    #[cfg(test)]
    pub fn new_startup(
        mut globals: GlobalData,
        audio: GlobalAudio,
        assets: AdvAssets,
        server: Arc<AssetServer>,
    ) -> Self {
        let scene = TitleDemoScene::new(server.clone(), &assets.scenario, &mut globals, &audio);
        Self {
            scene: Some(Scene::Demo(DemoRuntime {
                scene,
                globals,
                audio,
            })),
            pending: None,
            assets: Some(assets),
            common: None,
            server,
            pointer: Default::default(),
            error: None,
            restoring_from_title: false,
            viewer_axes: glam::Vec2::ZERO,
        }
    }
    #[cfg(test)]
    pub fn new(adv: Adv, assets: AdvAssets, server: Arc<AssetServer>) -> Self {
        Self {
            scene: Some(Scene::Adv(Box::new(adv))),
            pending: None,
            assets: Some(assets),
            common: None,
            server,
            pointer: Default::default(),
            error: None,
            restoring_from_title: false,
            viewer_axes: glam::Vec2::ZERO,
        }
    }
    fn enter_adv(
        &mut self,
        globals: GlobalData,
        audio: GlobalAudio,
        entry: GameData,
        fresh: bool,
        source: Option<shin_render::render_texture::RenderTexture>,
    ) -> anyhow::Result<()> {
        let Some(assets) = &self.assets else {
            let common = self
                .common
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("ADV requires common resources"))?
                .clone();
            let server = self.server.clone();
            self.scene = Some(Scene::PreparingAdv(AdvPreparation {
                loading: shin_tasks::async_io::spawn(async move { common.load_adv(&server).await }),
                globals,
                audio,
                entry,
                fresh,
                source,
            }));
            return Ok(());
        };
        let saved = globals.save().clone();
        let (id, seed) = if fresh {
            (entry.entry.scenario_id, entry.entry.random_seed)
        } else {
            (0, 42)
        };
        let mut adv = Adv::new_with_audio(
            self.server.clone(),
            assets.clone(),
            Scripter::new(&assets.scenario, id, seed),
            audio,
        );
        adv.install_global_data(globals);
        if let Err(error) = adv.load_game_data(&saved, &entry, self.server.clone()) {
            adv.show_scene_load_error(&error, &self.server)?;
        }
        if let Some(source) = source {
            adv.set_saved_load_source(source);
        }
        self.scene = Some(Scene::Adv(Box::new(adv)));
        self.restoring_from_title = true;
        Ok(())
    }
    // Keep gallery handoffs in separate frames: global save data is large,
    // and the common destination dispatcher already moves several scene owners.
    fn restore_title_after_gallery_error(
        &mut self,
        error: anyhow::Error,
        item: TitleItem,
        globals: GlobalData,
        audio: GlobalAudio,
        extent: (u16, u16),
    ) {
        tracing::warn!(%error, ?item, "Constructing gallery failed; returning to title");
        let mut scene = TitleScene::new(
            self.server.clone(),
            TitleSetup::from_save(globals.save(), item as i32),
        );
        scene.resize(extent);
        self.scene = Some(Scene::Title(TitleRuntime {
            scene,
            globals,
            audio,
            extent,
        }));
    }
    fn enter_music(
        &mut self,
        context: &mut crate::render::PreRenderContext,
        extent: (u16, u16),
    ) -> anyhow::Result<()> {
        let Some(Scene::Title(title)) = &self.scene else {
            anyhow::bail!("MusicMode requires Title");
        };
        let mut previous = context.new_render_texture("Title to MusicMode previous frame".into());
        {
            let mut pass = context.begin_pass(
                previous.as_texture_target(),
                Some(context.depth_stencil),
                "MusicMode complete previous title frame",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            title.scene.render(&mut pass);
        }
        let (scenario, font, system) = if let Some(common) = &self.common {
            (
                common.scenario.clone(),
                common.fonts.system_font.clone(),
                common._system.clone(),
            )
        } else {
            let assets = self
                .assets
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("MusicMode requires common resources"))?;
            let system = match self.server.load_sync("/systex.txa") {
                Ok(system) => system,
                Err(error) => {
                    let Some(Scene::Title(title)) = self.scene.take() else {
                        unreachable!()
                    };
                    let TitleRuntime {
                        scene,
                        globals,
                        audio,
                        ..
                    } = title;
                    drop(scene);
                    self.restore_title_after_gallery_error(
                        error,
                        TitleItem::Bgm,
                        globals,
                        audio,
                        extent,
                    );
                    return Ok(());
                }
            };
            (
                assets.scenario.clone(),
                assets.fonts.system_font.clone(),
                system,
            )
        };
        let Some(Scene::Title(title)) = self.scene.take() else {
            unreachable!()
        };
        let TitleRuntime {
            scene,
            globals,
            audio,
            ..
        } = title;
        drop(scene);
        let scene = match MusicGalleryScene::new(
            self.server.clone(),
            scenario,
            font,
            system,
            &globals,
            previous,
            extent,
        ) {
            Ok(scene) => scene,
            Err(error) => {
                self.restore_title_after_gallery_error(
                    error,
                    TitleItem::Bgm,
                    globals,
                    audio,
                    extent,
                );
                return Ok(());
            }
        };
        self.scene = Some(Scene::Music(Box::new(MusicRuntime {
            scene,
            globals,
            audio,
        })));
        Ok(())
    }
    fn capture_title_return(
        &self,
        context: &mut crate::render::PreRenderContext,
    ) -> shin_render::render_texture::RenderTexture {
        let mut previous =
            context.new_render_texture("Submenu return to Title previous frame".into());
        let mut pass = context.begin_pass(
            previous.as_texture_target(),
            Some(context.depth_stencil),
            "Complete outgoing submenu frame",
        );
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.0),
        );
        self.render(&mut pass);
        drop(pass);
        previous
    }
    fn leave_music(
        &mut self,
        argument: i32,
        context: &mut crate::render::PreRenderContext,
        extent: (u16, u16),
    ) -> anyhow::Result<()> {
        let previous = self.capture_title_return(context);
        let Some(Scene::Music(music)) = self.scene.take() else {
            anyhow::bail!("MusicMode back requires music gallery");
        };
        let MusicRuntime {
            scene,
            globals,
            audio,
        } = *music;
        drop(scene);
        let mut scene = TitleScene::new(
            self.server.clone(),
            TitleSetup::from_save(globals.save(), argument),
        )
        .with_return_capture(previous);
        scene.resize(extent);
        self.scene = Some(Scene::Title(TitleRuntime {
            scene,
            globals,
            audio,
            extent,
        }));
        Ok(())
    }
    fn enter_movie(
        &mut self,
        context: &mut crate::render::PreRenderContext,
        extent: (u16, u16),
    ) -> anyhow::Result<()> {
        let Some(Scene::Title(title)) = &self.scene else {
            anyhow::bail!("MovieMode requires Title");
        };
        let mut previous = context.new_render_texture("Title to MovieMode previous frame".into());
        {
            let mut pass = context.begin_pass(
                previous.as_texture_target(),
                Some(context.depth_stencil),
                "MovieMode complete previous title frame",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            title.scene.render(&mut pass);
        }
        let (scenario, font) = if let Some(common) = &self.common {
            (common.scenario.clone(), common.fonts.system_font.clone())
        } else {
            let assets = self
                .assets
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("MovieMode requires common resources"))?;
            (assets.scenario.clone(), assets.fonts.system_font.clone())
        };
        let Some(Scene::Title(title)) = self.scene.take() else {
            unreachable!()
        };
        let TitleRuntime {
            scene,
            globals,
            audio,
            ..
        } = title;
        drop(scene);
        let scene = match MovieGalleryScene::new(
            self.server.clone(),
            scenario,
            font,
            &globals,
            previous,
            extent,
        ) {
            Ok(scene) => scene,
            Err(error) => {
                self.restore_title_after_gallery_error(
                    error,
                    TitleItem::Movie,
                    globals,
                    audio,
                    extent,
                );
                return Ok(());
            }
        };
        self.scene = Some(Scene::Movie(Box::new(MovieRuntime {
            scene,
            globals,
            audio,
        })));
        Ok(())
    }
    fn leave_movie(
        &mut self,
        argument: i32,
        context: &mut crate::render::PreRenderContext,
        extent: (u16, u16),
    ) -> anyhow::Result<()> {
        let previous = self.capture_title_return(context);
        let Some(Scene::Movie(movie)) = self.scene.take() else {
            anyhow::bail!("MovieMode back requires movie gallery");
        };
        let MovieRuntime {
            scene,
            globals,
            audio,
        } = *movie;
        drop(scene);
        let mut scene = TitleScene::new(
            self.server.clone(),
            TitleSetup::from_save(globals.save(), argument),
        )
        .with_return_capture(previous);
        scene.resize(extent);
        self.scene = Some(Scene::Title(TitleRuntime {
            scene,
            globals,
            audio,
            extent,
        }));
        Ok(())
    }
    fn enter_cg(
        &mut self,
        context: &mut crate::render::PreRenderContext,
        extent: (u16, u16),
    ) -> anyhow::Result<()> {
        let Some(Scene::Title(title)) = &self.scene else {
            anyhow::bail!("CgMode requires Title");
        };
        let mut previous = context.new_render_texture("Title to CgMode previous frame".into());
        {
            let mut pass = context.begin_pass(
                previous.as_texture_target(),
                Some(context.depth_stencil),
                "CgMode complete previous title frame",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            title.scene.render(&mut pass);
        }
        let (scenario, font) = if let Some(common) = &self.common {
            (common.scenario.clone(), common.fonts.system_font.clone())
        } else {
            let assets = self
                .assets
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("CgMode requires common resources"))?;
            (assets.scenario.clone(), assets.fonts.system_font.clone())
        };
        let Some(Scene::Title(title)) = self.scene.take() else {
            unreachable!()
        };
        let TitleRuntime {
            scene,
            globals,
            audio,
            ..
        } = title;
        drop(scene);
        let scene = match CgGalleryScene::new(
            self.server.clone(),
            scenario,
            font,
            &globals,
            previous,
            extent,
        ) {
            Ok(scene) => scene,
            Err(error) => {
                self.restore_title_after_gallery_error(
                    error,
                    TitleItem::Cg,
                    globals,
                    audio,
                    extent,
                );
                return Ok(());
            }
        };
        self.scene = Some(Scene::Cg(Box::new(CgRuntime {
            scene,
            globals,
            audio,
        })));
        Ok(())
    }
    fn leave_cg(
        &mut self,
        argument: i32,
        context: &mut crate::render::PreRenderContext,
        extent: (u16, u16),
    ) -> anyhow::Result<()> {
        let previous = self.capture_title_return(context);
        let Some(Scene::Cg(cg)) = self.scene.take() else {
            anyhow::bail!("CgMode back requires cg gallery");
        };
        let CgRuntime {
            scene,
            globals,
            audio,
        } = *cg;
        drop(scene);
        let mut scene = TitleScene::new(
            self.server.clone(),
            TitleSetup::from_save(globals.save(), argument),
        )
        .with_return_capture(previous);
        scene.resize(extent);
        self.scene = Some(Scene::Title(TitleRuntime {
            scene,
            globals,
            audio,
            extent,
        }));
        Ok(())
    }
    fn enter_bustup(
        &mut self,
        context: &mut crate::render::PreRenderContext,
        extent: (u16, u16),
    ) -> anyhow::Result<()> {
        let Some(Scene::Title(title)) = &self.scene else {
            anyhow::bail!("BustupMode requires Title");
        };
        let mut previous = context.new_render_texture("Title to BustupMode previous frame".into());
        {
            let mut pass = context.begin_pass(
                previous.as_texture_target(),
                Some(context.depth_stencil),
                "BustupMode complete previous title frame",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            title.scene.render(&mut pass);
        }
        let scenario = if let Some(common) = &self.common {
            common.scenario.clone()
        } else {
            let assets = self
                .assets
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("BustupMode requires common resources"))?;
            assets.scenario.clone()
        };
        let Some(Scene::Title(title)) = self.scene.take() else {
            unreachable!()
        };
        let TitleRuntime {
            scene,
            globals,
            audio,
            ..
        } = title;
        drop(scene);
        let scene = match BustupScene::new(self.server.clone(), scenario, previous, extent) {
            Ok(scene) => scene,
            Err(error) => {
                self.restore_title_after_gallery_error(
                    error,
                    TitleItem::Bustup,
                    globals,
                    audio,
                    extent,
                );
                return Ok(());
            }
        };
        self.scene = Some(Scene::Bustup(Box::new(BustupRuntime {
            scene,
            globals,
            audio,
        })));
        Ok(())
    }
    fn leave_bustup(
        &mut self,
        argument: i32,
        context: &mut crate::render::PreRenderContext,
        extent: (u16, u16),
    ) -> anyhow::Result<()> {
        let previous = self.capture_title_return(context);
        let Some(Scene::Bustup(bup)) = self.scene.take() else {
            anyhow::bail!("BustupMode back requires Bustup gallery");
        };
        let BustupRuntime {
            scene,
            globals,
            audio,
        } = *bup;
        drop(scene);
        let mut scene = TitleScene::new(
            self.server.clone(),
            TitleSetup::from_save(globals.save(), argument),
        )
        .with_return_capture(previous);
        scene.resize(extent);
        self.scene = Some(Scene::Title(TitleRuntime {
            scene,
            globals,
            audio,
            extent,
        }));
        Ok(())
    }
    fn apply_pending(
        &mut self,
        context: &mut crate::render::PreRenderContext,
        extent: (u16, u16),
    ) -> anyhow::Result<()> {
        let Some(destination) = self.pending.take() else {
            return Ok(());
        };
        match destination {
            Destination::Demo => {
                let Some(Scene::Boot(boot)) = self.scene.take() else {
                    anyhow::bail!("TitleDemo requires FirstBoot")
                };
                let BootRuntime {
                    scene,
                    mut globals,
                    audio,
                } = boot;
                drop(scene);
                let common = self
                    .common
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("FirstBoot assets not published"))?;
                let scene = TitleDemoScene::new(
                    self.server.clone(),
                    &common.scenario,
                    &mut globals,
                    &audio,
                );
                self.scene = Some(Scene::Demo(DemoRuntime {
                    scene,
                    globals,
                    audio,
                }));
            }
            Destination::StartupTitle(argument) => {
                // Native110644 captures the outgoing frame for every Title,
                // so the first entry also fades in from TitleDemo's frame.
                let previous = self.capture_title_return(context);
                let Some(Scene::Demo(demo)) = self.scene.take() else {
                    anyhow::bail!("Initial title requires TitleDemo");
                };
                let DemoRuntime {
                    scene,
                    globals,
                    audio,
                } = demo;
                drop(scene);
                let mut scene = TitleScene::new(
                    self.server.clone(),
                    TitleSetup::from_save(globals.save(), argument),
                )
                .with_return_capture(previous);
                scene.resize(extent);
                self.scene = Some(Scene::Title(TitleRuntime {
                    scene,
                    globals,
                    audio,
                    extent,
                }));
            }
            Destination::Title(setup) => {
                let previous = self.capture_title_return(context);
                let Some(Scene::Adv(adv)) = &mut self.scene else {
                    anyhow::bail!("Title handoff requires ADV");
                };
                let globals = adv.take_title_global_data()?;
                let audio = adv.take_title_audio()?;
                drop(self.scene.take());
                let mut scene =
                    TitleScene::new(self.server.clone(), setup).with_return_capture(previous);
                scene.resize(extent);
                self.scene = Some(Scene::Title(TitleRuntime {
                    scene,
                    globals,
                    audio,
                    extent,
                }));
            }
            Destination::Action(TitleItem::Bgm) => self.enter_music(context, extent)?,
            Destination::MusicBack(argument) => self.leave_music(argument, context, extent)?,
            Destination::Action(TitleItem::Movie) => self.enter_movie(context, extent)?,
            Destination::MovieBack(argument) => self.leave_movie(argument, context, extent)?,
            Destination::Action(TitleItem::Cg) => self.enter_cg(context, extent)?,
            Destination::CgBack(argument) => self.leave_cg(argument, context, extent)?,
            Destination::Action(TitleItem::Bustup) => self.enter_bustup(context, extent)?,
            Destination::BustupBack(argument) => self.leave_bustup(argument, context, extent)?,
            Destination::Action(TitleItem::Start) => {
                let Some(Scene::Title(title)) = &self.scene else {
                    anyhow::bail!("Start requires title");
                };
                // Recompose the completed previous title frame before retiring
                // its resources. No input or clocks advance during this capture.
                let mut previous =
                    context.new_render_texture("Title to ScenarioSelect previous frame".into());
                {
                    let mut pass = context.begin_pass(
                        previous.as_texture_target(),
                        Some(context.depth_stencil),
                        "Title handoff complete frame",
                    );
                    pass.clear(
                        Some(shin_core::primitives::color::UnormColor::BLACK),
                        Some(0),
                        Some(1.0),
                    );
                    title.scene.render(&mut pass);
                }
                let Some(Scene::Title(title)) = self.scene.take() else {
                    unreachable!()
                };
                let TitleRuntime {
                    scene,
                    globals,
                    audio,
                    ..
                } = title;
                drop(scene);
                self.scene = Some(Scene::Picker(PickerRuntime {
                    scene: ScenarioSelectScene::new(
                        self.server.clone(),
                        globals.save(),
                        previous,
                        context,
                        extent,
                    ),
                    globals,
                    audio,
                    extent,
                }));
            }
            Destination::PickerBack => {
                let previous = self.capture_title_return(context);
                let Some(Scene::Picker(picker)) = self.scene.take() else {
                    anyhow::bail!("Picker back requires ScenarioSelect");
                };
                let PickerRuntime {
                    scene,
                    globals,
                    audio,
                    ..
                } = picker;
                drop(scene);
                let setup = TitleSetup::from_save(globals.save(), 0);
                let mut scene =
                    TitleScene::new(self.server.clone(), setup).with_return_capture(previous);
                scene.resize(extent);
                self.scene = Some(Scene::Title(TitleRuntime {
                    scene,
                    globals,
                    audio,
                    extent,
                }));
            }
            Destination::PickerRefresh => {
                let Some(Scene::Picker(picker)) = &self.scene else {
                    anyhow::bail!("Scenario unlock refresh requires picker");
                };
                let mut previous =
                    context.new_render_texture("Scenario unlock previous frame".into());
                {
                    let mut pass = context.begin_pass(
                        previous.as_texture_target(),
                        Some(context.depth_stencil),
                        "Scenario unlock handoff complete frame",
                    );
                    pass.clear(
                        Some(shin_core::primitives::color::UnormColor::BLACK),
                        Some(0),
                        Some(1.0),
                    );
                    picker.scene.render(&mut pass);
                }
                let Some(Scene::Picker(picker)) = self.scene.take() else {
                    unreachable!()
                };
                let PickerRuntime {
                    scene,
                    globals,
                    audio,
                    ..
                } = picker;
                drop(scene);
                self.scene = Some(Scene::Picker(PickerRuntime {
                    scene: ScenarioSelectScene::new(
                        self.server.clone(),
                        globals.save(),
                        previous,
                        context,
                        extent,
                    ),
                    globals,
                    audio,
                    extent,
                }));
            }
            Destination::Episode(id) => {
                let Some(Scene::Picker(picker)) = &self.scene else {
                    anyhow::bail!("Episode transition requires ScenarioSelect");
                };
                let mut previous =
                    context.new_render_texture("ScenarioSelect to episode previous frame".into());
                {
                    let mut pass = context.begin_pass(
                        previous.as_texture_target(),
                        Some(context.depth_stencil),
                        "Episode handoff completed picker frame",
                    );
                    pass.clear(
                        Some(shin_core::primitives::color::UnormColor::BLACK),
                        Some(0),
                        Some(1.0),
                    );
                    picker.scene.render(&mut pass);
                }
                let Some(Scene::Picker(picker)) = self.scene.take() else {
                    unreachable!()
                };
                let PickerRuntime {
                    scene,
                    globals,
                    audio,
                    ..
                } = picker;
                drop(scene);
                self.scene = Some(Scene::Episode(EpisodeRuntime {
                    scene: EpisodeScene::new(id, previous, context, extent),
                    globals,
                    audio,
                }));
            }
            Destination::Fresh(entry) => {
                let Some(Scene::Episode(episode)) = self.scene.take() else {
                    anyhow::bail!("Fresh ADV requires episode transition");
                };
                let EpisodeRuntime {
                    scene,
                    globals,
                    audio,
                } = episode;
                drop(scene);
                self.enter_adv(globals, audio, GameData::from_entry(entry), true, None)?;
            }
            Destination::Load(entry) => {
                let source = self.capture_title_return(context);
                let Some(Scene::Title(title)) = self.scene.take() else {
                    anyhow::bail!("Manual Load requires Title");
                };
                let TitleRuntime {
                    scene,
                    globals,
                    audio,
                    ..
                } = title;
                drop(scene);
                self.enter_adv(globals, audio, entry, false, Some(source))?;
            }
            Destination::Action(TitleItem::Continue) => {
                let Some(Scene::Title(title)) = &self.scene else {
                    anyhow::bail!("Continue requires title");
                };
                let saved = title.globals.save().clone();
                let entry = saved
                    .auto_save_slot
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Continue autosave is empty"))?
                    .clone();
                let Some(Scene::Title(title)) = self.scene.take() else {
                    unreachable!()
                };
                let TitleRuntime {
                    scene,
                    globals,
                    audio,
                    ..
                } = title;
                drop(scene);
                self.enter_adv(globals, audio, entry, false, None)?;
            }
            Destination::Action(item) => {
                // Keep requests for factories still being recovered; don't turn
                // those into unrelated raw scenario entry points.
                self.pending = Some(Destination::Action(item));
            }
        }
        Ok(())
    }
    pub fn update(
        &mut self,
        context: &mut UpdateContext,
        input: EnumMap<AppAction, ActionState>,
        physical_pointer: glam::Vec2,
        canvas_pointer: glam::Vec2,
        extent: (u16, u16),
        scroll_axis: f32,
        elapsed_seconds: f64,
    ) -> anyhow::Result<()> {
        if self.error.is_some() {
            return Ok(());
        }
        let result = self.update_inner(
            context,
            input,
            physical_pointer,
            canvas_pointer,
            extent,
            scroll_axis,
            elapsed_seconds,
        );
        if let Err(error) = &result {
            self.error = Some(format!("{error:#}"));
        }
        result
    }
    fn update_inner(
        &mut self,
        context: &mut UpdateContext,
        input: EnumMap<AppAction, ActionState>,
        physical_pointer: glam::Vec2,
        canvas_pointer: glam::Vec2,
        extent: (u16, u16),
        scroll_axis: f32,
        elapsed_seconds: f64,
    ) -> anyhow::Result<()> {
        self.apply_pending(context.pre_render, extent)?;
        if let Some(Scene::PreparingAdv(preparation)) = &mut self.scene {
            if let Some(result) = preparation.loading.poll_naive() {
                self.assets = Some(result?);
                let Some(Scene::PreparingAdv(preparation)) = self.scene.take() else {
                    unreachable!()
                };
                let AdvPreparation {
                    globals,
                    audio,
                    entry,
                    fresh,
                    source,
                    ..
                } = preparation;
                self.enter_adv(globals, audio, entry, fresh, source)?;
            }
        }
        let scenario = self
            .assets
            .as_ref()
            .map(|assets| &assets.scenario)
            .or_else(|| self.common.as_ref().map(|common| &common.scenario));
        self.pointer.sample(
            physical_pointer,
            input[AppAction::PointerClick].is_held,
            elapsed_seconds,
        );
        match self.scene.as_mut().unwrap() {
            Scene::Boot(boot) => {
                boot.scene
                    .input(canvas_pointer, extent, elapsed_seconds, &input);
                let update =
                    boot.scene
                        .update(&mut boot.globals, &mut boot.audio, context.delta_ticks)?;
                boot.scene.prepare(context.pre_render, &mut boot.audio);
                if let Some(assets) = update.assets {
                    self.common = Some(Arc::new(assets));
                }
                if update.complete {
                    self.pending = Some(Destination::Demo);
                }
                update_global_audio(&mut boot.audio, &boot.globals, context.delta_ticks);
                boot.globals.advance_play_clock(context.delta_ticks);
            }
            Scene::PreparingAdv(preparation) => {
                update_global_audio(
                    &mut preparation.audio,
                    &preparation.globals,
                    context.delta_ticks,
                );
                preparation.globals.advance_play_clock(context.delta_ticks);
            }
            Scene::Demo(demo) => {
                demo.scene.update(
                    context,
                    &self.pointer.frame(),
                    &input,
                    extent,
                    &demo.globals,
                    &mut demo.audio,
                )?;
                let settings = &demo.globals.save().settings;
                demo.audio
                    .bgm_player
                    .set_settings_volume(settings.v0_bgmvol);
                demo.audio
                    .bgm_player
                    .update_voice_focus(context.delta_ticks, false, false);
                demo.audio.bgm_player.update();
                demo.audio
                    .system_player
                    .set_settings_volume(settings.v3_sysvol);
                demo.audio.system_player.update();
                demo.audio.se_player.set_settings_volume(settings.v1_sfxvol);
                demo.audio.se_player.update();
                if let Some(argument) = demo.scene.take_destination() {
                    self.pending = Some(Destination::StartupTitle(argument));
                }
                demo.globals.advance_play_clock(context.delta_ticks);
            }
            Scene::Adv(adv) => {
                adv.sample_modal_pointer(
                    physical_pointer,
                    extent,
                    input[AppAction::PointerClick].is_held,
                    elapsed_seconds,
                );
                adv.handle_pointer_state(
                    canvas_pointer,
                    input[AppAction::PointerClick].is_clicked,
                    input[AppAction::PointerClick].is_held,
                    elapsed_seconds,
                );
                adv.set_character_scroll_axis(scroll_axis);
                adv.set_interactive_axes(self.viewer_axes);
                adv.update(context, input);
                if self.restoring_from_title {
                    if adv.saved_scene_load_error().is_some() {
                        if let Some(setup) = adv.failed_scene_title_setup() {
                            self.pending = Some(Destination::Title(setup));
                            self.restoring_from_title = false;
                        }
                    } else if !adv.is_loading_saved_scene() {
                        self.restoring_from_title = false;
                    }
                }
                if let Some(setup) = adv.pending_title_setup() {
                    self.pending = Some(Destination::Title(setup));
                }
                adv.advance_play_clock(context.delta_ticks);
            }
            Scene::Title(title) => {
                if title.extent != extent {
                    title.scene.resize(extent);
                    title.extent = extent;
                }
                let canvas = context.pre_render.resize_source.canvas_handle().get();
                title.scene.configure_load(
                    scenario.expect("Title requires scenario").clone(),
                    title.audio.manager.clone(),
                    (canvas.width, canvas.height),
                );
                title.scene.update_with_globals(
                    context.delta_ticks,
                    &self.pointer.frame(),
                    &input,
                    &mut title.globals,
                    physical_pointer,
                    input[AppAction::PointerClick].is_held,
                    elapsed_seconds,
                    scroll_axis,
                )?;
                let GlobalAudio {
                    bgm_player,
                    system_player,
                    se_player,
                    ..
                } = &mut title.audio;
                title.scene.update_audio(
                    &self.server,
                    scenario.expect("Title requires scenario"),
                    &title.globals.save().settings,
                    bgm_player,
                    system_player,
                    context.delta_ticks,
                );
                se_player.set_settings_volume(title.globals.save().settings.v1_sfxvol);
                se_player.update();
                title.scene.prepare(context.pre_render);
                while let Some(item) = title.scene.take_destination() {
                    self.pending = Some(Destination::Action(item));
                }
                if let Some(entry) = title.scene.take_load_game_data() {
                    self.pending = Some(Destination::Load(entry));
                }
                title.globals.advance_play_clock(context.delta_ticks);
            }
            Scene::Music(music) => {
                music.scene.resize(extent);
                music.scene.update(
                    context.delta_ticks,
                    &self.pointer.frame(),
                    &input,
                    scroll_axis,
                    &mut music.globals,
                    &mut music.audio,
                )?;
                update_global_audio(&mut music.audio, &music.globals, context.delta_ticks);
                if let Some(argument) = music.scene.take_destination() {
                    self.pending = Some(Destination::MusicBack(argument));
                }
                music.globals.advance_play_clock(context.delta_ticks);
            }
            Scene::Movie(movie) => {
                movie.scene.resize(extent);
                movie.scene.update(
                    context,
                    &self.pointer.frame(),
                    &input,
                    &movie.globals,
                    &mut movie.audio,
                )?;
                update_global_audio(&mut movie.audio, &movie.globals, context.delta_ticks);
                if let Some(argument) = movie.scene.take_destination() {
                    self.pending = Some(Destination::MovieBack(argument));
                }
                movie.globals.advance_play_clock(context.delta_ticks);
            }
            Scene::Cg(cg) => {
                cg.scene.resize(extent);
                cg.scene.update(
                    context,
                    &self.pointer.frame(),
                    &input,
                    &cg.globals,
                    &mut cg.audio,
                    scroll_axis,
                    self.viewer_axes,
                )?;
                update_global_audio(&mut cg.audio, &cg.globals, context.delta_ticks);
                if let Some(argument) = cg.scene.take_destination() {
                    self.pending = Some(Destination::CgBack(argument));
                }
                cg.globals.advance_play_clock(context.delta_ticks);
            }
            Scene::Bustup(bup) => {
                bup.scene.resize(extent);
                bup.scene.update(
                    context,
                    &self.pointer.frame(),
                    &input,
                    &bup.globals,
                    &mut bup.audio,
                    self.viewer_axes,
                )?;
                update_global_audio(&mut bup.audio, &bup.globals, context.delta_ticks);
                if let Some(argument) = bup.scene.take_destination() {
                    self.pending = Some(Destination::BustupBack(argument));
                }
                bup.globals.advance_play_clock(context.delta_ticks);
            }
            Scene::Picker(picker) => {
                if picker.extent != extent {
                    picker
                        .scene
                        .resize(extent, input[AppAction::PointerClick].is_held);
                    picker.extent = extent;
                }
                picker.scene.update_frame(
                    context.delta_ticks,
                    &self.pointer.frame(),
                    &input,
                    canvas_pointer,
                    elapsed_seconds,
                    &self.server,
                    context.pre_render,
                )?;
                let GlobalAudio {
                    bgm_player,
                    system_player,
                    se_player,
                    ..
                } = &mut picker.audio;
                picker.scene.update_audio(
                    &self.server,
                    scenario.expect("ScenarioSelect requires scenario"),
                    &picker.globals.save().settings,
                    bgm_player,
                    system_player,
                    context.delta_ticks,
                );
                se_player.set_settings_volume(picker.globals.save().settings.v1_sfxvol);
                se_player.update();
                if let Some(destination) = picker.scene.take_destination() {
                    self.pending = Some(match destination {
                        ScenarioDestination::Title => Destination::PickerBack,
                        ScenarioDestination::Episode(id) => Destination::Episode(id),
                        ScenarioDestination::UnlockScenarios => {
                            picker.globals.unlock_main_scenarios();
                            Destination::PickerRefresh
                        }
                    });
                }
                picker.globals.advance_play_clock(context.delta_ticks);
            }
            Scene::Episode(episode) => {
                episode
                    .scene
                    .update(context.delta_ticks, context.pre_render);
                let settings = &episode.globals.save().settings;
                episode
                    .audio
                    .bgm_player
                    .set_settings_volume(settings.v0_bgmvol);
                episode
                    .audio
                    .bgm_player
                    .update_voice_focus(context.delta_ticks, false, false);
                episode.audio.bgm_player.update();
                episode
                    .audio
                    .system_player
                    .set_settings_volume(settings.v3_sysvol);
                episode.audio.system_player.update();
                episode
                    .audio
                    .se_player
                    .set_settings_volume(settings.v1_sfxvol);
                episode.audio.se_player.update();
                if let Some(entry) = episode.scene.take_destination() {
                    self.pending = Some(Destination::Fresh(entry));
                }
                episode.globals.advance_play_clock(context.delta_ticks);
            }
        }
        Ok(())
    }
    pub fn render(&self, pass: &mut RenderPass) {
        // Native app27070..27080 clears color black, stencil0 and depth1 before
        // invoking the active scene. RenderPass otherwise loads prior pixels.
        pass.clear(
            Some(shin_core::primitives::color::UnormColor::BLACK),
            Some(0),
            Some(1.0),
        );
        match &self.scene {
            Some(Scene::Adv(adv)) => adv.render(pass),
            Some(Scene::Title(title)) => title.scene.render(pass),
            Some(Scene::Picker(picker)) => picker.scene.render(pass),
            Some(Scene::Music(music)) => music.scene.render(pass),
            Some(Scene::Movie(movie)) => movie.scene.render(pass),
            Some(Scene::Cg(cg)) => cg.scene.render(pass),
            Some(Scene::Bustup(bup)) => bup.scene.render(pass),
            Some(Scene::Episode(episode)) => episode.scene.render(pass),
            Some(Scene::Demo(demo)) => demo.scene.render(pass),
            Some(Scene::Boot(boot)) => boot.scene.render(pass),
            Some(Scene::PreparingAdv(preparation)) => {
                if let Some(source) = &preparation.source {
                    crate::adv::scene_load_fade::render_snapshot(source, pass);
                }
            }
            None => {}
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_enter_saved_scene(
        &mut self,
        globals: GlobalData,
        audio: GlobalAudio,
        game: GameData,
    ) -> anyhow::Result<()> {
        self.enter_adv(globals, audio, game, false, None)
    }
    #[cfg(test)]
    pub fn reference_save(&self) -> shin_core::format::save::SaveData {
        match self.scene.as_ref().unwrap() {
            Scene::Adv(adv) => adv.reference_global_save(),
            Scene::Title(title) => title.globals.save().clone(),
            Scene::Picker(picker) => picker.globals.save().clone(),
            Scene::Music(music) => music.globals.save().clone(),
            Scene::Movie(movie) => movie.globals.save().clone(),
            Scene::Cg(cg) => cg.globals.save().clone(),
            Scene::Bustup(bup) => bup.globals.save().clone(),
            Scene::Episode(episode) => episode.globals.save().clone(),
            Scene::Demo(demo) => demo.globals.save().clone(),
            Scene::Boot(boot) => boot.globals.save().clone(),
            Scene::PreparingAdv(preparation) => preparation.globals.save().clone(),
        }
    }
    #[cfg(test)]
    pub fn reference_play_ticks(&self) -> f64 {
        match self.scene.as_ref().unwrap() {
            Scene::Adv(adv) => adv.play_ticks(),
            Scene::Title(title) => title.globals.play_ticks(),
            Scene::Picker(picker) => picker.globals.play_ticks(),
            Scene::Music(music) => music.globals.play_ticks(),
            Scene::Movie(movie) => movie.globals.play_ticks(),
            Scene::Cg(cg) => cg.globals.play_ticks(),
            Scene::Bustup(bup) => bup.globals.play_ticks(),
            Scene::Episode(episode) => episode.globals.play_ticks(),
            Scene::Demo(demo) => demo.globals.play_ticks(),
            Scene::Boot(boot) => boot.globals.play_ticks(),
            Scene::PreparingAdv(preparation) => preparation.globals.play_ticks(),
        }
    }
    #[cfg(test)]
    pub fn reference_adv(&self) -> Option<&Adv> {
        match &self.scene {
            Some(Scene::Adv(adv)) => Some(adv),
            _ => None,
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_adv_mut(&mut self) -> Option<&mut Adv> {
        match &mut self.scene {
            Some(Scene::Adv(adv)) => Some(adv),
            _ => None,
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_request_saved_scene(&mut self, game: GameData) {
        assert!(matches!(self.scene, Some(Scene::Title(_))));
        self.pending = Some(Destination::Load(game));
    }
    #[cfg(test)]
    pub fn reference_title(&self) -> Option<&TitleScene> {
        match &self.scene {
            Some(Scene::Title(title)) => Some(&title.scene),
            _ => None,
        }
    }
    #[cfg(test)]
    pub fn reference_picker(&self) -> Option<&ScenarioSelectScene> {
        match &self.scene {
            Some(Scene::Picker(picker)) => Some(&picker.scene),
            _ => None,
        }
    }
    #[cfg(test)]
    pub fn reference_episode_request(&self) -> Option<u8> {
        match self.pending {
            Some(Destination::Episode(id)) => Some(id),
            _ => None,
        }
    }
    #[cfg(test)]
    pub fn reference_transition(&self) -> bool {
        matches!(self.scene, Some(Scene::Episode(_)))
    }
    #[cfg(test)]
    pub fn reference_fresh_entry(&self) -> Option<&GameDataEntry> {
        match &self.pending {
            Some(Destination::Fresh(entry)) => Some(entry),
            _ => None,
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_gallery_return<'a>(
        server: &'a Arc<AssetServer>,
        ctx: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
        target: &wgpu::Texture,
    ) {
        gallery_return_reference::check(server, ctx, target);
    }
    #[cfg(test)]
    pub(crate) fn reference_music_roundtrip<'a>(
        server: &'a Arc<AssetServer>,
        context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
        target: &wgpu::Texture,
        view: &wgpu::TextureView,
    ) {
        music_reference::reference_live_music_gallery_roundtrip(server, context, target, view);
        movie_reference::reference_live_movie_gallery_roundtrip(server, context, target, view);
    }
    #[cfg(test)]
    pub(crate) fn reference_bustup_roundtrip<'a>(
        server: &'a Arc<AssetServer>,
        ctx: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
        target: &wgpu::Texture,
        view: &wgpu::TextureView,
    ) {
        bustup_reference::reference_live_bustup_roundtrip(server, ctx, target, view);
    }
    #[cfg(test)]
    pub(crate) fn reference_cg_roundtrip<'a>(
        server: &'a Arc<AssetServer>,
        ctx: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
        target: &wgpu::Texture,
        view: &wgpu::TextureView,
    ) {
        cg_reference::reference_live_cg_gallery_roundtrip(server, ctx, target, view);
    }
    #[cfg(test)]
    pub fn reference_audio(&self) -> &GlobalAudio {
        match self.scene.as_ref().unwrap() {
            Scene::Adv(adv) => adv.adv_state_audio(),
            Scene::Title(title) => &title.audio,
            Scene::Picker(picker) => &picker.audio,
            Scene::Music(music) => &music.audio,
            Scene::Movie(movie) => &movie.audio,
            Scene::Cg(cg) => &cg.audio,
            Scene::Bustup(bup) => &bup.audio,
            Scene::Episode(episode) => &episode.audio,
            Scene::Demo(demo) => &demo.audio,
            Scene::Boot(boot) => &boot.audio,
            Scene::PreparingAdv(preparation) => &preparation.audio,
        }
    }
}

fn update_global_audio(
    audio: &mut GlobalAudio,
    globals: &GlobalData,
    delta: shin_core::time::Ticks,
) {
    let settings = &globals.save().settings;
    audio.bgm_player.set_settings_volume(settings.v0_bgmvol);
    audio.bgm_player.update_voice_focus(delta, false, false);
    audio.bgm_player.update();
    audio.system_player.set_settings_volume(settings.v3_sysvol);
    audio.system_player.update();
    audio.se_player.set_settings_volume(settings.v1_sfxvol);
    audio.se_player.update();
}
