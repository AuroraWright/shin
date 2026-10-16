//! Native CG child4a720: asynchronous picture publication and capture replacement.
use crate::title::{
    captured_scene::CapturedScene,
    cg_gallery::CgRow,
    cg_viewer::{CgViewerNavigation, ViewerRequest},
    cg_viewer::input::CgViewerInput,
    cg_viewer::motion::{CgViewerMotion, ViewerEvent},
    cg_viewer::view,
};
use crate::{
    adv::{GlobalData, characters::view::CharacterTextures, input_area::PointerFrame},
    app::AppAction,
    asset::{picture::Picture, system::AssetServer},
    audio::GlobalAudio,
    layer::user::{PictureBlockParams, PictureBlockPassKind, render_block},
    render::{PreRenderContext, top_left_projection_matrix},
    update::UpdateContext,
};
use anyhow::Context;
use enum_map::EnumMap;
use glam::{Mat4, Vec2, Vec4};
use shin_core::{
    format::scenario::{Scenario, info::PictureId},
    primitives::color::{FloatColor4, UnormColor},
};
use shin_input::ActionState;
use shin_render::{
    ColorBlendType, LayerBlendType, LayerFragmentShader, RenderRequestBuilder,
    quad_vertices::QuadVertices, render_pass::RenderPass,
};
use std::sync::Arc;

