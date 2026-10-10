use replace_with::replace_with;
use shin_core::primitives::color::{FloatColor4, UnormColor};
use shin_render::{
    PassKind, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{
        RenderClone, RenderCloneCtx,
        texture::{DepthStencilTarget, TextureTarget},
    },
};
use tracing::debug;

use crate::{
    layer::{
        DrawableLayer, Layer, NewDrawableLayer,
        either::EitherLayer,
        new_drawable_layer::{NewDrawableLayerNeedsSeparatePass, NewDrawableLayerState},
        page_layer::PageLayer,
        properties::LayerProperties,
        render_layer,
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
    },
    render::{PreRenderContext, render_texture_holder::RenderTextureHolder},
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::{AnyWiper, Wiper as _},
};

struct TransitionLayer {
    update_tracker: crate::layer::update_tracker::LayerUpdateTracker,
    source_layer: Option<EitherLayer<Box<PageLayer>, Box<TransitionLayer>>>,
    target_layer: Option<PageLayer>,
    wiper: Option<AnyWiper>,

    source_render_texture: RenderTextureHolder,
    target_render_texture: RenderTextureHolder,
}

impl RenderClone for TransitionLayer {
    fn render_clone(&self, context: &mut RenderCloneCtx) -> Self {
        // Native e71f0 copies source, target and wipe, but e725c/e7260 leave
        // both scratch capture handles empty for the copy's next pre-render.
        Self {
            update_tracker: Default::default(),
            source_layer: self.source_layer.render_clone(context),
            target_layer: self.target_layer.render_clone(context),
            wiper: self.wiper.render_clone(context),
            source_render_texture: RenderTextureHolder::new(
                "TransitionLayer/source_render_texture",
            ),
            target_render_texture: RenderTextureHolder::new(
                "TransitionLayer/target_render_texture",
            ),
        }
    }
}

impl TransitionLayer {
    pub fn new(
        source: Option<Box<TransitionLayer>>,
        target: PageLayer,
        wiper: Option<AnyWiper>,
    ) -> Self {
        Self {
            update_tracker: Default::default(),
            // Native immediate install (0xe5e44) releases the previous scene.
            source_layer: if wiper.is_some() {
                source.map(EitherLayer::Right)
            } else {
                None
            },
            target_layer: Some(target),
            wiper,
            source_render_texture: RenderTextureHolder::new(
                "TransitionLayer/source_render_texture",
            ),
            target_render_texture: RenderTextureHolder::new(
                "TransitionLayer/target_render_texture",
            ),
        }
    }

    pub fn dummy() -> Self {
        Self {
            update_tracker: Default::default(),
            source_layer: None,
            target_layer: None,
            wiper: None,
            source_render_texture: RenderTextureHolder::new(
                "TransitionLayer/source_render_texture",
            ),
            target_render_texture: RenderTextureHolder::new(
                "TransitionLayer/target_render_texture",
            ),
        }
    }

    #[cfg(test)]
    fn collect_voice_positions(&self, parent: &TransformParams, positions: &mut Vec<(i32, f32)>) {
        if self.wiper.is_some() {
            if let Some(source) = &self.source_layer {
                match source {
                    EitherLayer::Left(page) => page.collect_voice_positions(parent, positions),
                    EitherLayer::Right(transition) => {
                        transition.collect_voice_positions(parent, positions)
                    }
                }
            }
        }
        self.get_target_layer()
            .collect_voice_positions(parent, positions);
    }

    pub fn is_transition_active(&self) -> bool {
        self.wiper.is_some()
    }

    pub fn get_target_layer(&self) -> &PageLayer {
        self.target_layer.as_ref().unwrap()
    }

    pub fn get_target_layer_mut(&mut self) -> &mut PageLayer {
        self.target_layer.as_mut().unwrap()
    }

    pub fn into_target_layer(self) -> PageLayer {
        self.target_layer.unwrap()
    }
}

impl AdvUpdatable for TransitionLayer {
    fn update(&mut self, context: &AdvUpdateContext) {
        if !self.update_tracker.update(context.frame_id) {
            return;
        }
        if let Some(source_layer) = &mut self.source_layer {
            source_layer.update(context);

            // Native 0xe6978 updates the source once before checking completion.
            // Updating again here would accelerate every nested page and wipe.
            // If we are storing a finished transition layer, flatten the hierarchy.
            if let EitherLayer::Right(transition_layer) = source_layer {
                if !transition_layer.is_transition_active() {
                    debug!("Inner TransitionLayer has finished running, flattening hierarchy");
                    let Some(EitherLayer::Right(transition_layer)) = self.source_layer.take()
                    else {
                        unreachable!()
                    };
                    self.source_layer = Some(EitherLayer::Left(Box::new(
                        transition_layer.into_target_layer(),
                    )));
                }
            }
        }

        self.get_target_layer_mut().update(context);

        if let Some(wiper) = &mut self.wiper {
            wiper.update(context);
            if !wiper.is_running() {
                debug!("Wiper has finished running, cleaning up & switching to direct rendering");
                self.wiper = None;
                self.source_layer = None;
                self.source_render_texture.clear();
                self.target_render_texture.clear();
            }
        }
    }
}

