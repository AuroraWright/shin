//! Native per-slot replacement wrapper (7e5e0), including lower-layer captures.
use super::*;
use crate::{
    layer::{
        owner::{LayerOwner, LayerOwnerRead, LayerPropertyRead, LayerPropertyWrite, LayerStorage},
        render_layers,
    },
    render::render_texture_holder::RenderTextureHolder,
    wiper::{AnyWiper, Wiper as _},
};

pub(super) struct SlotLayer<T> {
    update_tracker: crate::layer::update_tracker::LayerUpdateTracker,
    pub target: LayerStorage<T>,
    pub(super) source: Option<LayerOwner<SlotLayer<T>>>,
    pub(super) wiper: Option<LayerOwner<AnyWiper>>,
    pub(super) source_texture: RenderTextureHolder,
    pub(super) target_texture: RenderTextureHolder,
}

/// Scoped views keep nested source locks and their actual owners alive. Fixtures
/// never return a bare reference obtained from a temporary owner guard.
#[cfg(test)]
pub(crate) struct ReferenceLayerLeaf<'a, T>(ReferenceLeafState<'a, T>);
#[cfg(test)]
enum ReferenceLeafState<'a, T> {
    Borrowed(&'a T),
    Owned(LayerOwnerRead<SlotLayer<T>>),
    Target(LayerOwnerRead<T>),
}
#[cfg(test)]
impl<'a, T> ReferenceLayerLeaf<'a, T> {
    pub(crate) fn borrowed(layer: &'a T) -> Self {
        Self(ReferenceLeafState::Borrowed(layer))
    }
}
#[cfg(test)]
impl<T> std::ops::Deref for ReferenceLayerLeaf<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        match &self.0 {
            ReferenceLeafState::Borrowed(layer) => layer,
            ReferenceLeafState::Owned(layer) => layer.target.direct().unwrap(),
            ReferenceLeafState::Target(layer) => layer,
        }
    }
}

impl<T: DrawableLayer + shin_render::shaders::types::RenderClone>
    shin_render::shaders::types::RenderClone for SlotLayer<T>
{
    fn render_clone(&self, context: &mut shin_render::shaders::types::RenderCloneCtx) -> Self {
        // Native7f9d0 clones the source before the target. Nested retained
        // sources can rebind an original owner's property pointer while copying.
        let source = self.source.render_clone(context);
        let mut target = self.target.render_clone(context);
        if let (Some(original_source), Some(copied_source)) = (&self.source, &source) {
            // Native7fc20..7fc9c accesses both original property objects after
            // cloning, then repairs only this source/target pair.7fd34 also
            // rebinds retained targets; a whole-tree alias map changes semantics.
            let source_properties = original_source.read().target.properties().share();
            let target_properties = self.target.properties().share();
            if source_properties.identity() == target_properties.identity() {
                target.set_properties(copied_source.read().target.properties().share());
            }
        }
        Self {
            target,
            update_tracker: Default::default(),
            source,
            wiper: self.wiper.render_clone(context),
            // Native7fa50/7fa54 leaves the copied slot's capture handles null;
            // each replacement wrapper prepares its own scratch targets later.
            source_texture: RenderTextureHolder::new("SlotLayer/source"),
            target_texture: RenderTextureHolder::new("SlotLayer/target"),
        }
    }
}