pub(crate) struct CgViewerScene {
    navigation: CgViewerNavigation,
    motion: CgViewerMotion,
    input: CgViewerInput,
    loading: Option<shin_tasks::AsyncTask<anyhow::Result<Arc<Picture>>>>,
    picture: Option<Arc<Picture>>,
    capture: Option<CapturedScene>,
    server: Arc<AssetServer>,
    scenario: Arc<Scenario>,
    extent: (u16, u16),
}
impl CgViewerScene {
    pub fn new(
        server: Arc<AssetServer>,
        scenario: Arc<Scenario>,
        row: &CgRow,
        extent: (u16, u16),
    ) -> anyhow::Result<Self> {
        let mut navigation = CgViewerNavigation::new(row);
        let requests = navigation.take_requests();
        let motion = if requests.is_empty() {
            CgViewerMotion::without_load()
        } else {
            CgViewerMotion::new()
        };
        let mut input = CgViewerInput::new();
        input.resize(&mut navigation, row, extent);
        let mut this = Self {
            navigation,
            motion,
            input,
            loading: None,
            picture: None,
            capture: None,
            server,
            scenario,
            extent,
        };
        for request in requests {
            if let ViewerRequest::Load { picture, .. } = request {
                this.load(picture)?;
            }
        }
        Ok(this)
    }
    fn load(&mut self, picture: PictureId) -> anyhow::Result<()> {
        let index = usize::try_from(picture.into_i32()).context("Negative CG picture ID")?;
        let info = self
            .scenario
            .info_tables()
            .picture_info
            .get(index)
            .context("Missing CG picture metadata")?;
        let path = info.path();
        let server = self.server.clone();
        self.loading = Some(shin_tasks::async_io::spawn(async move {
            server.load::<Picture, _>(path).await
        }));
        Ok(())
    }
    /// Parent callback draws the gallery and older children, excluding this child.
    /// Native parent virtual18 draws this child too, with C1 markers suppressed.
    fn capture_before_change(
        &mut self,
        ctx: &mut PreRenderContext,
        parent: &mut impl FnMut(&mut RenderPass),
        old_opacity: f32,
        after: &mut impl FnMut(&mut RenderPass),
    ) {
        let mut target = ctx.new_render_texture("CG viewer transition capture".into());
        {
            let mut pass = ctx.begin_pass(
                target.as_texture_target(),
                Some(ctx.depth_stencil),
                "CG viewer capture",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            parent(&mut pass);
            self.render_body(&mut pass, old_opacity);
            after(&mut pass);
        }
        self.capture = Some(CapturedScene::new(target));
    }
    pub fn update(
        &mut self,
        ctx: &mut UpdateContext,
        row: &CgRow,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
        axes: Vec2,
        extent: (u16, u16),
        globals: &GlobalData,
        audio: &mut GlobalAudio,
        eligible: bool,
        mut parent: impl FnMut(&mut RenderPass),
        mut after: impl FnMut(&mut RenderPass),
    ) -> anyhow::Result<()> {
        if self.extent != extent {
            self.input.resize(&mut self.navigation, row, extent);
            self.extent = extent;
        }
        let ready = self
            .loading
            .as_mut()
            .and_then(|task| task.poll_naive())
            .transpose()?;
        if ready.is_some() {
            self.loading = None;
        }
        if let Some(picture) = &ready {
            anyhow::ensure!(
                picture.effective_width > 0 && picture.effective_height > 0,
                "CG picture has zero effective extent"
            );
        }
        let old_opacity = self.motion.capture_opacity();
        self.motion.prepare_frame(ready.is_some(), eligible);
        for event in self.motion.take_events() {
            match event {
                ViewerEvent::PublishPicture => {
                    self.capture_before_change(
                        ctx.pre_render,
                        &mut parent,
                        old_opacity,
                        &mut after,
                    );
                    let picture = ready
                        .as_ref()
                        .expect("CG publication without prepared picture")
                        .clone();
                    self.navigation.publish_picture(Vec2::new(
                        picture.effective_width as f32,
                        picture.effective_height as f32,
                    ));
                    self.picture = Some(picture);
                }
                ViewerEvent::CaptureClosing => {
                    unreachable!("Closing capture is emitted by input below")
                }
            }
        }
        self.input.input_frame(
            &mut self.navigation,
            row,
            self.motion.accepts_input(),
            frame,
            state,
            axes,
        );
        for request in self.navigation.take_requests() {
            match request {
                ViewerRequest::Load { picture, .. } => {
                    assert!(
                        self.motion.begin_load(),
                        "CG load outside eligible input task"
                    );
                    self.load(picture)?;
                }
                ViewerRequest::Close => {
                    assert!(self.motion.close(), "CG close outside eligible input task");
                    self.capture_before_change(
                        ctx.pre_render,
                        &mut parent,
                        old_opacity,
                        &mut after,
                    );
                    self.picture = None;
                    assert_eq!(self.motion.take_events(), [ViewerEvent::CaptureClosing]);
                }
            }
        }
        audio
            .system_player
            .set_settings_volume(globals.save().settings.v3_sysvol);
        for sound in self.navigation.take_sounds() {
            audio
                .system_player
                .play(&self.server, sound, Default::default());
        }
        self.navigation.update(ctx.delta_ticks);
        self.motion.advance(ctx.delta_ticks);
        Ok(())
    }
    fn render_body(&self, pass: &mut RenderPass, opacity: f32) {
        if let Some(picture) = &self.picture {
            render_picture(picture, self.navigation.transform(), pass);
        }
        if let Some(capture) = &self.capture {
            capture.render(pass, opacity);
        }
    }
    pub fn render(&self, row: &CgRow, atlas: &CharacterTextures, pass: &mut RenderPass) {
        self.render_body(pass, self.motion.capture_opacity());
        view::render_indicators(&self.navigation, row, atlas, pass);
    }
    #[cfg(test)]
    pub fn reference_ready(&self) -> bool {
        self.picture.is_some() && self.loading.is_none()
    }
    #[cfg(test)]
    pub fn reference_state(&self) -> (bool, bool, i32, bool) {
        (
            self.motion.accepts_input(),
            self.navigation.closing(),
            self.navigation.ordinal(),
            self.navigation.fit(),
        )
    }
    pub fn is_finished(&self) -> bool {
        self.motion.is_finished()
    }
}

/// c5e20 gives bounds (-originX,-originY,width,height). 4dd00/20 centers
/// that rectangle using layer origins. Header origins consequently cancel.
fn picture_transform(size: Vec2, placement: Vec4) -> Mat4 {
    top_left_projection_matrix()
        * Mat4::from_translation(glam::vec3(960.0 + placement.x, 540.0 + placement.y, 0.0))
        * Mat4::from_scale(glam::vec3(placement.z * 0.001, placement.w * 0.001, 1.0))
        * Mat4::from_translation((-size * 0.5).extend(0.0))
}
fn render_picture(picture: &Picture, placement: Vec4, pass: &mut RenderPass) {
    //4b5c0 gates both full-canvas black fill and picture drawing on current PIC.
    QuadVertices::new()
        .with_box(0.0, 0.0, 1920.0, 1080.0)
        .with_color(UnormColor::BLACK)
        .fill(
            pass,
            RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
            top_left_projection_matrix(),
        );
    let transform = picture_transform(
        Vec2::new(
            picture.effective_width as f32,
            picture.effective_height as f32,
        ),
        placement,
    );
    // Preserve the ordinary picture layer's opaque-then-transparent ordering
    // across all blocks; alpha is1, so no combined fade pass is needed.
    for pass_kind in [
        PictureBlockPassKind::OpaqueOnly,
        PictureBlockPassKind::TransparentOnly,
    ] {
        let params = PictureBlockParams {
            pass_kind,
            color_multiplier: FloatColor4::WHITE,
            blend_type: LayerBlendType::Type1,
            fragment_shader: LayerFragmentShader::Default,
            fragment_shader_param: Vec4::ZERO,
        };
        for (_, position, block) in picture.blocks.iter_placements() {
            render_block(
                block,
                pass,
                RenderRequestBuilder::new(),
                params,
                transform * Mat4::from_translation(position.extend(0.0)),
            );
        }
    }
}
#[cfg(test)]
mod tests;