impl Layer for TransitionLayer {
    fn fast_forward(&mut self) {
        // Native 0xe6c20 forwards source, target, then wiper; update retires them.
        if let Some(source_layer) = &mut self.source_layer {
            source_layer.fast_forward();
        }
        self.get_target_layer_mut().fast_forward();
        if let Some(wiper) = &mut self.wiper {
            wiper.fast_forward();
        }
    }

    fn get_stencil_bump(&self) -> u8 {
        if self.wiper.is_some() {
            return 1;
        }

        self.get_target_layer().get_stencil_bump()
    }

    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        if self.wiper.is_none() {
            self.get_target_layer_mut().pre_render(context, transform);
            return;
        }

        self.source_layer
            .as_mut()
            .unwrap()
            .pre_render(context, transform);
        self.target_layer
            .as_mut()
            .unwrap()
            .pre_render(context, transform);

        let source_render_texture = self.source_render_texture.get_or_init(context);

        {
            let mut pass = context.begin_pass(
                source_render_texture.as_texture_target(),
                Some(context.depth_stencil),
                "TransitionLayer/source",
            );
            pass.clear(None, Some(0), None);

            render_layer(
                &mut pass,
                transform,
                self.source_layer.as_mut().unwrap(),
                FloatColor4::BLACK,
                0,
            );
        }

        let target_render_texture = self.target_render_texture.get_or_init(context);
        {
            let mut pass = context.begin_pass(
                target_render_texture.as_texture_target(),
                Some(context.depth_stencil),
                "TransitionLayer/target",
            );
            pass.clear(None, Some(0), None);

            render_layer(
                &mut pass,
                transform,
                self.target_layer.as_mut().unwrap(),
                FloatColor4::BLACK,
                0,
            );
        }
    }

    #[tracing::instrument(skip_all)]
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        let Some(wiper) = &self.wiper else {
            self.get_target_layer()
                .render(pass, transform, stencil_ref, pass_kind);
            return;
        };

        if pass_kind == PassKind::Opaque {
            let render_request_builder =
                RenderRequestBuilder::new().depth_stencil_shorthand(stencil_ref, true, false);

            wiper.render(
                pass,
                render_request_builder,
                self.target_render_texture
                    .get()
                    .unwrap()
                    .as_texture_source(),
                self.source_render_texture
                    .get()
                    .unwrap()
                    .as_texture_source(),
            );
        }
    }
}

pub struct ScreenLayer {
    update_tracker: crate::layer::update_tracker::LayerUpdateTracker,
    active_layer: TransitionLayer,
    pending_layer: Option<PageLayer>,

    plane_count: usize,

    new_drawable_state: NewDrawableLayerState,
    props: LayerProperties,
}

impl RenderClone for ScreenLayer {
    fn render_clone(&self, context: &mut RenderCloneCtx) -> Self {
        // Nativee50c0 copies the drawable base before active and pending pages.
        let props = self.props.clone();
        let new_drawable_state = self.new_drawable_state.render_clone(context);
        let active_layer = self.active_layer.render_clone(context);
        let pending_layer = self.pending_layer.render_clone(context);
        Self {
            update_tracker: self.update_tracker.clone(),
            active_layer,
            pending_layer,
            plane_count: self.plane_count,
            new_drawable_state,
            props,
        }
    }
}

