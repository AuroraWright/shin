//! Native Screen composition: group captures -> wiper -> history -> root6aaec.
//! Outerba554 samples current1498; enabled updates own all history swaps.
use super::{
    layers::HigurashiLayers,
    scene::HigurashiScene,
    scene_history::{ScreenHistory, WeakHistory},
    wipe::HigurashiDefaultTransition,
};
use crate::{
    layer::render_params::{higurashi::HigurashiTransform, multiply_transforms},
    render::PreRenderContext,
};
use anyhow::Result;
use glam::{Vec2, Vec3, Vec4};
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    DrawPrimitive, LayerFragmentShader, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    shaders::types::{buffer::VertexSource, texture::DepthStencilTarget, vertices::PosTexVertex},
};

pub struct HigurashiSceneTextures {
    history: Option<WeakHistory>,
    // The old standalone WIPE fixture API has no Scene owner or enabled ticks.
    standalone_history: Option<ScreenHistory>,
    effect_depth: crate::layer::user::higurashi::effect_capture::SharedDepth,
}
impl HigurashiSceneTextures {
    pub fn new(context: &PreRenderContext) -> Self {
        let _ = context;
        Self {
            history: None,
            standalone_history: None,
            effect_depth: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        }
    }
    pub(crate) fn for_scene(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        extent: [u32; 2],
        scene: &HigurashiScene,
    ) -> Result<Self> {
        scene.bind_history(device, queue, extent)?;
        Ok(Self {
            history: Some(scene.history().weak()),
            standalone_history: None,
            effect_depth: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        })
    }
    pub fn prepare(
        &mut self,
        context: &mut PreRenderContext,
        layers: &HigurashiLayers,
        transition: &HigurashiDefaultTransition,
    ) -> Result<()> {
        let history = self
            .standalone_history
            .get_or_insert_with(ScreenHistory::default)
            .clone();
        let size = context.resize_source.canvas_handle().get();
        history.bind(context.device, context.queue, [size.width, size.height])?;
        self.history = Some(history.weak());
        self.prepare_groups(context, layers, Some(transition), &history)
    }
    pub fn prepare_scene(
        &mut self,
        context: &mut PreRenderContext,
        scene: &HigurashiScene,
    ) -> Result<()> {
        let size = context.resize_source.canvas_handle().get();
        scene.bind_history(context.device, context.queue, [size.width, size.height])?;
        self.standalone_history = None;
        self.history = Some(scene.history().weak());
        self.prepare_groups(context, scene.layers(), scene.transition(), scene.history())
    }
    fn prepare_groups(
        &mut self,
        context: &mut PreRenderContext,
        layers: &HigurashiLayers,
        transition: Option<&HigurashiDefaultTransition>,
        history: &ScreenHistory,
    ) -> Result<()> {
        let parent = layers.root_transform();
        layers.prepare_movies(context)?;
        if let Some(transition) = transition {
            transition.previous.prepare_movies(context)?;
        }
        history.with_graphics(|graphics| -> Result<()> {
            if let Some(transition) = transition {
                // Native allocates/captures outgoing1488 BEFORE allocating
                // incoming1490. Both use the stored constructor size/depth.
                graphics.ensure_outgoing();
                transition
                    .previous
                    .prepare_group(context, &parent, &self.effect_depth)?;
                {
                    let mut pass = context.begin_pass(
                        graphics.outgoing.as_mut().unwrap().as_texture_target(),
                        Some(DepthStencilTarget {
                            view: &graphics.depth_view,
                        }),
                        "Higurashi outgoing internal group",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                    transition
                        .previous
                        .render_capture_group(&mut pass, &parent)?;
                }
                super::framebuffer::apply(
                    context,
                    graphics.outgoing.as_mut().unwrap(),
                    &transition.previous.screen,
                );
                graphics.ensure_incoming();
                layers.prepare_front_group(context, &parent, &self.effect_depth)?;
                {
                    let mut pass = context.begin_pass(
                        graphics.incoming.as_mut().unwrap().as_texture_target(),
                        Some(DepthStencilTarget {
                            view: &graphics.depth_view,
                        }),
                        "Higurashi incoming internal group",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                    layers.render_front_capture(&mut pass, &parent)?;
                }
                super::framebuffer::apply(
                    context,
                    graphics.incoming.as_mut().unwrap(),
                    layers.front_properties(),
                );
                let (current, previous) = graphics.images.current_and_previous();
                {
                    let mut pass = context.begin_pass(
                        current.texture.as_texture_target(),
                        Some(DepthStencilTarget {
                            view: &graphics.depth_view,
                        }),
                        "Higurashi current1498 wiper/history",
                    );
                    // b9cc8..b9cd8 resets the same constructor depth/stencil
                    // after both group captures, before the default wiper.
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                    transition.render(
                        &mut pass,
                        RenderRequestBuilder::new(),
                        graphics.incoming.as_ref().unwrap().as_texture_source(),
                        graphics.outgoing.as_ref().unwrap().as_texture_source(),
                    );
                    if let Some(previous) = previous {
                        if let Some(packet) =
                            super::temporal::AfterimagePacket::new(layers.screen_properties())
                        {
                            packet.render(&mut pass, previous.texture.as_texture_source());
                        }
                    }
                }
                super::framebuffer::apply(
                    context,
                    &mut current.texture,
                    layers.screen_properties(),
                );
                current.known_pixels = true;
            } else {
                graphics.retire_planes();
                layers.prepare_front_group(context, &parent, &self.effect_depth)?;
                let (current, previous) = graphics.images.current_and_previous();
                {
                    let mut pass = context.begin_pass(
                        current.texture.as_texture_target(),
                        Some(DepthStencilTarget {
                            view: &graphics.depth_view,
                        }),
                        "Higurashi direct current1498 group",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
                    layers.render_front_capture(&mut pass, &parent)?;
                }
                super::framebuffer::apply(context, &mut current.texture, layers.front_properties());
                if let Some(previous) = previous {
                    if let Some(packet) =
                        super::temporal::AfterimagePacket::new(layers.screen_properties())
                    {
                        let mut pass = context.begin_pass(
                            current.texture.as_texture_target(),
                            None,
                            "Higurashi current1498 history",
                        );
                        packet.render(&mut pass, previous.texture.as_texture_source());
                    }
                }
                super::framebuffer::apply(
                    context,
                    &mut current.texture,
                    layers.screen_properties(),
                );
                current.known_pixels = true;
            }
            Ok(())
        })??;
        Ok(())
    }
    fn current_history(&self) -> Result<ScreenHistory> {
        self.history
            .as_ref()
            .and_then(ScreenHistory::from_weak)
            .ok_or_else(|| anyhow::anyhow!("Higurashi Screen capture has no live resource owner"))
    }
    /// b9508 retains current1498 regardless of which image was last prepared.
    pub(crate) fn modal_capture_ready(&self) -> bool {
        self.current_history()
            .is_ok_and(|history| history.is_bound())
    }
    pub fn capture_modal(&self, context: &mut PreRenderContext) -> Result<RenderTexture> {
        self.current_history()?.with_graphics(|graphics| {
            super::modal::capture::capture(context, &graphics.images.current().texture)
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_capture_modal_with_extent(
        &self,
        context: &mut PreRenderContext,
        extent: (u32, u32),
    ) -> Result<RenderTexture> {
        self.current_history()?.with_graphics(|graphics| {
            super::modal::capture::capture_with_extent(
                context,
                &graphics.images.current().texture,
                extent,
            )
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_capture_current(
        &self,
        context: &mut PreRenderContext,
    ) -> Result<RenderTexture> {
        self.current_history()?.capture_current(context)
    }
    pub fn render(&self, pass: &mut RenderPass, _transition: &HigurashiDefaultTransition) {
        self.render_current(pass, 1)
            .expect("Higurashi Screen must be bound before its outer draw");
    }
    pub fn render_scene(&self, pass: &mut RenderPass, scene: &HigurashiScene) {
        self.render_screen(pass, scene, 1);
    }
    pub fn render_runner(
        &self,
        pass: &mut RenderPass,
        runner: &super::runner::HigurashiRunner,
    ) -> Result<()> {
        self.render_frame(pass, runner.frame())?;
        if let Some(overlay) = runner.reconstruction_overlay() {
            overlay.render(pass);
        }
        Ok(())
    }
    pub fn capture_frame(
        &mut self,
        context: &mut PreRenderContext,
        runner: &super::runner::HigurashiRunner,
    ) -> Result<RenderTexture> {
        self.capture_frame_with_overlay(context, runner, None)
    }
    pub fn capture_frame_with_overlay(
        &mut self,
        context: &mut PreRenderContext,
        runner: &super::runner::HigurashiRunner,
        _overlay: Option<&dyn super::touch::Overlay>,
    ) -> Result<RenderTexture> {
        // Actual14d30 calls6c928 thena9510; no child update/preparation/draw.
        // Child0 is present only if it was part of the last admitted prepare.
        Ok(runner.frame().cache.capture(context))
    }
    pub(crate) fn prepare_frame_with_overlay(
        &mut self,
        context: &mut PreRenderContext,
        frame: &super::frame::HigurashiFrame,
        overlay: Option<&dyn super::touch::Overlay>,
    ) -> Result<()> {
        // Native6d200 prepares children BEFORE attaching the composed cache.
        self.prepare_scene(context, &frame.scene)?;
        frame.cache.prepare(context, |pass| {
            self.render_frame_with_overlay(pass, frame, overlay)
        })
    }
    pub(crate) fn render_cached_frame(
        &self,
        pass: &mut RenderPass,
        frame: &super::frame::HigurashiFrame,
        stencil: u8,
    ) {
        frame.cache.render(pass, stencil);
    }
    fn render_current(&self, pass: &mut RenderPass, stencil: u8) -> Result<()> {
        let projection = multiply_transforms(
            HigurashiTransform::default().compute_native_final_transform(),
            glam::Mat4::from_translation(Vec3::new(-640., -720., 0.)),
        );
        let vertices =
            [(0., 0.), (1280., 0.), (0., 720.), (1280., 720.)].map(|(x, y)| PosTexVertex {
                position: Vec2::new(x, y),
                texture_position: Vec2::new(x / 1280., y / 720.),
            });
        self.current_history()?.with_graphics(|graphics| {
            pass.run(
                RenderRequestBuilder::new()
                    .depth_stencil_shorthand(stencil, false, false)
                    .build(
                        RenderProgramWithArguments::HigurashiPostprocess {
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
                            texture: graphics.images.current().texture.as_texture_source(),
                            transform: HigurashiTransform::convert_clip_depth(projection),
                            color: FloatColor4::WHITE,
                            operation: LayerFragmentShader::Default,
                            param: Vec4::ZERO,
                        },
                        DrawPrimitive::TrianglesStrip,
                    ),
            );
        })
    }
    pub(super) fn render_screen(&self, pass: &mut RenderPass, scene: &HigurashiScene, stencil: u8) {
        if !scene.layers().screen_properties().is_visible() {
            return;
        }
        // The caller's parent/root transform and tint are already represented
        // in1498; ba554's outer program6 draws it with literalwhite/strictstencil.
        self.render_current(pass, stencil)
            .expect("Higurashi Screen must be bound before its outer draw");
    }
    /// Native screen ba554 draws on pass1 (opaque), message84f6c on pass0
    /// (transparent). The message composes with the outer identity, independently
    /// of ScreenLayer's root properties and offscreen wipe.
    pub fn render_frame(
        &self,
        pass: &mut RenderPass,
        frame: &super::frame::HigurashiFrame,
    ) -> Result<()> {
        self.render_frame_with_overlay(pass, frame, None)
    }
    pub fn render_frame_with_overlay(
        &self,
        pass: &mut RenderPass,
        frame: &super::frame::HigurashiFrame,
        overlay: Option<&dyn super::touch::Overlay>,
    ) -> Result<()> {
        use super::frame::Child;
        use shin_render::PassKind;
        let layers = frame.scene.layers();
        let plan = super::frame::render_plan_with_overlay_owner(
            layers.message_properties().values()[5],
            layers.screen_properties().values()[5],
            overlay.map(|o| (o.priority(), o.owner_id())),
        );
        for draw in plan {
            match (draw.child, draw.pass) {
                (Child::Screen, PassKind::Opaque) => {
                    self.render_screen(pass, &frame.scene, draw.stencil);
                }
                (Child::Message, PassKind::Transparent) => {
                    let transform = frame
                        .scene
                        .layers()
                        .message_properties()
                        .composed_transform(&Default::default());
                    // Message84f6c has no generic68abc visibility test.
                    frame.message.render(pass, &transform, draw.stencil)?;
                }
                (Child::Overlay, _) => {
                    if let Some(overlay) = overlay {
                        overlay.render(pass, draw.stencil, draw.pass)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}