impl<T> SlotLayer<T> {
    #[cfg(test)]
    pub(super) fn reference_owner_leaves<'a>(
        &'a self,
        leaves: &mut Vec<ReferenceLayerLeaf<'a, T>>,
    ) {
        if let Some(source) = &self.source {
            Self::reference_owned_leaves(source, leaves);
        }
        match &self.target {
            LayerStorage::Direct(target) => leaves.push(ReferenceLayerLeaf::borrowed(target)),
            LayerStorage::Shared(target) => leaves.push(ReferenceLayerLeaf(
                ReferenceLeafState::Target(target.read_owned()),
            )),
        }
    }
    #[cfg(test)]
    pub(super) fn reference_owned_leaves<'a>(
        owner: &LayerOwner<Self>,
        leaves: &mut Vec<ReferenceLayerLeaf<'a, T>>,
    ) {
        let layer = owner.read_owned();
        if let Some(source) = &layer.source {
            Self::reference_owned_leaves(source, leaves);
        }
        match &layer.target {
            LayerStorage::Shared(target) => leaves.push(ReferenceLayerLeaf(
                ReferenceLeafState::Target(target.read_owned()),
            )),
            LayerStorage::Direct(_) => {
                leaves.push(ReferenceLayerLeaf(ReferenceLeafState::Owned(layer)))
            }
        }
    }
    #[cfg(test)]
    pub(super) fn reference_transition_words(&self, words: &mut Vec<[u32; 7]>) {
        if let Some(wiper) = &self.wiper {
            let wiper = wiper.read();
            let AnyWiper::Default(wipe) = &*wiper else {
                panic!("reference Default wipe")
            };
            let [current, total] = wipe.reference_time_words();
            words.push([
                self.source
                    .as_ref()
                    .map_or(0, |s| if s.read().wiper.is_some() { 2 } else { 1 }),
                1,
                1,
                current,
                total,
                u32::from(self.source_texture.get().is_some()),
                u32::from(self.target_texture.get().is_some()),
            ]);
        } else {
            assert!(self.wiper.is_none());
        }
        if let Some(source) = &self.source {
            source.read().reference_transition_words(words);
        }
    }
    #[cfg(test)]
    pub(super) fn reference_advance_clocks(&mut self, delta: f32) {
        if let Some(wiper) = &self.wiper {
            let mut wiper = wiper.write();
            let AnyWiper::Default(wipe) = &mut *wiper else {
                panic!("reference Default wipe")
            };
            wipe.advance_ticks(delta);
        } else {
            assert!(self.wiper.is_none());
        }
        if let Some(source) = &mut self.source {
            source.write().reference_advance_clocks(delta);
        }
    }
    #[cfg(test)]
    pub(super) fn reference_prepare_textures(
        &mut self,
        device: &wgpu::Device,
        resize: &shin_render::resize::SurfaceResizeSource,
    ) -> usize {
        let mut count = 0;
        if self.wiper.is_some() {
            for holder in [&mut self.source_texture, &mut self.target_texture] {
                assert!(holder.get().is_none());
                *holder.as_inner_mut() = Some(shin_render::render_texture::RenderTexture::new(
                    device.clone(),
                    resize.canvas_handle(),
                    "Populated slot ownership reference".into(),
                ));
                count += 1;
            }
        }
        if let Some(source) = &mut self.source {
            count += source.write().reference_prepare_textures(device, resize);
        }
        count
    }
    pub fn new(target: T, source: Option<Self>, wiper: Option<AnyWiper>) -> Self {
        Self::new_stored(
            LayerStorage::new_shared(target),
            source.map(LayerOwner::new),
            wiper,
        )
    }
    pub(super) fn new_stored(
        target: LayerStorage<T>,
        source: Option<LayerOwner<Self>>,
        wiper: Option<AnyWiper>,
    ) -> Self {
        Self {
            update_tracker: Default::default(),
            target,
            source: source.filter(|_| wiper.is_some()),
            wiper: wiper.map(LayerOwner::new),
            source_texture: RenderTextureHolder::new("SlotLayer/source"),
            target_texture: RenderTextureHolder::new("SlotLayer/target"),
        }
    }

    pub fn is_transition_active(&self) -> bool {
        self.wiper.is_some()
    }
}

impl<T: Layer> SlotLayer<T> {
    pub(super) fn can_batch_with(&self, next: &Self) -> bool {
        let Some(wipe) = &self.wiper else {
            return false;
        };
        // Native7ca30 refuses an active nested source on either side.
        if self
            .source
            .as_ref()
            .is_some_and(|s| s.read().is_transition_active())
            || next
                .source
                .as_ref()
                .is_some_and(|s| s.read().is_transition_active())
        {
            return false;
        }
        match &next.wiper {
            Some(other) => wipe.read().can_batch_with(&other.read()),
            None => !next.target.needs_lower_capture(),
        }
    }

    pub(super) fn batch_layer(&self, source: bool) -> Option<&dyn Layer> {
        if source && self.wiper.is_some() {
            self.source.as_ref().map(|layer| layer as &dyn Layer)
        } else {
            Some(&self.target)
        }
    }

    pub(super) fn pre_render_owned_batch_layer(
        &mut self,
        source: bool,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[crate::layer::owner::RetainedLayer],
    ) -> bool {
        if source && self.wiper.is_some() {
            self.source
                .as_mut()
                .is_some_and(|layer| layer.pre_render_with_owned_lower(context, transform, lower))
        } else {
            self.target
                .pre_render_with_owned_lower(context, transform, lower)
        }
    }
    pub(super) fn pre_render_batch_layer(
        &mut self,
        source: bool,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[&dyn Layer],
    ) -> bool {
        if source && self.wiper.is_some() {
            self.source
                .as_mut()
                .is_some_and(|layer| layer.pre_render_with_lower(context, transform, lower))
        } else {
            self.target.pre_render_with_lower(context, transform, lower)
        }
    }