impl ScreenLayer {
    #[cfg(test)]
    pub(crate) fn reference_pages(&self) -> impl Iterator<Item = &PageLayer> {
        fn collect<'a>(transition: &'a TransitionLayer, pages: &mut Vec<&'a PageLayer>) {
            if let Some(source) = &transition.source_layer {
                match source {
                    EitherLayer::Left(page) => pages.push(page),
                    EitherLayer::Right(transition) => collect(transition, pages),
                }
            }
            pages.push(transition.get_target_layer());
        }
        let mut pages = Vec::new();
        collect(&self.active_layer, &mut pages);
        pages.extend(self.pending_layer.iter());
        pages.into_iter()
    }
    #[cfg(test)]
    pub(crate) fn reference_transition_words(&self) -> Vec<[u32; 7]> {
        fn collect(transition: &TransitionLayer, words: &mut Vec<[u32; 7]>) {
            let Some(crate::wiper::AnyWiper::Default(wipe)) = &transition.wiper else {
                assert!(transition.wiper.is_none());
                return;
            };
            let source_kind = match &transition.source_layer {
                None => 0,
                Some(EitherLayer::Left(_)) => 1,
                Some(EitherLayer::Right(source)) => {
                    if source.wiper.is_some() {
                        2
                    } else {
                        1
                    }
                }
            };
            let [current, total] = wipe.reference_time_words();
            words.push([
                source_kind,
                u32::from(transition.target_layer.is_some()),
                1,
                current,
                total,
                u32::from(transition.source_render_texture.get().is_some()),
                u32::from(transition.target_render_texture.get().is_some()),
            ]);
            if let Some(EitherLayer::Right(source)) = &transition.source_layer {
                collect(source, words);
            }
        }
        let mut words = Vec::new();
        collect(&self.active_layer, &mut words);
        words
    }
    #[cfg(test)]
    pub(crate) fn reference_prepare_transition_textures(
        &mut self,
        device: &wgpu::Device,
        resize: &shin_render::resize::SurfaceResizeSource,
    ) -> usize {
        fn prepare(
            transition: &mut TransitionLayer,
            device: &wgpu::Device,
            resize: &shin_render::resize::SurfaceResizeSource,
        ) -> usize {
            let mut count = 0;
            if transition.wiper.is_some() {
                for holder in [
                    &mut transition.source_render_texture,
                    &mut transition.target_render_texture,
                ] {
                    assert!(holder.get().is_none());
                    *holder.as_inner_mut() = Some(shin_render::render_texture::RenderTexture::new(
                        device.clone(),
                        resize.canvas_handle(),
                        "Screen capture ownership reference".into(),
                    ));
                    count += 1;
                }
            }
            if let Some(EitherLayer::Right(source)) = &mut transition.source_layer {
                count += prepare(source, device, resize);
            }
            count
        }
        prepare(&mut self.active_layer, device, resize)
    }
    #[cfg(test)]
    pub(crate) fn reference_advance_transition_clocks(&mut self, delta: f32) {
        fn advance(transition: &mut TransitionLayer, delta: f32) {
            if let Some(crate::wiper::AnyWiper::Default(wipe)) = &mut transition.wiper {
                wipe.advance_ticks(delta);
            } else {
                assert!(transition.wiper.is_none());
            }
            if let Some(EitherLayer::Right(source)) = &mut transition.source_layer {
                advance(source, delta);
            }
        }
        advance(&mut self.active_layer, delta);
    }
    #[cfg(test)]
    pub(crate) fn reference_dissolve_seed(&self) -> u32 {
        self.new_drawable_state.reference_dissolve_seed()
    }
    #[cfg(test)]
    pub(crate) fn reference_effect_phases(&self) -> [f32; 3] {
        self.new_drawable_state.reference_effect_phases()
    }
    #[cfg(test)]
    pub(crate) fn collect_voice_positions(
        &self,
        parent: &TransformParams,
        positions: &mut Vec<(i32, f32)>,
    ) {
        let transform = self.props.get_composed_transform_params(parent);
        self.active_layer
            .collect_voice_positions(&transform, positions);
    }

    pub fn new(plane_count: usize) -> Self {
        // Nativee4f1c consumes the screen seed before9e860 builds its page.
        let new_drawable_state = NewDrawableLayerState::new();
        Self {
            update_tracker: Default::default(),
            // NB: the original game stores __either__ a `TransitionLayer` or `PageLayer` here
            // however, after the first transition, the `TransitionLayer` stays
            // I just decided to take a shorter route and always store `TransitionLayer` :)
            active_layer: TransitionLayer::new(None, PageLayer::new(plane_count), None),
            pending_layer: None,
            plane_count,
            new_drawable_state,
            props: LayerProperties::new(),
        }
    }

    pub fn page_layer(&self) -> &PageLayer {
        if let Some(pending_layer) = &self.pending_layer {
            pending_layer
        } else {
            self.active_layer.get_target_layer()
        }
    }

    pub fn page_layer_mut(&mut self) -> &mut PageLayer {
        if let Some(pending_layer) = &mut self.pending_layer {
            pending_layer
        } else {
            self.active_layer.get_target_layer_mut()
        }
    }

    pub fn pageback(&mut self, ctx: &mut RenderCloneCtx, start_anew: bool) {
        self.pending_layer = Some(if start_anew {
            // Native 0xe5718..0xe5738 constructs a fresh page with plane_count.
            PageLayer::new(self.plane_count)
        } else {
            self.active_layer.get_target_layer().render_clone(ctx)
        });

        // NB: the original engine iterates over plane `LayerGroup`s and calls `LayerGroup::stop_transition` on them
        // We do not implement `LayerGroup`-level transitions, so this is skipped
    }

    pub fn apply_transition(&mut self, wiper: Option<AnyWiper>) {
        let Some(pending_layer) = self.pending_layer.take() else {
            return;
        };
        replace_with(
            &mut self.active_layer,
            TransitionLayer::dummy,
            |prev_transition_layer| {
                TransitionLayer::new(Some(Box::new(prev_transition_layer)), pending_layer, wiper)
            },
        );
    }

    pub fn is_transition_active(&self) -> bool {
        self.active_layer.is_transition_active()
    }
}

