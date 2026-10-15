//! Concrete CGMode lifetime. Native grid, retained Viewer and First share one
//! borrowed private scope; the caller owns audio/acquisitions/scene dispatch.
pub(in crate::app::higurashi) use super::viewer::owner::Seeds;
use super::{Action, Kind, Model, Services as ModelServices, Task, ViewState, view::View, viewer};
use crate::{
    adv::{
        character_view::CharacterTextures,
        higurashi::{
            browser_navigation::labels::{Glyphs, Layout},
            input::Buttons,
            mode_stack::{Mode, ModePush, ModeStack},
            reconstruction_mode::{OverlayClock, overlay::DrawPacket},
        },
        input_area::{InputArea, InputAreaTarget, PointerFrame},
    },
    asset::{font::GpuFontLazy, mask::MaskTexture, system::AssetServer},
    layer::render_params::multiply_transforms,
};
use anyhow::{Result, bail, ensure};
use glam::{Mat4, Vec2, Vec4, vec3};
use shin_core::{
    format::{save::higurashi::SaveData, scenario::higurashi::HigurashiScenario},
    layout::LayoutParams,
    time::Ticks,
};
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, LayerShaderOutputKind,
    RenderProgramWithArguments, RenderRequestBuilder, render_pass::RenderPass,
    render_texture::RenderTexture, shaders::types::buffer::VertexSource,
};
use std::sync::Arc;

/// This composition is borrowed only during the native main-poll callback.
/// It includes the grid, completion glyphs and the OLD Viewer with badges
/// hidden. The capture service must not update anything or poll any mode.
pub(in crate::app::higurashi) struct Composition<'a, 'v> {
    parent: Parent<'a>,
    viewer: &'a viewer::owner::Composition<'v>,
}
impl Composition<'_, '_> {
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        self.parent.render(pass)?;
        self.viewer.render(pass)
    }
    pub fn badges_hidden(&self) -> bool {
        self.viewer.badges_hidden()
    }
    pub fn old_picture_identity(&self) -> Option<u64> {
        self.viewer.old_picture_identity()
    }
}
pub(in crate::app::higurashi) trait Services {
    /// Render/copy exactly this composition into a newly owned physical color
    /// bitmap (clearblack/depth1/stencil0), restoring the prior render target.
    /// A cached bitmap is valid only if it already matches these native flags.
    fn capture_parent(&mut self, composition: &Composition<'_, '_>) -> Result<RenderTexture>;
    /// Literal BGM/cue/achievement/Title15 events. The outer app retains complete
    /// SaveData/runtime slots/history/clock providers; no GameData is projected.
    fn event(&mut self, action: Action) -> Result<()>;
}
/// The grid keeps the application's approved Dpad/first-stick menu aliases.
/// Viewer pan instead uses genuine Dpad/keyboard low bits plus signed axes.
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::app::higurashi) struct Input {
    pub buttons: Buttons,
    pub viewer_buttons: Buttons,
    pub first_stick: [i8; 2],
}