    pub fn pre_render_with_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[&dyn Layer],
    ) -> bool {
        if self.wiper.is_none() {
            return self.target.pre_render_with_lower(context, transform, lower);
        }
        let source_consumed = self
            .source
            .as_mut()
            .is_some_and(|source| source.pre_render_with_lower(context, transform, lower));
        let target_consumed = self.target.pre_render_with_lower(context, transform, lower);

        self.capture_prepared_sides(context, transform, lower, source_consumed, target_consumed)
    }

    pub fn pre_render_with_owned_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[crate::layer::owner::RetainedLayer],
    ) -> bool {
        if self.wiper.is_none() {
            return self
                .target
                .pre_render_with_owned_lower(context, transform, lower);
        }
        let source_consumed = self
            .source
            .as_mut()
            .is_some_and(|source| source.pre_render_with_owned_lower(context, transform, lower));
        let target_consumed = self
            .target
            .pre_render_with_owned_lower(context, transform, lower);
        let borrowed = lower
            .iter()
            .map(|layer| layer as &dyn Layer)
            .collect::<Vec<_>>();
        self.capture_prepared_sides(
            context,
            transform,
            &borrowed,
            source_consumed,
            target_consumed,
        )
    }

    fn capture_prepared_sides(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[&dyn Layer],
        source_consumed: bool,
        target_consumed: bool,
    ) -> bool {
        // Native 7f1d4/7f418 copies the lower-layer list and appends the old/new
        // layer. Capturing only the character would fade its transparency to black.
        let source_texture = self.source_texture.get_or_init(context);
        {
            let mut pass = context.begin_pass(
                source_texture.as_texture_target(),
                Some(context.depth_stencil),
                "SlotLayer/source",
            );
            pass.clear(None, Some(0), None);
            let mut layers = if source_consumed {
                Vec::new()
            } else {
                lower.to_vec()
            };
            if let Some(source) = &self.source {
                layers.push(source);
            }
            render_layers(&mut pass, transform, &layers, FloatColor4::BLACK, 0);
        }
        let target_texture = self.target_texture.get_or_init(context);
        {
            let mut pass = context.begin_pass(
                target_texture.as_texture_target(),
                Some(context.depth_stencil),
                "SlotLayer/target",
            );
            pass.clear(None, Some(0), None);
            let mut layers = if target_consumed {
                Vec::new()
            } else {
                lower.to_vec()
            };
            layers.push(&self.target);
            render_layers(&mut pass, transform, &layers, FloatColor4::BLACK, 0);
        }
        // Native7f708 returns false: the wrapper retains its outer lower batch.
        false
    }
}

impl<T: AdvUpdatable> AdvUpdatable for SlotLayer<T> {
    fn update(&mut self, context: &AdvUpdateContext) {
        if !self.update_tracker.update(context.frame_id) {
            return;
        }
        if let Some(source) = &mut self.source {
            // Completion makes this node delegate directly to its target. Keep
            // its stable allocation rather than updating the target again while
            // flattening, as the native 7ea20 path specifically avoids doing so.
            source.update(context);
        }
        self.target.update(context);
        if let Some(wiper) = &mut self.wiper {
            wiper.update(context);
            if !wiper.is_running() {
                self.wiper = None;
                self.source = None;
                self.source_texture.clear();
                self.target_texture.clear();
            }
        }
    }
}

impl<T: Layer> Layer for SlotLayer<T> {
    fn pre_render_with_owned_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[crate::layer::owner::RetainedLayer],
    ) -> bool {
        SlotLayer::pre_render_with_owned_lower(self, context, transform, lower)
    }
    fn needs_lower_capture(&self) -> bool {
        self.wiper.is_some() || self.target.needs_lower_capture()
    }
    fn pre_render_with_lower(
        &mut self,
        context: &mut PreRenderContext,
        transform: &TransformParams,
        lower: &[&dyn Layer],
    ) -> bool {
        SlotLayer::pre_render_with_lower(self, context, transform, lower)
    }
    fn fast_forward(&mut self) {
        if let Some(source) = &mut self.source {
            source.fast_forward();
        }
        self.target.fast_forward();
        if let Some(wiper) = &mut self.wiper {
            wiper.fast_forward();
        }
    }
    fn get_stencil_bump(&self) -> u8 {
        if self.wiper.is_some() {
            1
        } else {
            self.target.get_stencil_bump()
        }
    }
    fn pre_render(&mut self, context: &mut PreRenderContext, transform: &TransformParams) {
        self.pre_render_with_lower(context, transform, &[]);
    }
    fn render(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if let Some(wiper) = &self.wiper {
            if pass_kind == PassKind::Opaque {
                wiper.render(
                    pass,
                    RenderRequestBuilder::new().depth_stencil_shorthand(stencil_ref, true, false),
                    self.target_texture.get().unwrap().as_texture_source(),
                    self.source_texture.get().unwrap().as_texture_source(),
                );
            }
        } else {
            self.target.render(pass, transform, stencil_ref, pass_kind);
        }
    }
}