struct ScreenLayerNewDrawableDelegate<'a> {
    active_layer: &'a TransitionLayer,
}

impl NewDrawableLayerNeedsSeparatePass for ScreenLayerNewDrawableDelegate<'_> {
    fn needs_separate_pass(&self, props: &LayerProperties) -> bool {
        props.get_clip_mode() != DrawableClipMode::None
            || props.is_fragment_shader_nontrivial()
            || props.is_blending_nontrivial()
    }
}

impl NewDrawableLayer for ScreenLayerNewDrawableDelegate<'_> {
    #[tracing::instrument(skip_all)]
    fn render_drawable_indirect(
        &mut self,
        context: &mut PreRenderContext,
        props: &LayerProperties,
        target: TextureTarget,
        depth_stencil: DepthStencilTarget,
        transform: &TransformParams,
    ) -> PassKind {
        let mut pass = context.begin_pass(target, Some(depth_stencil), "ScreenLayer/indirect");

        if !props.is_visible() {
            pass.clear(Some(UnormColor::BLACK), None, None);
        } else {
            let self_transform = props.get_composed_transform_params(transform);

            pass.clear(None, Some(0), None);
            // NB:     ^ doesn't clear color here
            // this is unlike PageLayer and LayerGroup, which do

            render_layer(
                &mut pass,
                &self_transform,
                self.active_layer,
                FloatColor4::BLACK,
                0,
            );
        }

        PassKind::Transparent
    }

    fn render_drawable_direct(
        &self,
        _pass: &mut RenderPass,
        _transform: &TransformParams,
        _drawable: &DrawableParams,
        _clip: &DrawableClipParams,
        _stencil_ref: u8,
        _pass_kind: PassKind,
    ) {
        // direct rendering is always done by the ScreenLayer without relying on NewDrawableLayer
        unreachable!()
    }
}

impl AdvUpdatable for ScreenLayer {
    fn update(&mut self, context: &AdvUpdateContext) {
        if !self.update_tracker.update(context.frame_id) {
            return;
        }
        self.props.update(context);
        self.new_drawable_state.update(context, &self.props);

        self.active_layer.update(context);
        if let Some(pending_layer) = &mut self.pending_layer {
            pending_layer.update(context);
        }
    }
}

impl Layer for ScreenLayer {
    fn fast_forward(&mut self) {
        self.props.fast_forward();
        self.active_layer.fast_forward();
        if let Some(target_layer) = &mut self.pending_layer {
            target_layer.fast_forward();
        }
    }

    fn get_stencil_bump(&self) -> u8 {
        self.active_layer.get_stencil_bump()
    }

    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        let props = &self.props;

        let self_transform = props.get_composed_transform_params(transform);

        self.active_layer.pre_render(context, &self_transform);

        let mut delegate = ScreenLayerNewDrawableDelegate {
            active_layer: &self.active_layer,
        };

        self.new_drawable_state
            .pre_render(context, props, &mut delegate, transform);
    }

    #[tracing::instrument(skip_all)]
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        let props = &self.props;
        if self.new_drawable_state.try_finish_indirect_render(
            props,
            pass,
            transform,
            stencil_ref,
            pass_kind,
        ) {
            return;
        }

        if !props.is_visible() {
            return;
        }

        let self_transform = props.get_composed_transform_params(transform);
        self.active_layer
            .render(pass, &self_transform, stencil_ref, pass_kind);
    }
}

impl DrawableLayer for ScreenLayer {
    fn properties(&self) -> &LayerProperties {
        &self.props
    }

    fn properties_mut(&mut self) -> &mut LayerProperties {
        &mut self.props
    }
}