struct Job {
    task: Option<shin_tasks::AsyncTask<Result<()>>>,
    retirement: shin_tasks::LoaderRetirement,
}
impl Drop for Job {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.cancel_and_retire();
        }
        self.retirement.clone().retire(());
    }
}
struct Loader {
    job: Option<Job>,
    atlas: Option<Arc<CharacterTextures>>,
    mask: Option<Arc<MaskTexture>>,
    snapshot: Option<RenderTexture>,
    published: Arc<parking_lot::Mutex<Published>>,
}
enum Gate {
    None,
    #[cfg(test)]
    AfterAtlas(
        Box<
            dyn FnOnce(
                    std::sync::Weak<CharacterTextures>,
                ) -> futures::future::BoxFuture<'static, ()>
                + Send,
        >,
    ),
}
#[derive(Default)]
struct Published {
    closed: bool,
    atlas: Option<Arc<CharacterTextures>>,
    mask: Option<Arc<MaskTexture>>,
}
impl Loader {
    fn current_atlas(&self) -> Option<Arc<CharacterTextures>> {
        self.atlas
            .clone()
            .or_else(|| self.published.lock().atlas.clone())
    }
    fn new(server: Arc<AssetServer>, snapshot: Option<RenderTexture>, gate: Gate) -> Self {
        let published = Arc::new(parking_lot::Mutex::new(Published::default()));
        let output = published.clone();
        let task = shin_tasks::async_io::spawn_loader(async move {
            // Actual41804 sequence; do not parallelize the mask acquisition.
            let atlas: Arc<CharacterTextures> = server.load("/cgmode.txa").await?;
            {
                let mut output = output.lock();
                if output.closed {
                    return Ok(());
                }
                output.atlas = Some(atlas.clone());
            }
            match gate {
                Gate::None => {}
                #[cfg(test)]
                Gate::AfterAtlas(callback) => callback(Arc::downgrade(&atlas)).await,
            }
            let mask = server.load("/default.msk").await?;
            let mut output = output.lock();
            if !output.closed {
                output.mask = Some(mask);
            }
            Ok(())
        });
        Self {
            job: Some(Job {
                retirement: task.retirement_handle().unwrap(),
                task: Some(task),
            }),
            atlas: None,
            mask: None,
            snapshot,
            published,
        }
    }
    fn poll(&mut self, model: &mut Model) -> Result<bool> {
        let completed = self
            .job
            .as_mut()
            .and_then(|j| j.task.as_mut())
            .and_then(|t| t.poll_naive());
        if completed.is_some() {
            self.job.as_mut().unwrap().task = None;
        }
        // The original atomic atlas may become visible while the mask worker
        // is still loading. Publish its art independently of status2.
        if let Some(atlas) = self.published.lock().atlas.take() {
            for name in [
                "bg",
                "logo",
                "check",
                "num",
                "obi",
                "caption",
                "close",
                "help_cgmode",
                "key",
            ] {
                ensure!(atlas.contains(name), "CG atlas is missing {name}");
            }
            for entry in &model.entries {
                ensure!(
                    atlas.contains(&entry.name),
                    "CG atlas is missing catalog entry {}",
                    entry.name
                );
            }
            self.atlas = Some(atlas);
            model.publish_resources();
        }
        if let Some(mask) = self.published.lock().mask.take() {
            self.mask = Some(mask);
        }
        if let Some(output) = completed {
            output?;
        }
        Ok(self.atlas.is_some() && self.job.as_ref().is_none_or(|job| job.task.is_none()))
    }
    fn retire_first(&mut self) {
        // Actual3f07c: join first, then capture/mask, then job owner.
        if let Some(job) = &mut self.job {
            if let Some(task) = job.task.take() {
                task.cancel_and_retire();
            }
            job.retirement.clone().retire(());
        }
        self.snapshot.take();
        self.mask.take();
        self.published.lock().mask.take();
        self.job.take();
    }
    fn release_parent_atlas(&mut self) {
        // Freeze future publication before dropping the parent's atomic atlas.
        // This safe output-slot boundary replaces the native raw parent pointer.
        let mut published = self.published.lock();
        published.closed = true;
        self.atlas.take();
        published.atlas.take();
    }
}
impl Drop for Loader {
    fn drop(&mut self) {
        self.retire_first();
    }
}