impl<T: DrawableLayer> SlotLayer<T> {
    #[cfg(test)]
    pub(super) fn property_trace(&self, property: LayerProperty, output: &mut Vec<(usize, f32)>) {
        if let Some(source) = &self.source {
            source.read().property_trace(property, output);
        }
        output.push((
            self.target.properties().identity(),
            self.target.properties().get_value(property),
        ));
    }
}

impl<T: DrawableLayer> SlotLayer<T> {
    pub(super) fn render_order_key(&self) -> (f32, i32) {
        self.target.render_order_key()
    }
    fn properties(&self) -> LayerPropertyRead<'_, T> {
        self.target.properties()
    }
    fn properties_mut(&mut self) -> LayerPropertyWrite<'_, T> {
        self.target.properties_mut()
    }
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod native_copy_tests;

#[cfg(test)]
mod native_batch_render_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::user::TileLayer;
    use crate::wiper::DefaultWiper;
    use shin_core::primitives::update::FrameId;
    use shin_core::time::Tween;
    use shin_render::shaders::types::RenderClone;

    fn tile(color: FloatColor4, full: bool, position: f32) -> TileLayer {
        let mut tile = TileLayer::new(
            color,
            if full {
                glam::vec4(-960.0, -540.0, 1920.0, 1080.0)
            } else {
                glam::vec4(-400.0, -200.0, 800.0, 400.0)
            },
        );
        tile.properties_mut()
            .property_tweener_mut(LayerProperty::RenderPosition)
            .fast_forward_to(position);
        tile.properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .enqueue_now(60.0, Tween::linear(Ticks::from_f32(60.0)));
        tile
    }
    fn motions(layer: &SlotLayer<TileLayer>, output: &mut Vec<f32>) {
        if let Some(source) = &layer.source {
            motions(&source.read(), output);
        }
        output.push(
            layer
                .target
                .properties()
                .get_value(LayerProperty::TranslateX),
        );
    }
    #[test]
    fn immediate_install_releases_interrupted_source_and_slot_access_targets_new_layer() {
        let slot = LayerbankId::new(10);
        let mut group = LayerGroup::new(None);
        group.add_layer(slot, tile(FloatColor4::WHITE, false, 1000.0));
        group.add_layer_with_wiper(
            slot,
            tile(FloatColor4::BLACK, false, 1200.0),
            Some(AnyWiper::Default(DefaultWiper::new(Ticks::from_f32(10.0)))),
        );
        assert!(group.has_wiper_for_layerbank(slot));
        assert_eq!(
            group
                .get_layer(slot)
                .unwrap()
                .properties()
                .get_value(LayerProperty::RenderPosition),
            1200.0
        );
        group.add_layer(slot, tile(FloatColor4::WHITE, false, 1000.0));
        assert!(!group.has_wiper_for_layerbank(slot));
        assert!(group.layers[0].layer.read().source.is_none());
        assert!(group.remove_layer(slot, Ticks::ZERO).is_some());
        assert!(group.get_layer(slot).is_none());
    }
    #[test]
    fn retained_nested_source_survives_bank_replacement_and_group_removal() {
        let bank = LayerbankId::new(0);
        let mut group = LayerGroup::new(None);
        group.add_layer(bank, tile(FloatColor4::BLUE, false, 1000.0));
        for color in [FloatColor4::RED, FloatColor4::WHITE] {
            group.add_layer_with_wiper(
                bank,
                tile(color, false, 1000.0),
                Some(DefaultWiper::new(Ticks::from_f32(10.0)).into()),
            );
        }
        let source = group.layers[0]
            .layer
            .read()
            .source
            .as_ref()
            .unwrap()
            .clone();
        let outer_weak = source.reference_weak();
        let inner_weak = source.read().source.as_ref().unwrap().reference_weak();
        let identity = source.reference_identity();
        let mut captured = source.retain();
        assert_eq!(captured.reference_identity(), identity);
        source
            .write()
            .target
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(17.0);
        assert_eq!(
            source
                .read()
                .target
                .properties()
                .get_value(LayerProperty::TranslateX),
            17.0
        );
        // A fresh layer in the old bank must not redirect the retained capture.
        group.add_layer(bank, tile(FloatColor4::BLACK, false, 1000.0));
        assert_eq!(outer_weak(), 2);
        assert_eq!(inner_weak(), 1);
        captured.fast_forward();
        assert_eq!(
            source
                .read()
                .source
                .as_ref()
                .unwrap()
                .read()
                .target
                .properties()
                .get_value(LayerProperty::TranslateX),
            60.0
        );
        assert_eq!(
            group
                .get_layer(bank)
                .unwrap()
                .properties()
                .get_value(LayerProperty::TranslateX),
            0.0
        );
        drop(group.remove_layer(bank, Ticks::ZERO));
        drop(group);
        // An owned read view safely outlives both typed and erased handles.
        let view = source.read_owned();
        drop(source);
        drop(captured);
        assert_eq!(outer_weak(), 1);
        assert_eq!(inner_weak(), 1);
        assert_eq!(
            view.target
                .properties()
                .get_value(LayerProperty::TranslateX),
            17.0
        );
        drop(view);
        assert_eq!(outer_weak(), 0);
        assert_eq!(inner_weak(), 0);
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_slot_replacement_update_capture_and_opaque_draw_contract() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x7a750, 0xc8dffea8u32),
            (0x7a834, 0x94000f6bu32),
            (0x7a9c4, 0x94000f07u32),
            (0x7ea54, 0x91012276u32),
            (0x7ea68, 0xf9400908u32),
            (0x7ea74, 0xd63f0100u32),
            (0x7eabc, 0x91016108u32),
            (0x7eac4, 0xb50005c8u32),
            (0x7ead4, 0x91014108u32),
            (0x7eb98, 0x91014268u32),
            (0x7eba4, 0xeb0a013fu32),
            (0x7eba8, 0x540000e0u32),
            (0x7ebc0, 0xd63f0100u32),
            (0x7ebc4, 0x91016275u32),
            (0x7ebe4, 0xd63f0100u32),
            (0x7ebf0, 0xf9401908u32),
            (0x7ebf4, 0xd63f0100u32),
            (0x7ebf8, 0x370003e0u32),
            (0x7ec34, 0x91018268u32),
            (0x7ec54, 0x9101a268u32),
            (0x7ed60, 0xa9be4ff4u32),
            (0x7edf0, 0x91016008u32),
            (0x7edfc, 0x320003e0u32),
            (0x7ef20, 0x36000423u32),
            (0x7ef28, 0x320003e1u32),
            (0x7ef38, 0xc8dffd14u32),
            (0x7ef40, 0xc8dffd00u32),
            (0x7ef50, 0xc8dffd00u32),
            (0x7ef68, 0xaa1503e1u32),
            (0x7f1d4, 0x910023e0u32),
            (0x7f1d8, 0xaa1503e1u32),
            (0x7f1dc, 0x940003f1u32),
            (0x7f2dc, 0x9101a297u32),
            (0x7f39c, 0x321e03e0u32),
            (0x7f3a0, 0x32081fe1u32),
            (0x7f3b4, 0x910023e1u32),
            (0x7f3c0, 0x97ffc8c8u32),
            (0x7f418, 0x910023e0u32),
            (0x7f41c, 0xaa1503e1u32),
            (0x7f420, 0x94000360u32),
            (0x7f520, 0x91018296u32),
            (0x7f5e0, 0x321e03e0u32),
            (0x7f5e4, 0x32081fe1u32),
            (0x7f5f8, 0x910023e1u32),
            (0x7f604, 0x97ffc837u32),
            (0x7f708, 0x2a1f03e0u32),
        ] {
            let actual = u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
            assert_eq!(actual, expected, "native instruction{address:x}");
        }
    }
    #[test]
    #[ignore = "requires GPU"]
    fn reference_slot_transition_lower_capture_nested_clocks_and_retirement() {
        run_reference_capture_gpu(false, 0);
    }

    #[test]
    #[ignore = "requires Metal;copied populated screen captures rebuild with matching framebuffer pixels;no audio"]
    fn reference_screen_transition_clone_rebuild_and_pixels() {
        run_reference_capture_gpu(true, 0);
    }

    #[test]
    #[ignore = "requires Metal;native adjacent Default-wipe batch capture parity;no audio"]
    fn reference_prepared_adjacent_slot_batch_pixels() {
        run_reference_capture_gpu(false, 1);
    }

    #[test]
    #[ignore = "requires Metal;all native wipe types compose old/new pairs before the shared wipe;no audio"]
    fn reference_all_native_wiper_batch_composition() {
        run_reference_capture_gpu(false, 2);
    }

    #[test]
    #[ignore = "requires Metal;actual owned replacement sides and invisible child retirement;no audio/window/saves"]
    fn reference_owned_slot_capture_source_target_and_retirement() {
        run_reference_capture_gpu(false, 3);
    }

    fn run_reference_capture_gpu(screen_only: bool, batch_mode: u8) {
        use crate::asset::system::{
            AssetLoadContext, AssetServer, LayeredAssetIo, cache::AssetCache,
        };
        use shin_render::{
            dynamic_buffer::DynamicBuffer,
            pipelines::PipelineStorage,
            resize::{SurfaceResizeSource, ViewportParams},
            shaders::types::{
                buffer::BytesAddress,
                texture::{
                    DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind,
                },
            },
        };
        crate::init_test_task_pools();
        let instance = wgpu::Instance::default();
        let adapter =
            shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) = shin_tasks::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                required_features: if adapter.get_info().backend == wgpu::Backend::Vulkan {
                    adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS
                } else {
                    wgpu::Features::empty()
                },
                ..Default::default()
            }))
        .unwrap();
        let io = LayeredAssetIo::new();

        let server = std::sync::Arc::new(AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Umineko,
                higurashi_bustup_cache: Default::default(),
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        ));
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            1920, 1080,
        )));
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Title reference depth"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
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
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Title reference target"),
            size: wgpu::Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let samplers = TextureSamplerStore::new(&device);
        let mut pipelines = PipelineStorage::new(device.clone(), format);
        let mut dynamic = DynamicBuffer::new(device.clone(), BytesAddress::new(1024 * 1024));
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut context = PreRenderContext {
            breakup_texture: None,
            device: &device,
            queue: &queue,
            resize_source: &resize,
            sampler_store: &samplers,
            depth_stencil: DepthStencilTarget { view: &depth_view },
            pipeline_storage: &mut pipelines,
            dynamic_buffer: &mut dynamic,
            encoder: &mut encoder,
        };

        if batch_mode == 3 {
            for empty_recapture in [false, true] {
                let child = LayerOwner::new(UserLayer::from(tile(FloatColor4::BLUE, true, 1000.0)));
                let child_weak = child.reference_weak();
                let hidden_effect = || {
                    let mut effect = crate::layer::user::EffectLayer::new();
                    effect
                        .properties_mut()
                        .property_tweener_mut(LayerProperty::ShowLayer)
                        .fast_forward_to(0.0);
                    UserLayer::from(effect)
                };
                let mut slot = SlotLayer::new(
                    hidden_effect(),
                    Some(SlotLayer::new(hidden_effect(), None, None)),
                    Some(DefaultWiper::new(Ticks::from_f32(10.0)).into()),
                );
                let lower = vec![child.retain()];
                assert!(!slot.pre_render_with_owned_lower(
                    &mut context,
                    &TransformParams::default(),
                    &lower
                ));
                drop(lower);
                assert_eq!(child_weak(), 3); // graph child plus both invisible Effects
                let copy = slot.render_clone(&mut context.render_clone_ctx());
                let mut leaves = Vec::new();
                copy.reference_owner_leaves(&mut leaves);
                for leaf in &leaves {
                    let UserLayer::Effect(effect) = &**leaf else {
                        unreachable!()
                    };
                    assert_eq!(effect.reference_pending_owner_count(), 0);
                }
                drop(leaves);
                let mut retained_source = slot.source.as_ref().unwrap().clone();
                let source_weak = retained_source.reference_weak();
                drop(child);
                assert_eq!(child_weak(), 2);
                drop(slot); // target retires; the actual old source stays retained
                assert_eq!(source_weak(), 1);
                assert_eq!(child_weak(), 1);
                drop(copy);
                if empty_recapture {
                    assert!(retained_source.pre_render_with_owned_lower(
                        &mut context,
                        &TransformParams::default(),
                        &[]
                    ));
                    assert_eq!(child_weak(), 0);
                }
                drop(retained_source);
                assert_eq!(source_weak(), 0);
                assert_eq!(child_weak(), 0);
            }
            context.dynamic_buffer.finish(context.encoder);
            let finished = std::mem::replace(
                context.encoder,
                device.create_command_encoder(&Default::default()),
            )
            .finish();
            queue.submit([finished]);
            device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
            context.dynamic_buffer.recall();
            eprintln!(
                "Owned replacement source/target: invisible pending children, empty copied lists, source detach, empty recapture/destruction and four final owner retirements passed"
            );
            return;
        }

        if screen_only {
            crate::layer::ScreenLayer::assert_reference_nested_transitions(&mut context, &server);
            eprintln!(
                "Copied nested screen captures: empty cloned scratch holders rebuild through production pre-render;full framebuffer pixels match the source;completion retirement and independent clocks pass"
            );
            return;
        }

        if batch_mode == 2 {
            super::native_batch_render_tests::run(&mut context, &view, &target);
            return;
        }

        if batch_mode == 1 {
            let fixture_tile = |id: usize, position: f32| {
                let i = id as f32;
                let mut layer = TileLayer::new(
                    FloatColor4::from_rgba(0.125 * i, 0.25, 0.5, 1.0),
                    glam::vec4(-32.0 * i, 16.0 * i, 96.0, 48.0),
                );
                layer
                    .properties_mut()
                    .property_tweener_mut(LayerProperty::RenderPosition)
                    .fast_forward_to(position);
                layer
            };
            let mut group = LayerGroup::new(None);
            for (bank, old, new, position) in [(1, 1, 2, 2000.0), (5, 3, 4, 1000.0)] {
                let bank = LayerbankId::new(bank);
                group.add_layer(bank, fixture_tile(old, position));
                group.add_layer_with_wiper(
                    bank,
                    fixture_tile(new, position),
                    Some(AnyWiper::Default(DefaultWiper::new(Ticks::from_f32(30.0)))),
                );
                group
                    .layers
                    .iter_mut()
                    .find(|item| item.layerbank_id == Some(bank))
                    .unwrap()
                    .layer
                    .write()
                    .reference_advance_clocks(7.5);
            }
            group.pre_render(&mut context, &TransformParams::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Native adjacent batch reference",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                crate::layer::render_layer_without_bg(
                    &mut pass,
                    &TransformParams::default(),
                    &group,
                    0,
                );
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            // Native7ca30/806b0 produce pairs(1,2),(3,4),then7efc0 captures
            // the complete old list and complete new list before one wipe.
            let sample = |ids: [usize; 2], x: f32, y: f32| {
                let mut color = [0.0, 0.0, 0.0];
                for id in ids {
                    let i = id as f32;
                    if x >= -32.0 * i
                        && x < -32.0 * i + 96.0
                        && y >= 16.0 * i
                        && y < 16.0 * i + 48.0
                    {
                        color = [0.125 * i, 0.25, 0.5];
                    }
                }
                color
            };
            let mut mismatches = 0;
            for y in 0..1080 {
                for x in 0..1920 {
                    let old = sample([1, 3], x as f32 + 0.5 - 960.0, y as f32 + 0.5 - 540.0);
                    let new = sample([2, 4], x as f32 + 0.5 - 960.0, y as f32 + 0.5 - 540.0);
                    let pixel = &pixels[(y * 1920 + x) * 4..][..4];
                    let wrong = (0..3).any(|c| {
                        (f32::from(pixel[c]) - 255.0 * (old[c] * 0.75 + new[c] * 0.25)).abs() > 2.0
                    });
                    mismatches += usize::from(wrong || pixel[3] != 255);
                }
            }
            eprintln!(
                "Native adjacent batch: {mismatches} mismatching pixels of2073600;probe={:?}",
                &pixels[((540 + 55) * 1920 + (960 - 60)) * 4..][..4]
            );
            assert_eq!(
                mismatches, 0,
                "native old/new lists must be composed before the shared wipe"
            );
            return;
        }

        let slot = LayerbankId::new(10);
        let mut group = LayerGroup::new(None);
        group.add_layer(
            LayerbankId::new(1),
            tile(FloatColor4::from_rgba(0.0, 0.0, 1.0, 1.0), true, 1001.0),
        );
        group.add_layer(
            slot,
            tile(FloatColor4::from_rgba(1.0, 1.0, 1.0, 0.5), false, 1000.0),
        );
        for (color, duration) in [
            (FloatColor4::from_rgba(1.0, 0.0, 0.0, 0.5), 10.0),
            (FloatColor4::from_rgba(0.0, 1.0, 0.0, 0.5), 20.0),
        ] {
            group.add_layer_with_wiper(
                slot,
                tile(color, false, 1000.0),
                Some(AnyWiper::Default(DefaultWiper::new(Ticks::from_f32(
                    duration,
                )))),
            );
        }
        let mut foreground = TileLayer::new(
            FloatColor4::from_rgba(1.0, 0.0, 0.0, 0.5),
            glam::vec4(650.0, -200.0, 80.0, 400.0),
        );
        foreground
            .properties_mut()
            .property_tweener_mut(LayerProperty::RenderPosition)
            .fast_forward_to(1000.0);
        group.add_layer(LayerbankId::new(0), foreground);
        let mut elapsed = 0.0;
        let mut property_elapsed = 0.0;
        let mut frame = FrameId::default();
        let mut previous = None;
        for (delta, allowed) in [
            (0.0, true),
            (2.0, true),
            (50.0, false),
            (-1.0, true),
            (4.0, true),
            (5.0, true),
            (10.0, true),
        ] {
            frame.advance();
            group.update(&AdvUpdateContext {
                frame_id: frame,
                delta_ticks: Ticks::from_f32(delta),
                asset_server: &server,
                device: &device,
                queue: &queue,
                are_animations_allowed: allowed,
                voice_callback: None,
                movie_volume_settings: Default::default(),
            });
            if allowed {
                elapsed += delta;
                property_elapsed += delta.max(0.0);
            }
            let current = group.layers[2].layer.read();
            let mut values = Vec::new();
            motions(&current, &mut values);
            assert_eq!(
                values.len(),
                if elapsed < 10.0 {
                    3
                } else if elapsed < 20.0 {
                    2
                } else {
                    1
                }
            );
            assert!(
                values
                    .iter()
                    .all(|value| (*value - property_elapsed).abs() < 0.001),
                "duplicate update: {values:?}"
            );
            assert_eq!(
                group.layers[1]
                    .layer
                    .read()
                    .target
                    .properties()
                    .get_value(LayerProperty::TranslateX),
                property_elapsed
            );
            drop(current);
            assert_eq!(group.has_wiper_for_layerbank(slot), elapsed < 20.0);
            group.pre_render(&mut context, &TransformParams::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Slot reference",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                crate::layer::render_layer_without_bg(
                    &mut pass,
                    &TransformParams::default(),
                    &group,
                    0,
                );
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let a = (elapsed / 10.0).min(1.0);
            let b = (elapsed / 20.0).min(1.0);
            let expected = [
                127.5 * (1.0 - b),
                127.5 * ((1.0 - a) * (1.0 - b) + b),
                127.5 * ((1.0 - a) * (1.0 - b) + 1.0),
            ];
            for y in 440..640 {
                for x in 760..1160 {
                    let pixel = &pixels[(y * 1920 + x) * 4..][..4];
                    for channel in 0..3 {
                        assert!(
                            (f32::from(pixel[channel]) - expected[channel]).abs() <= 2.0,
                            "elapsed{elapsed}, pixel{pixel:?}, expected{expected:?}"
                        );
                    }
                    assert_eq!(pixel[3], 255);
                }
            }
            assert_eq!(&pixels[(100 * 1920 + 100) * 4..][..4], &[0, 0, 255, 255]);
            let foreground_pixel = &pixels[(500 * 1920 + 1650) * 4..][..4];
            for (channel, expected) in foreground_pixel[..3].iter().zip([128u8, 0, 128]) {
                assert!(
                    channel.abs_diff(expected) <= 1,
                    "Foreground included in lower capture"
                );
            }
            if !allowed {
                assert_eq!(previous.as_ref().unwrap(), &pixels);
            }
            previous = Some(pixels);
            if elapsed == 2.0 && allowed {
                let mut clone = group.render_clone(&mut context.render_clone_ctx());
                let copied_slot = clone.layers[2].layer.read();
                assert!(copied_slot.source_texture.get().is_none());
                assert!(copied_slot.target_texture.get().is_none());
                assert!(
                    copied_slot
                        .source
                        .as_ref()
                        .unwrap()
                        .read()
                        .source_texture
                        .get()
                        .is_none()
                );
                assert!(
                    copied_slot
                        .source
                        .as_ref()
                        .unwrap()
                        .read()
                        .target_texture
                        .get()
                        .is_none()
                );
                assert!(group.layers[2].layer.read().source_texture.get().is_some());
                assert!(group.layers[2].layer.read().target_texture.get().is_some());
                drop(copied_slot);
                clone.pre_render(&mut context, &TransformParams::default());
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view: &view,
                            kind: TextureTargetKind::Screen,
                        },
                        Some(context.depth_stencil),
                        "Slot copied reference",
                    );
                    pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
                    crate::layer::render_layer_without_bg(
                        &mut pass,
                        &TransformParams::default(),
                        &clone,
                        0,
                    );
                }
                assert_eq!(
                    previous.as_ref().unwrap(),
                    &crate::layer::user::readback_test_texture(&mut context, &target)
                );
                clone.fast_forward();
                assert!(clone.has_wiper_for_layerbank(slot));
                clone.update(&AdvUpdateContext {
                    frame_id: frame,
                    delta_ticks: Ticks::ZERO,
                    asset_server: &server,
                    device: &device,
                    queue: &queue,
                    are_animations_allowed: false,
                    voice_callback: None,
                    movie_volume_settings: Default::default(),
                });
                assert!(!clone.has_wiper_for_layerbank(slot));
                assert!(clone.layers[2].layer.read().source.is_none());
                assert!(clone.layers[2].layer.read().source_texture.get().is_none());
                assert!(clone.layers[2].layer.read().target_texture.get().is_none());
                assert!(group.has_wiper_for_layerbank(slot));
            }
        }
        let finished = group.layers[2].layer.read();
        assert!(finished.source.is_none());
        assert!(finished.source_texture.get().is_none());
        assert!(finished.target_texture.get().is_none());
        eprintln!(
            "Slot captures: 560000 analytical RGB/alpha pixels; lower siblings preserved, signed clocks, paused pixel equality, nested once-only updates, render cloning and retirement pass"
        );
    }
}