#[cfg(test)]
impl ScreenLayer {
    pub(crate) fn assert_reference_nested_transitions(
        context: &mut PreRenderContext,
        server: &std::sync::Arc<crate::asset::system::AssetServer>,
    ) {
        use shin_core::{
            primitives::update::FrameId,
            time::{Ticks, Tween},
            vm::command::types::{LayerProperty, LayerbankId, PlaneId},
        };
        use shin_render::shaders::types::texture::TextureTargetKind;
        fn page(color: FloatColor4) -> PageLayer {
            let mut page = PageLayer::new(1);
            page.get_plane_mut(PlaneId::new(0)).add_layer(
                LayerbankId::new(0),
                crate::layer::user::TileLayer::new(
                    color,
                    glam::vec4(-960.0, -540.0, 1920.0, 1080.0),
                )
                .into(),
            );
            page.properties_mut()
                .property_tweener_mut(LayerProperty::TranslateX)
                .enqueue_now(600.0, Tween::linear(Ticks::from_f32(60.0)));
            page
        }
        fn motions(transition: &TransitionLayer, values: &mut Vec<f32>) {
            values.push(
                transition
                    .get_target_layer()
                    .properties()
                    .get_value(LayerProperty::TranslateX),
            );
            match &transition.source_layer {
                Some(EitherLayer::Left(page)) => {
                    values.push(page.properties().get_value(LayerProperty::TranslateX))
                }
                Some(EitherLayer::Right(transition)) => motions(transition, values),
                None => {}
            }
        }
        let red =
            TransitionLayer::new(None, page(FloatColor4::from_rgba(1.0, 0.0, 0.0, 1.0)), None);
        let mut screen = Self::new(1);
        screen.active_layer = red;
        for (color, duration) in [
            (FloatColor4::from_rgba(0.0, 1.0, 0.0, 1.0), 10.0),
            (FloatColor4::from_rgba(0.0, 0.0, 1.0, 1.0), 20.0),
            (FloatColor4::from_rgba(1.0, 1.0, 0.0, 1.0), 30.0),
        ] {
            screen.pageback(&mut context.render_clone_ctx(), false);
            screen
                .page_layer_mut()
                .get_plane_mut(PlaneId::new(0))
                .add_layer(
                    LayerbankId::new(0),
                    crate::layer::user::TileLayer::new(
                        color,
                        glam::vec4(-960.0, -540.0, 1920.0, 1080.0),
                    )
                    .into(),
                );
            screen.apply_transition(Some(AnyWiper::Default(crate::wiper::DefaultWiper::new(
                Ticks::from_f32(duration),
            ))));
        }
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Nested screen transition clocks"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut frame = FrameId::default();
        let mut elapsed = 0.0f32;
        let mut previous = None;
        for (delta, allowed) in [
            (2.0, true),
            (7.0, false),
            (3.0, true),
            (5.0, true),
            (2.0, true),
            (8.0, true),
            (10.0, true),
            (1.0, true),
        ] {
            frame.advance();
            screen.update(&AdvUpdateContext {
                frame_id: frame,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: server,
                device: context.device,
                queue: context.queue,
                are_animations_allowed: allowed,
                voice_callback: None,
                movie_volume_settings: Default::default(),
            });
            if allowed {
                elapsed += delta;
            }
            let mut values = Vec::new();
            motions(&screen.active_layer, &mut values);
            let depth = if elapsed < 10.0 {
                4
            } else if elapsed < 20.0 {
                3
            } else if elapsed < 30.0 {
                2
            } else {
                1
            };
            assert_eq!(values.len(), depth, "native flattening at{elapsed}");
            for value in values {
                assert!(
                    (value - elapsed * 10.0).abs() < 0.001,
                    "page advanced twice at{elapsed}: {value}"
                );
            }
            screen.pre_render(context, &TransformParams::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Nested transitions",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                crate::layer::render_layer_without_bg(
                    &mut pass,
                    &TransformParams::default(),
                    &screen,
                    0,
                );
            }
            let pixels = crate::layer::user::readback_test_texture(context, &target);
            let a = (elapsed / 10.0).min(1.0);
            let b = (elapsed / 20.0).min(1.0);
            let c = (elapsed / 30.0).min(1.0);
            let expected = [
                ((1.0 - a) * (1.0 - b) * (1.0 - c) + c) * 255.0,
                (a * (1.0 - b) * (1.0 - c) + c) * 255.0,
                b * (1.0 - c) * 255.0,
            ];
            for y in 400..650 {
                for x in 600..1300 {
                    let pixel = &pixels[(y * 1920 + x) * 4..][..4];
                    for channel in 0..3 {
                        assert!(
                            (f32::from(pixel[channel]) - expected[channel]).abs() <= 3.0,
                            "nested blend at{elapsed}: {pixel:?} vs {expected:?}"
                        );
                    }
                    assert_eq!(pixel[3], 255);
                }
            }
            if !allowed {
                assert_eq!(
                    previous.as_ref().unwrap(),
                    &pixels,
                    "paused nested clocks changed pixels"
                );
            }
            previous = Some(pixels);
            assert_eq!(screen.is_transition_active(), elapsed < 30.0);
            if elapsed == 2.0 && allowed {
                // Copied native screen transitions own fresh scratch targets.
                // Rebuild them through production pre-render and compare pixels.
                let mut copied = screen.render_clone(&mut context.render_clone_ctx());
                assert!(
                    copied
                        .reference_transition_words()
                        .iter()
                        .all(|w| w[5] == 0 && w[6] == 0)
                );
                copied.pre_render(context, &TransformParams::default());
                assert!(
                    copied
                        .reference_transition_words()
                        .iter()
                        .all(|w| w[5] == 1 && w[6] == 1)
                );
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view: &view,
                            kind: TextureTargetKind::Screen,
                        },
                        Some(context.depth_stencil),
                        "Copied nested screen scratch rebuild",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                    crate::layer::render_layer_without_bg(
                        &mut pass,
                        &TransformParams::default(),
                        &copied,
                        0,
                    );
                }
                let copied_pixels = crate::layer::user::readback_test_texture(context, &target);
                assert_eq!(
                    previous.as_ref().unwrap(),
                    &copied_pixels,
                    "rebuilt copied transition pixels differ from source"
                );
                let mut skipped = screen.render_clone(&mut context.render_clone_ctx());
                skipped.pageback(&mut context.render_clone_ctx(), false);
                skipped.fast_forward();
                let mut values = Vec::new();
                motions(&skipped.active_layer, &mut values);
                assert_eq!(values, vec![600.0; 4]);
                assert_eq!(
                    skipped
                        .pending_layer
                        .as_ref()
                        .unwrap()
                        .properties()
                        .get_value(LayerProperty::TranslateX),
                    600.0,
                );
                assert!(skipped.is_transition_active());
                assert!(!skipped.active_layer.wiper.as_ref().unwrap().is_running());
                skipped.update(&AdvUpdateContext {
                    frame_id: frame,
                    delta_ticks: Ticks::ZERO,
                    asset_server: server,
                    device: context.device,
                    queue: context.queue,
                    are_animations_allowed: false,
                    voice_callback: None,
                    movie_volume_settings: Default::default(),
                });
                assert!(!skipped.is_transition_active());
                assert!(skipped.active_layer.source_layer.is_none());
                assert!(skipped.active_layer.source_render_texture.get().is_none());
                assert!(skipped.active_layer.target_render_texture.get().is_none());
                skipped.pre_render(context, &TransformParams::default());
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view: &view,
                            kind: TextureTargetKind::Screen,
                        },
                        Some(context.depth_stencil),
                        "Fast-forwarded nested transitions",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                    crate::layer::render_layer_without_bg(
                        &mut pass,
                        &TransformParams::default(),
                        &skipped,
                        0,
                    );
                }
                let pixels = crate::layer::user::readback_test_texture(context, &target);
                for y in 400..650 {
                    for x in 600..1300 {
                        assert_eq!(&pixels[(y * 1920 + x) * 4..][..4], &[255, 255, 0, 255]);
                    }
                }
                eprintln!(
                    "Native nested fast-forward: four active page clocks and pending PAGEBACK finish; zero-delta paused update retires wipes/textures;175000 final yellow pixels pass"
                );
                let mut source_values = Vec::new();
                motions(&screen.active_layer, &mut source_values);
                let mut fresh = screen.render_clone(&mut context.render_clone_ctx());
                fresh.pageback(&mut context.render_clone_ctx(), false);
                assert_eq!(
                    fresh
                        .page_layer_mut()
                        .properties()
                        .get_value(LayerProperty::TranslateX),
                    source_values[0]
                );
                assert_eq!(
                    fresh
                        .page_layer_mut()
                        .get_plane_mut(PlaneId::new(0))
                        .get_used_layerbank_ids(),
                    vec![LayerbankId::new(0)]
                );
                fresh.pageback(&mut context.render_clone_ctx(), true);
                assert_eq!(
                    fresh
                        .page_layer_mut()
                        .properties()
                        .get_value(LayerProperty::TranslateX),
                    0.0
                );
                assert!(
                    fresh
                        .page_layer_mut()
                        .get_plane_mut(PlaneId::new(0))
                        .get_used_layerbank_ids()
                        .is_empty()
                );
                let mut values = Vec::new();
                motions(&fresh.active_layer, &mut values);
                assert_eq!(values, source_values, "PAGEBACK changed the active scene");
                fresh.apply_transition(None);
                assert!(fresh.pending_layer.is_none());
                assert!(fresh.active_layer.source_layer.is_none());
                assert!(!fresh.is_transition_active());
                fresh.pre_render(context, &TransformParams::default());
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view: &view,
                            kind: TextureTargetKind::Screen,
                        },
                        Some(context.depth_stencil),
                        "Fresh PAGEBACK immediate install",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                    crate::layer::render_layer_without_bg(
                        &mut pass,
                        &TransformParams::default(),
                        &fresh,
                        0,
                    );
                }
                let pixels = crate::layer::user::readback_test_texture(context, &target);
                assert!(pixels.chunks_exact(4).all(|pixel| pixel == [0, 0, 0, 255]));
                let mut four_planes = Self::new(4);
                for index in 0..4 {
                    four_planes
                        .page_layer_mut()
                        .get_plane_mut(PlaneId::new(index))
                        .add_layer(
                            LayerbankId::new(index),
                            crate::layer::user::TileLayer::new(
                                FloatColor4::WHITE,
                                glam::vec4(-960.0, -540.0, 1920.0, 1080.0),
                            )
                            .into(),
                        );
                }
                four_planes.pageback(&mut context.render_clone_ctx(), true);
                for index in 0..4 {
                    assert!(
                        four_planes
                            .page_layer_mut()
                            .get_plane_mut(PlaneId::new(index))
                            .get_used_layerbank_ids()
                            .is_empty()
                    );
                    assert_eq!(
                        four_planes
                            .active_layer
                            .get_target_layer_mut()
                            .get_plane_mut(PlaneId::new(index))
                            .get_used_layerbank_ids(),
                        vec![LayerbankId::new(index)]
                    );
                }
                eprintln!(
                    "Native fresh PAGEBACK: clone retains target properties/content, fresh replaces pending with default empty planes, active nested scene stays intact, immediate install drops sources and2073600 black pixels pass; four-plane reset pass"
                );
            }
        }
        assert!(screen.active_layer.source_layer.is_none());
        assert!(screen.active_layer.source_render_texture.get().is_none());
        assert!(screen.active_layer.target_render_texture.get().is_none());
        eprintln!(
            "Native nested screen transitions: four page clocks advance once, three independently timed wipes flatten at10/20/30ticks, paused pixel equality,175000 analytical RGB pixels per phase and render-texture retirement pass"
        );
    }
}