fn draw_overlay(
    clock: &OverlayClock,
    snapshot: Option<&RenderTexture>,
    mask: Option<&MaskTexture>,
    pass: &mut RenderPass,
) -> Result<()> {
    let mask = mask.filter(|_| clock.mask_present);
    let dimensions = mask.map(|m| {
        let s = m.texture.size();
        (s.width, s.height)
    });
    let Some(packet) = clock.draw_packet(dimensions) else {
        return Ok(());
    };
    let snapshot =
        snapshot.ok_or_else(|| anyhow::anyhow!("CG First has visible capture without a bitmap"))?;
    let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::LayerPremultiplied1);
    let transform = crate::render::normalized_projection_matrix();
    match packet {
        DrawPacket::Plain { vertices, opacity } => pass.run(builder.build(
            RenderProgramWithArguments::Layer {
                output_kind: LayerShaderOutputKind::Layer,
                fragment_shader: LayerFragmentShader::Default,
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture: snapshot.as_texture_source(),
                transform,
                color_multiplier: shin_core::primitives::color::FloatColor4::from_rgba(
                    opacity, opacity, opacity, opacity,
                ),
                fragment_shader_param: Vec4::ZERO,
            },
            DrawPrimitive::TrianglesStrip,
        )),
        DrawPacket::Masked { vertices, minmax } => pass.run(builder.build(
            RenderProgramWithArguments::HigurashiOverlay {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture: snapshot.as_texture_source(),
                mask: mask.unwrap().texture.as_source(),
                transform,
                minmax,
            },
            DrawPrimitive::TrianglesStrip,
        )),
    }
    Ok(())
}
#[derive(Clone, Copy)]
struct Parent<'a> {
    model: &'a Model,
    atlas: Option<&'a CharacterTextures>,
    label: &'a Glyphs,
    view: &'a View,
}
impl Parent<'_> {
    fn render(&self, pass: &mut RenderPass) -> Result<()> {
        let mut state = self.model.view_state();
        // Native parent rendering samples the atomic atlas independently of
        // whether the main poll has observed First worker completion.
        state.ready = self.atlas.is_some();
        self.view.render(&state, self.atlas, false, pass);
        if self.atlas.is_some() {
            let projection = Mat4::from_cols_array(&[
                2. / 1920.,
                0.,
                0.,
                0.,
                0.,
                -2. / 1080.,
                0.,
                0.,
                0.,
                0.,
                -1.,
                0.,
                -1.,
                1.,
                -0.,
                1.,
            ]);
            let matrix = multiply_transforms(
                projection,
                multiply_transforms(
                    Mat4::from_translation(vec3(48., 1012., 0.)),
                    Mat4::from_scale(vec3(1., 1., 0.)),
                ),
            );
            self.label.render_at(
                pass,
                matrix,
                Vec4::ONE,
                Vec4::new(0., 0., 0., 240. * f32::from_bits(0x3b808081)),
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
            );
        }
        Ok(())
    }
}
struct ViewerCapture<'a> {
    parent: Parent<'a>,
    services: &'a mut dyn Services,
}
impl viewer::owner::Capture for ViewerCapture<'_> {
    fn capture_parent(&mut self, viewer: &viewer::owner::Composition<'_>) -> Result<RenderTexture> {
        ensure!(
            viewer.badges_hidden(),
            "Native CG Viewer capture requires hidden badges"
        );
        self.services.capture_parent(&Composition {
            parent: self.parent,
            viewer,
        })
    }
    fn cue(&mut self, cue: viewer::Cue) -> Result<()> {
        self.services.event(Action::Cue(match cue {
            viewer::Cue::Decide => "decide",
            viewer::Cue::Cancel => "cancel",
        }))
    }
}
struct Target<'a>(&'a mut Model);
impl InputAreaTarget for Target<'_> {
    fn probe(&mut self, p: Vec2) -> bool {
        self.0.probe_contact(p)
    }
    fn reset(&mut self) {
        self.0.reset_contact();
    }
    fn press(&mut self, _: Vec2) {
        self.0.press_contact();
    }
    fn release(&mut self, _: Vec2) {
        self.0.release_contact();
    }
    fn long_press(&mut self, _: Vec2) {}
    fn drag(&mut self, _: Vec2, _: Vec2) {
        self.0.drag_contact();
    }
    fn gesture(&mut self, v: Vec2) {
        self.0.gesture_contact(v);
    }
    fn cancel(&mut self) {
        self.0.cancel_contact();
    }
}
struct ModelFrame<'a, 'p> {
    area: &'a mut InputArea,
    buttons: Buttons,
    pointer: &'a PointerFrame<'p>,
    events: Vec<Action>,
}
impl ModelServices for ModelFrame<'_, '_> {
    fn input(&mut self, model: &mut Model, permission: bool) -> Result<()> {
        let captured = if permission {
            self.area.update(&mut Target(model), self.pointer)
        } else {
            self.area.disable(&mut Target(model));
            false
        };
        model.controls(permission, captured, self.buttons);
        Ok(())
    }
    fn event(&mut self, action: Action) -> Result<()> {
        self.events.push(action);
        Ok(())
    }
}
enum Owned {
    Private(Task),
    Viewer(viewer::owner::Owner),
}
struct Frame<'a, 'p> {
    model: &'a mut Model,
    loader: &'a mut Loader,
    area: &'a mut InputArea,
    ready: bool,
    server: &'a Arc<AssetServer>,
    scenario: &'a Arc<HigurashiScenario>,
    seeds: &'a Seeds,
    label: &'a Glyphs,
    view: &'a View,
    extent: (u16, u16),
    input: viewer::Input,
    buttons: Buttons,
    pointer: &'a PointerFrame<'p>,
    services: &'a mut dyn Services,
}
impl Mode<Frame<'_, '_>> for Owned {
    fn completion(&mut self) -> u32 {
        match self {
            Self::Private(t) => u32::from(t.done),
            Self::Viewer(v) => u32::from(v.complete()),
        }
    }
    fn retire(&mut self, frame: &mut Frame<'_, '_>) {
        if matches!(
            self,
            Self::Private(Task {
                kind: Kind::First(_),
                ..
            })
        ) {
            frame.loader.retire_first();
        }
    }
    fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        frame: &mut Frame<'_, '_>,
        pushes: &mut ModePush<'_, Self>,
    ) -> Result<u32> {
        match self {
            Self::Private(task) => {
                let mut services = ModelFrame {
                    area: frame.area,
                    buttons: frame.buttons,
                    pointer: frame.pointer,
                    events: vec![],
                };
                for task in task.poll_native(
                    frame.model,
                    if frame.ready { 2 } else { 1 },
                    delta,
                    permission,
                    &mut services,
                )? {
                    pushes.push(Self::Private(task));
                }
                for event in services.events {
                    match event {
                        Action::OpenViewer(index) => {
                            let entry = frame
                                .model
                                .entries
                                .get(index)
                                .ok_or_else(|| anyhow::anyhow!("CG Viewer index exceeds catalog"))?
                                .clone();
                            let atlas = frame
                                .loader
                                .atlas
                                .as_ref()
                                .ok_or_else(|| {
                                    anyhow::anyhow!("CG Viewer opened before atlas publication")
                                })?
                                .clone();
                            pushes.push(Self::Viewer(viewer::owner::Owner::new(
                                entry,
                                frame.server.clone(),
                                frame.scenario.clone(),
                                frame.seeds.clone(),
                                atlas,
                                frame.extent,
                            )?));
                        }
                        Action::DisableInput => {
                            frame.area.disable(&mut Target(frame.model));
                        }
                        other => frame.services.event(other)?,
                    }
                }
            }
            Self::Viewer(viewer) => {
                let parent = Parent {
                    model: frame.model,
                    atlas: frame.loader.atlas.as_deref(),
                    label: frame.label,
                    view: frame.view,
                };
                viewer.poll(
                    delta,
                    permission,
                    frame.input,
                    frame.pointer,
                    &mut ViewerCapture {
                        parent,
                        services: frame.services,
                    },
                )?;
            }
        }
        // Native private function wrappers and Viewer consume parent input.
        Ok(0)
    }
}
pub(in crate::app::higurashi) struct Owner {
    model: Option<Model>,
    label: Option<Glyphs>,
    loader: Loader,
    scope: ModeStack<Owned>,
    area: InputArea,
    view: View,
    server: Arc<AssetServer>,
    scenario: Arc<HigurashiScenario>,
    seeds: Seeds,
    extent: (u16, u16),
    fault: Option<String>,
}
impl Owner {
    pub fn new(
        server: Arc<AssetServer>,
        scenario: Arc<HigurashiScenario>,
        font: Arc<GpuFontLazy>,
        settings: &SaveData,
        capture: Option<RenderTexture>,
        seeds: Seeds,
        extent: (u16, u16),
        services: &mut dyn Services,
    ) -> Result<Self> {
        let mut capture = capture;
        Self::new_retained(
            server,
            scenario,
            font,
            settings,
            &mut capture,
            seeds,
            extent,
            services,
        )
    }
    pub fn new_retained(
        server: Arc<AssetServer>,
        scenario: Arc<HigurashiScenario>,
        font: Arc<GpuFontLazy>,
        settings: &SaveData,
        capture: &mut Option<RenderTexture>,
        seeds: Seeds,
        extent: (u16, u16),
        services: &mut dyn Services,
    ) -> Result<Self> {
        Self::construct(
            server,
            scenario,
            font,
            settings,
            capture,
            seeds,
            extent,
            services,
            Gate::None,
        )
    }
    fn construct(
        server: Arc<AssetServer>,
        scenario: Arc<HigurashiScenario>,
        font: Arc<GpuFontLazy>,
        settings: &SaveData,
        capture: &mut Option<RenderTexture>,
        seeds: Seeds,
        extent: (u16, u16),
        services: &mut dyn Services,
        gate: Gate,
    ) -> Result<Self> {
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "CG grid requires a nonzero pointer extent"
        );
        ensure!(
            server.context().game == crate::cli::Game::Higurashi,
            "CG grid requires Higurashi assets"
        );
        let mut model = Model::new(&scenario.picture_box_info, settings, capture.is_some())?;
        for action in model.take_actions() {
            services.event(action)?;
        }
        // Literal SimpleText constructor params width640,size40. Existing older
        // layout/glyph renderer is reused; no newer Umineko text owner is used.
        let layout = Layout::with_params(
            font.clone(),
            &model.completion,
            LayoutParams {
                layout_width: 640.,
                text_size: 40.,
                perform_soft_breaks: false,
                ..Default::default()
            },
        )?;
        let label = Glyphs::from_layout(
            &server.context().wgpu_device,
            &server.context().wgpu_queue,
            font,
            layout,
        );
        let mut scope = ModeStack::default();
        for task in model.take_pending_tasks() {
            scope.push(Owned::Private(task));
        }
        let loader = Loader::new(server.clone(), capture.take(), gate);
        Ok(Self {
            model: Some(model),
            label: Some(label),
            loader,
            scope,
            area: InputArea::new(extent.0, extent.1),
            view: View::new(),
            server,
            scenario,
            seeds,
            extent,
            fault: None,
        })
    }
    pub fn resize(&mut self, extent: (u16, u16)) -> Result<()> {
        ensure!(
            extent.0 > 0 && extent.1 > 0,
            "Invalid CG grid pointer extent"
        );
        if self.extent != extent {
            self.cancel_input();
            self.area = InputArea::new(extent.0, extent.1);
            self.extent = extent;
            for item in self.scope.iter_mut() {
                if let Owned::Viewer(viewer) = item {
                    viewer.resize(extent)?;
                }
            }
        }
        Ok(())
    }
    pub fn cancel_input(&mut self) {
        self.area.disable(&mut Target(self.model.as_mut().unwrap()));
        for item in self.scope.iter_mut() {
            if let Owned::Viewer(viewer) = item {
                viewer.cancel_input();
            }
        }
    }
    /// input.first_stick is the native signed pair/Y-down. Only this Viewer
    /// consumer should exclude first-stick synthesized low-Dpad aliases; real
    /// Dpad/approved keyboard arrows remain digital. No pointer normalization
    /// is performed outside InputArea's single physical→1920x1080 conversion.
    pub fn poll(
        &mut self,
        delta: Ticks,
        permission: bool,
        input: Input,
        pointer: &PointerFrame<'_>,
        services: &mut dyn Services,
    ) -> Result<()> {
        ensure!(
            delta.as_f32().is_finite() && delta.as_f32() >= 0.,
            "Invalid CG grid delta"
        );
        if let Some(fault) = &self.fault {
            bail!("CG grid failed:{fault}");
        }
        let model = self.model.as_mut().unwrap();
        let result = (|| {
            let ready = self.loader.poll(model)?;
            self.scope.poll(
                delta,
                u32::from(permission),
                &mut Frame {
                    model,
                    loader: &mut self.loader,
                    area: &mut self.area,
                    ready,
                    server: &self.server,
                    scenario: &self.scenario,
                    seeds: &self.seeds,
                    label: self.label.as_ref().unwrap(),
                    view: &self.view,
                    extent: self.extent,
                    input: viewer::Input {
                        buttons: input.viewer_buttons,
                        first_stick: input.first_stick,
                    },
                    buttons: input.buttons,
                    pointer,
                    services,
                },
            )?;
            if self.loader.current_atlas().is_some() {
                model.publish_resources();
            }
            model.advance_clocks(delta);
            Ok(())
        })();
        if let Err(error) = &result {
            self.fault = Some(format!("{error:#}"));
        }
        result
    }
    pub fn state(&self) -> ViewState {
        self.model.as_ref().unwrap().view_state()
    }
    pub fn render(&self, pass: &mut RenderPass) -> Result<()> {
        let atlas = self.loader.current_atlas();
        Parent {
            model: self.model.as_ref().unwrap(),
            atlas: atlas.as_deref(),
            label: self.label.as_ref().unwrap(),
            view: &self.view,
        }
        .render(pass)?;
        let mut result = Ok(());
        self.scope.draw_ref(|item| {
            if result.is_err() {
                return;
            }
            result = match item {
                Owned::Private(Task {
                    capture: Some(clock),
                    ..
                }) => draw_overlay(
                    clock,
                    self.loader.snapshot.as_ref(),
                    self.loader.mask.as_deref(),
                    pass,
                ),
                Owned::Private(_) => Ok(()),
                Owned::Viewer(viewer) => viewer.render(pass),
            };
        });
        result
    }
    pub fn retirements(&self) -> Vec<shin_tasks::LoaderRetirement> {
        let mut result = Vec::new();
        if let Some(job) = &self.loader.job {
            result.push(job.retirement.clone());
        }
        for item in self.scope.iter() {
            if let Owned::Viewer(viewer) = item {
                if let Some(retirement) = viewer.retirement() {
                    result.push(retirement);
                }
            }
        }
        result
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub fn reference_viewer(&self) -> Option<&viewer::owner::Owner> {
        self.scope.iter().find_map(|item| {
            if let Owned::Viewer(viewer) = item {
                Some(viewer)
            } else {
                None
            }
        })
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub fn reference_model(&self) -> &Model {
        self.model.as_ref().unwrap()
    }
    #[cfg(any(test, feature = "higurashi-window-probe"))]
    pub fn reference_readiness(&self) -> (bool, bool, bool, Option<u64>) {
        let atlas = self.loader.current_atlas();
        (
            atlas.is_some(),
            self.loader.job.as_ref().is_none_or(|j| j.task.is_none()),
            self.scope.iter().any(|x| {
                matches!(
                    x,
                    Owned::Private(Task {
                        kind: Kind::Input,
                        done: false,
                        ..
                    })
                )
            }),
            atlas.as_ref().map(|a| Arc::as_ptr(a) as usize as u64),
        )
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        // Actual3c568 releases completion text, per-entry bitvectors and atlas
        // before the private scope. Workers only own copied paths/output slots.
        self.label.take();
        if let Some(model) = &mut self.model {
            model.entries.clear();
        }
        self.loader.release_parent_atlas();
        drop(std::mem::take(&mut self.scope));
        self.loader.retire_first();
        self.model.take();
    }
}

#[cfg(test)]
mod tests;