#[cfg(test)]
mod native_tests {
    use super::*;

    #[test]
    fn immediate_transition_releases_nested_sources() {
        use shin_core::time::Ticks;
        let first = TransitionLayer::new(None, PageLayer::new(1), None);
        let interrupted = TransitionLayer::new(
            Some(Box::new(first)),
            PageLayer::new(1),
            Some(AnyWiper::Default(crate::wiper::DefaultWiper::new(
                Ticks::from_f32(30.0),
            ))),
        );
        assert!(interrupted.source_layer.is_some());
        assert!(interrupted.is_transition_active());
        let installed = TransitionLayer::new(Some(Box::new(interrupted)), PageLayer::new(1), None);
        assert!(installed.source_layer.is_none());
        assert!(!installed.is_transition_active());
        assert!(installed.source_render_texture.get().is_none());
        assert!(installed.target_render_texture.get().is_none());
    }

    #[test]
    fn fast_forward_finishes_nested_source_pages_and_wipes() {
        use shin_core::{
            time::{Ticks, Tween},
            vm::command::types::LayerProperty,
        };
        fn page(value: f32) -> PageLayer {
            let mut page = PageLayer::new(1);
            page.properties_mut()
                .property_tweener_mut(LayerProperty::TranslateX)
                .enqueue_now(value, Tween::linear(Ticks::from_f32(60.0)));
            page
        }
        fn check(transition: &TransitionLayer, expected: &mut Vec<f32>) {
            assert_eq!(
                transition
                    .get_target_layer()
                    .properties()
                    .get_value(LayerProperty::TranslateX),
                expected.pop().unwrap(),
            );
            if let Some(wiper) = &transition.wiper {
                assert!(!wiper.is_running());
                // Native fast-forward leaves completion cleanup to update.
                assert!(transition.is_transition_active());
            }
            match &transition.source_layer {
                Some(EitherLayer::Right(source)) => check(source, expected),
                Some(EitherLayer::Left(source)) => assert_eq!(
                    source.properties().get_value(LayerProperty::TranslateX),
                    expected.pop().unwrap(),
                ),
                None => {}
            }
        }
        let mut source = TransitionLayer::new(None, page(100.0), None);
        source.source_layer = Some(EitherLayer::Left(Box::new(page(50.0))));
        for (value, duration) in [(200.0, 10.0), (300.0, 20.0), (400.0, 30.0)] {
            source = TransitionLayer::new(
                Some(Box::new(source)),
                page(value),
                Some(AnyWiper::Default(crate::wiper::DefaultWiper::new(
                    Ticks::from_f32(duration),
                ))),
            );
        }
        source.fast_forward();
        let mut expected = vec![50.0, 100.0, 200.0, 300.0, 400.0];
        check(&source, &mut expected);
        assert!(expected.is_empty());
        source.fast_forward();
        let mut expected = vec![50.0, 100.0, 200.0, 300.0, 400.0];
        check(&source, &mut expected);
        assert!(expected.is_empty());
    }

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_screen_pageback_and_transition_lifecycle() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, expected) in [
            (0xe694c, 0x9100a016u32),
            (0xe6950, 0xc8dffec8),
            (0xe6960, 0xb4000868),
            (0xe6964, 0xc8dffec0),
            (0xe6968, 0xf9400008),
            (0xe696c, 0xf9400908),
            (0xe6970, 0x4ea81d00),
            (0xe6978, 0xd63f0100),
            (0xe697c, 0xc8dffec0),
            (0xe69d4, 0xf9400008),
            (0xe69d8, 0xf9402d08),
            (0xe69dc, 0xd63f0100),
            (0xe69e0, 0x37000380),
            (0xe69f0, 0xf9403109),
            (0xe69fc, 0xd63f0120),
            (0xe6a00, 0xc8dffea0),
            (0xe6a04, 0xc85ffed5),
            (0xe6a08, 0xc808fec0),
            (0xe6a6c, 0x9100c268),
            (0xe6a74, 0x4ea81d00),
            (0xe6a80, 0x97fe0ff0),
            (0xe6a84, 0x9100e275),
            (0xe6a98, 0xf9400908),
            (0xe6a9c, 0x4ea81d00),
            (0xe6aa4, 0xd63f0100),
            (0xe6ab0, 0xf9401908),
            (0xe6ab4, 0xd63f0100),
            (0xe6ab8, 0x370003e0),
            (0xe6abc, 0xc85ffec0),
            (0xe6af4, 0x91012268),
            (0xe6b14, 0x91010268),
            (0xe6c2c, 0x9100a008),
            (0xe6c30, 0xc8dffd09),
            (0xe6c38, 0xb40000a9),
            (0xe6c3c, 0xc8dffd00),
            (0xe6c40, 0xf9400008),
            (0xe6c44, 0xf9401908),
            (0xe6c48, 0xd63f0100),
            (0xe6c4c, 0x9100c268),
            (0xe6c50, 0xc8dffd00),
            (0xe6c54, 0x97fee1b3),
            (0xe6c58, 0x9100e268),
            (0xe6c5c, 0xc8dffd09),
            (0xe6c60, 0xb40000e9),
            (0xe6c64, 0xc8dffd00),
            (0xe6c68, 0xf9400008),
            (0xe6c6c, 0xf9401d01),
            (0xe6c78, 0xd61f0020),
            (0x9f330, 0x97ff5a98),
            (0x9f334, 0xa9464e74),
            (0x9f348, 0xf9401908),
            (0x9f34c, 0xd63f0100),
            (0xe5718, 0x36000481),
            (0xe5724, 0x97fe4e4b),
            (0xe5728, 0xb9405e61),
            (0xe5738, 0x97fee44a),
            (0xe5740, 0x9101a269),
            (0xe5748, 0xc80afd28),
            (0xe57f4, 0xf9403109),
            (0xe5800, 0xd63f0120),
            (0xe5810, 0x97fee640),
            (0xe58b4, 0x9101a273),
            (0xe58bc, 0xc809fe68),
            (0xe59b0, 0x97fee5f0),
            (0xe59c4, 0x97fe579f),
            (0xe5c7c, 0xb40010e8),
            (0xe5c8c, 0xb4000dc8),
            (0xe5e44, 0x9101a289),
            (0xe5e48, 0xc8dffd20),
            (0xe5e50, 0xc85ffd13),
            (0xe5e54, 0xc809fd00),
            (0xe5e7c, 0xd63f0100),
            (0xe5e80, 0xc85ffea8),
            (0xe5e84, 0xc809febf),
            (0x9e968, 0x2a0203f5),
            (0x9e970, 0x97ff5d38),
            (0x9e994, 0x2a1503e1),
            (0x9e9a8, 0x9400007e),
            (0x9e9d0, 0x97ff6c6c),
            (0xc70c, 0x2a1f03e1),
            (0xc710, 0x940363fc),
            (0x1a208, 0x320003e1),
            (0x1a20c, 0x94032d3d),
        ] {
            assert_eq!(word(a), expected, "native nested transition{a:x}");
        }
    }
}
