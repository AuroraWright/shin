//! Native mode-2 menus, purple transcript browsers, and accusation view.
use super::murderer_model::{
    BACK_RECT, FIRST_SUSPECT_ENTRY, MurdererMenu, MurdererModel, MurdererWipeKind, SUBMIT_ENTRY,
    accusation_rect, replay_rect, top_menu_rect,
};
use crate::{
    asset::{system::AssetServer, texture_archive::TextureArchive},
    layer::{
        NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipParams, DrawableParams, TransformParams},
    },
    render::{PreRenderContext, render_texture_holder::RenderTextureHolder},
    update::{AdvUpdatable, AdvUpdateContext},
    wiper::Wiper,
};
use glam::{Mat4, Vec2, Vec4, vec4};
use shin_core::{
    primitives::color::{FloatColor4, UnormColor},
    time::Ticks,
};
use shin_render::{
    ColorBlendType, LayerBlendType, LayerFragmentShader, PassKind, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{
        RenderClone,
        texture::{DepthStencilTarget, TextureTarget},
    },
};
use std::sync::{Arc, Mutex};

// Family-tree connections from native descriptors 0..27 (0x17118b).
const CONNECTIONS: [[u8; 3]; 28] = [
    [3, 1, 28],
    [7, 1, 21],
    [11, 1, 21],
    [3, 2, 26],
    [3, 3, 26],
    [3, 4, 26],
    [3, 5, 30],
    [7, 5, 21],
    [11, 5, 21],
    [3, 6, 26],
    [3, 7, 26],
    [3, 8, 26],
    [3, 9, 30],
    [7, 9, 21],
    [11, 9, 21],
    [3, 10, 26],
    [3, 11, 26],
    [3, 12, 26],
    [3, 13, 22],
    [7, 13, 21],
    [8, 13, 21],
    [9, 13, 21],
    [10, 13, 21],
    [11, 13, 21],
    [1, 15, 42],
    [3, 17, 37],
    [7, 17, 37],
    [11, 17, 37],
];

#[derive(TextureArchive)]
pub struct MurdererTextures {
    #[txa(name = "different_space_1a")]
    background: GpuTexture,
    #[txa(name = "left")]
    left: GpuTexture,
    #[txa(name = "sentaku")]
    sentaku: GpuTexture,
    #[txa(name = "tokutei")]
    tokutei: GpuTexture,
    #[txa(name = "non")]
    non: GpuTexture,
    #[txa(name = "murderer")]
    murderer: GpuTexture,
    #[txa(name = "kla0")]
    kla0: GpuTexture,
    #[txa(name = "nat0")]
    nat0: GpuTexture,
    #[txa(name = "jes0")]
    jes0: GpuTexture,
    #[txa(name = "eva0")]
    eva0: GpuTexture,
    #[txa(name = "hid0")]
    hid0: GpuTexture,
    #[txa(name = "geo0")]
    geo0: GpuTexture,
    #[txa(name = "rud0")]
    rud0: GpuTexture,
    #[txa(name = "kir0")]
    kir0: GpuTexture,
    #[txa(name = "but0")]
    but0: GpuTexture,
    #[txa(name = "ros0")]
    ros0: GpuTexture,
    #[txa(name = "mar1")]
    mar1: GpuTexture,
    #[txa(name = "nan0")]
    nan0: GpuTexture,
    #[txa(name = "gen0")]
    gen0: GpuTexture,
    #[txa(name = "sha0")]
    sha0: GpuTexture,
    #[txa(name = "kan0")]
    kan0: GpuTexture,
    #[txa(name = "goh0")]
    goh0: GpuTexture,
    #[txa(name = "kum0")]
    kum0: GpuTexture,
    #[txa(name = "Ber_Akuwarai2")]
    _ber_akuwarai2: GpuTexture,
    #[txa(name = "Ber_AkuwaraiA1")]
    _ber_akuwaraia1: GpuTexture,
    #[txa(name = "Ber_Body1")]
    _ber_body1: GpuTexture,
    #[txa(name = "Ber_Body2")]
    _ber_body2: GpuTexture,
    #[txa(name = "Ber_Body3")]
    _ber_body3: GpuTexture,
    #[txa(name = "But_BodyA")]
    _but_bodya: GpuTexture,
    #[txa(name = "But_BodyB")]
    _but_bodyb: GpuTexture,
    #[txa(name = "But_FingerB1")]
    _but_fingerb1: GpuTexture,
    #[txa(name = "But_FingerB2")]
    _but_fingerb2: GpuTexture,
    #[txa(name = "But_FutekiB2")]
    _but_futekib2: GpuTexture,
    #[txa(name = "But_KomaruB1")]
    _but_komarub1: GpuTexture,
    #[txa(name = "But_Majime3")]
    _but_majime3: GpuTexture,
    #[txa(name = "back")]
    _back: GpuTexture,
    #[txa(name = "chap_menu")]
    _chap_menu: GpuTexture,
    #[txa(name = "chap_text")]
    _chap_text: GpuTexture,
    #[txa(name = "chapter")]
    _chapter: GpuTexture,
    #[txa(name = "different_space_2d")]
    _different_space_2d: GpuTexture,
    #[txa(name = "hana")]
    _hana: GpuTexture,
    #[txa(name = "help_chars")]
    _help_chars: GpuTexture,
    #[txa(name = "hint_menu")]
    _hint_menu: GpuTexture,
    #[txa(name = "hint_text")]
    _hint_text: GpuTexture,
    #[txa(name = "key")]
    _key: GpuTexture,
    #[txa(name = "line")]
    line: GpuTexture,
    #[txa(name = "plate")]
    _plate: GpuTexture,
    #[txa(name = "right")]
    _right: GpuTexture,
    #[txa(name = "right2")]
    _right2: GpuTexture,
    #[txa(name = "rule_text")]
    _rule_text: GpuTexture,
    #[txa(name = "thumb_1")]
    _thumb_1: GpuTexture,
    #[txa(name = "thumb_1_s")]
    _thumb_1_s: GpuTexture,
    #[txa(name = "thumb_2")]
    _thumb_2: GpuTexture,
    #[txa(name = "thumb_2_s")]
    _thumb_2_s: GpuTexture,
    #[txa(name = "thumb_4")]
    _thumb_4: GpuTexture,
    #[txa(name = "thumb_4_s")]
    _thumb_4_s: GpuTexture,
    #[txa(name = "thumb_56")]
    _thumb_56: GpuTexture,
    #[txa(name = "thumb_56_s")]
    _thumb_56_s: GpuTexture,
    #[txa(name = "thumb_7")]
    _thumb_7: GpuTexture,
    #[txa(name = "thumb_7_s")]
    _thumb_7_s: GpuTexture,
    #[txa(name = "thumb_8")]
    _thumb_8: GpuTexture,
    #[txa(name = "thumb_8_s")]
    _thumb_8_s: GpuTexture,
    #[txa(name = "topmenu")]
    _topmenu: GpuTexture,
}
impl MurdererTextures {
    // Native texture-name pointer table 0x1d91e0, indexed by flags & 0x3f.
    fn suspects(&self) -> [&GpuTexture; 17] {
        [
            &self.kla0, &self.nat0, &self.jes0, &self.eva0, &self.hid0, &self.geo0, &self.rud0,
            &self.kir0, &self.but0, &self.ros0, &self.mar1, &self.nan0, &self.gen0, &self.sha0,
            &self.kan0, &self.goh0, &self.kum0,
        ]
    }
}

#[derive(RenderClone)]
pub struct MurdererAccusationImpl {
    textures: Arc<MurdererTextures>,
    model: Arc<Mutex<MurdererModel>>,
    pages: Arc<super::purple_view::PurplePages>,
    silhouettes: Arc<Vec<Option<Arc<crate::adv::character_view::CharacterTextures>>>>,
    adv_textures: Arc<crate::adv::character_view::CharacterTextures>,
    transition: Arc<parking_lot::Mutex<MurdererTransitionRenderer>>,
}
#[derive(Debug, RenderClone)]
struct MurdererTransitionRenderer {
    serial: Option<u64>,
    #[render_clone(needs_render)]
    source: RenderTextureHolder,
    #[render_clone(needs_render)]
    live: RenderTextureHolder,
    #[render_clone(needs_render)]
    composite: RenderTextureHolder,
    elapsed: f32,
}
impl MurdererTransitionRenderer {
    fn finish_wipe(&mut self) {
        // Native9b1e4..9b230 releases wiper+40 and frozen capture+30;
        // live-child target+38 belongs to the retained layer until destruction.
        self.serial = None;
        self.source.clear();
        self.elapsed = 0.0;
        self.composite.clear();
    }
}
impl Default for MurdererTransitionRenderer {
    fn default() -> Self {
        Self {
            serial: None,
            source: RenderTextureHolder::new("Murderer frozen menu"),
            live: RenderTextureHolder::new("Murderer live menu"),
            composite: RenderTextureHolder::new("Murderer wipe"),
            elapsed: 0.0,
        }
    }
}
impl std::fmt::Debug for MurdererAccusationImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MurdererLayer")
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}
pub type MurdererAccusationView = NewDrawableLayerWrapper<MurdererAccusationImpl>;
pub(crate) struct PreparedMurdererLayer {
    textures: Arc<MurdererTextures>,
    pages: Arc<super::purple_view::PurplePages>,
    silhouettes: Arc<Vec<Option<Arc<crate::adv::character_view::CharacterTextures>>>>,
    adv_textures: Arc<crate::adv::character_view::CharacterTextures>,
    hint_progress: i32,
}
impl PreparedMurdererLayer {
    pub(crate) fn construct(&self, open_accusation: bool) -> MurdererAccusationView {
        MurdererAccusationView::from_retained_plain_layer_constructor(|| {
            let mut model = MurdererModel::new(self.hint_progress);
            if open_accusation {
                model.open_menu(5);
            }
            MurdererAccusationImpl {
                textures: self.textures.clone(),
                model: Arc::new(Mutex::new(model)),
                pages: self.pages.clone(),
                silhouettes: self.silhouettes.clone(),
                adv_textures: self.adv_textures.clone(),
                transition: Arc::new(parking_lot::Mutex::new(
                    MurdererTransitionRenderer::default(),
                )),
            }
        })
    }
}
impl MurdererAccusationView {
    /// Allocate genuine capture holders without rendering or initializing the
    /// lazy properties. The probe keeps no strong owner of either shared object.
    #[cfg(test)]
    pub(crate) fn reference_retirement_probe(
        &self,
        device: &wgpu::Device,
        resize: &shin_render::resize::SurfaceResizeSource,
        extra: bool,
    ) -> Box<dyn Fn() -> bool> {
        let inner = self.inner_ref();
        {
            let mut renderer = inner.transition.lock();
            let capture = |label: &str| {
                shin_render::render_texture::RenderTexture::new(
                    device.clone(),
                    resize.handle(),
                    label.into(),
                )
            };
            *renderer.live.as_inner_mut() = Some(capture("Retained Murderer live"));
            if extra {
                *renderer.source.as_inner_mut() = Some(capture("Retained Murderer frozen"));
                *renderer.composite.as_inner_mut() = Some(capture("Retained Murderer composite"));
            }
        }
        let model = Arc::downgrade(&inner.model);
        let transition = Arc::downgrade(&inner.transition);
        Box::new(move || {
            let alive = model.strong_count() != 0;
            assert_eq!(alive, transition.strong_count() != 0);
            alive
        })
    }

    #[cfg(test)]
    pub(crate) fn reference_open_menu(&self, index: usize) {
        self.inner_ref().model.lock().unwrap().open_menu(index);
    }

    pub fn restore_completed_answer(&self, answer: i32) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .restore_completed_answer(answer);
    }
    #[cfg(test)]
    pub(crate) fn reference_completed_visuals(&self) -> (usize, Vec<usize>, Option<usize>) {
        let model = self.inner_ref().model.lock().unwrap();
        (
            model.battler_sprite_state(),
            (FIRST_SUSPECT_ENTRY..SUBMIT_ENTRY)
                .filter(|&entry| model.suspect_selected(entry))
                .collect(),
            model.accusation_focus,
        )
    }
    pub async fn load(server: &AssetServer, hint_progress: i32) -> Self {
        Self::prepare(server, hint_progress).await.construct(true)
    }
    pub async fn load_menu(server: &AssetServer, hint_progress: i32) -> Self {
        Self::prepare(server, hint_progress).await.construct(false)
    }
    pub(crate) async fn prepare(server: &AssetServer, hint_progress: i32) -> PreparedMurdererLayer {
        let mut silhouettes = Vec::new();
        for name in [
            "kla", "nat", "jes", "", "", "geo", "", "", "but", "", "mar", "nan", "", "sha", "kan",
            "goh", "kum",
        ] {
            silhouettes.push(if name.is_empty() {
                None
            } else {
                Some(
                    server
                        .load(&format!("/chars/m_{name}.txa"))
                        .await
                        .expect("Failed to load purple character image"),
                )
            });
        }
        PreparedMurdererLayer {
            silhouettes: Arc::new(silhouettes),
            adv_textures: server
                .load("/systex.txa")
                .await
                .expect("Failed to load browser scrollbar"),
            textures: server
                .load("/murderer.txa")
                .await
                .expect("Failed to load murderer textures"),
            hint_progress,
            pages: super::purple_view::PurplePages::load(server).await,
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_controller_state(&self) -> (usize, usize, i32) {
        let inner = self.inner_ref();
        (
            Arc::as_ptr(&inner.textures) as usize,
            Arc::as_ptr(&inner.model) as usize,
            inner.model.lock().unwrap().menu_focus as i32,
        )
    }
    #[cfg(test)]
    pub(crate) fn reference_menu_state(&self) -> (super::murderer_model::MurdererMenu, bool) {
        let inner = self.inner_ref();
        let model = inner.model.lock().unwrap();
        (model.menu, model.menu_input_busy())
    }
    #[cfg(test)]
    pub(crate) fn reference_retained_state(&self) -> (usize, f32) {
        let inner = self.inner_ref();
        (
            Arc::as_ptr(&inner.transition) as usize,
            inner.model.lock().unwrap().top_focus_amount(1),
        )
    }
    #[cfg(test)]
    pub(crate) fn reference_wipe_state(&self) -> Option<(u64, f32, f32)> {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .wipe
            .as_ref()
            .map(|wipe| (wipe.serial, wipe.elapsed, wipe.duration))
    }
    #[cfg(test)]
    pub(crate) fn reference_wipe_kind(&self) -> Option<super::murderer_model::MurdererWipeKind> {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .wipe
            .as_ref()
            .map(|wipe| wipe.kind)
    }
    #[cfg(test)]
    pub(crate) fn reference_frozen_pixels(&self, context: &mut PreRenderContext) -> Vec<u8> {
        frozen_pixels(context, &self.inner_ref().transition.lock())
    }
    pub fn scroll_text(&self, delta: f32) {
        let inner = self.inner_ref();
        let mut model = inner.model.lock().unwrap();
        if model.menu == MurdererMenu::Hints {
            model.scroll_hint_list(delta);
            return;
        }
        let Some(index) = model.purple_original_index() else {
            return;
        };
        let page = if model.menu == MurdererMenu::PurpleByChapter {
            &inner.pages.chapters[index]
        } else {
            &inner.pages.persons[index]
        };
        model.scroll_purple_text(delta, page.height);
    }
    pub fn navigate(&self, delta: i32) {
        self.inner_ref().model.lock().unwrap().navigate(delta);
    }
    pub fn handle_controller_input(
        &self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
    ) -> bool {
        self.handle_controller_input_with_scroll(state, 0.0)
    }
    pub fn handle_controller_input_with_scroll(
        &self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
        scroll_axis: f32,
    ) -> bool {
        let inner = self.inner_ref();
        let mut model = inner.model.lock().unwrap();
        let height = model.purple_original_index().map_or(0.0, |index| {
            if model.menu == MurdererMenu::PurpleByChapter {
                inner.pages.chapters[index].height
            } else {
                inner.pages.persons[index].height
            }
        });
        model.handle_controller_input_with_scroll(state, scroll_axis, height)
    }
    pub fn confirm(&self) {
        self.inner_ref().model.lock().unwrap().confirm();
    }
    pub fn back(&self) {
        self.inner_ref().model.lock().unwrap().back();
    }
    pub fn reset(&self) {
        self.inner_ref().model.lock().unwrap().reset();
    }
    pub fn pointer(&self, position: Vec2, clicked: bool) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .pointer(position, clicked);
    }
    pub fn pointer_state(&self, position: Vec2, held: bool) {
        self.pointer_state_with_time(position, held, 0.0);
    }
    pub fn pointer_state_with_time(&self, position: Vec2, held: bool, seconds: f64) {
        let inner = self.inner_ref();
        let mut model = inner.model.lock().unwrap();
        let height = inner.purple_text_height(&model);
        model.pointer_state_with_time(position, held, seconds, height);
        let height = inner.purple_text_height(&model);
        model.set_purple_text_height(height);
    }
    pub fn ignore_pointer_state(&self, held: bool) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .ignore_pointer_state(held);
    }
    pub fn take_sounds(&self) -> Vec<&'static str> {
        self.inner_ref().model.lock().unwrap().take_sounds()
    }
    pub fn advance_animation(&self, ticks: f32) {
        let inner = self.inner_ref();
        let mut model = inner.model.lock().unwrap();
        let height = inner.purple_text_height(&model);
        model.set_purple_text_height(height);
        model.update(ticks);
    }
    #[cfg(test)]
    pub(crate) fn reference_hint_selection(&self) -> (usize, f32) {
        let model = self.inner_ref().model.lock().unwrap();
        (model.menu_focus, model.hint_scroll)
    }
    #[cfg(test)]
    pub(crate) fn reference_set_purple_list_scroll(&self, scroll: f32) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .reference_set_purple_list_scroll(scroll);
    }
    #[cfg(test)]
    pub(crate) fn reference_purple_selection(&self) -> (Option<usize>, f32, f32) {
        let model = self.inner_ref().model.lock().unwrap();
        (
            model.purple_original_index(),
            model.purple_list_scroll,
            model.purple_text_scroll,
        )
    }
    #[cfg(test)]
    pub(crate) fn reference_accusation_selection(&self, entry: usize) -> (Option<usize>, bool) {
        let model = self.inner_ref().model.lock().unwrap();
        (model.accusation_focus, model.suspect_selected(entry))
    }
    pub fn result(&self) -> Option<i32> {
        self.inner_ref().model.lock().unwrap().poll_result()
    }
    pub fn allows_playback_mode_changes(&self) -> bool {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .allows_playback_mode_changes()
    }
}
#[cfg(test)]
fn frozen_pixels(context: &mut PreRenderContext, renderer: &MurdererTransitionRenderer) -> Vec<u8> {
    capture_pixels(context, &renderer.source)
}

#[cfg(test)]
fn capture_pixels(context: &mut PreRenderContext, capture: &RenderTextureHolder) -> Vec<u8> {
    let texture = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Retained Murderer capture readback"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                view: &view,
                kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
            },
            Some(context.depth_stencil),
            "Retained frozen capture copy",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), None);
        QuadVertices::new()
            .with_box(-960.0, -540.0, 960.0, 540.0)
            .render_sprite(
                &mut pass,
                RenderRequestBuilder::new()
                    .depth_stencil_shorthand(1, true, false)
                    .color_blend_type(ColorBlendType::Opaque),
                capture.get().unwrap().as_texture_source(),
                TransformParams::default().compute_final_transform(),
            );
    }
    super::TileLayer::readback_reference_texture(context, &texture)
}

/// Exercise native retained ownership while real menu captures are allocated.
#[cfg(test)]
pub(crate) fn reference_fast_forward(context: &mut PreRenderContext, server: &Arc<AssetServer>) {
    use crate::layer::{DrawableLayer, Layer};
    use shin_core::{primitives::update::FrameId, time::Tween, vm::command::types::LayerProperty};
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Murderer fast-forward frames"),
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
    let draw = |context: &mut PreRenderContext, layer: &mut MurdererAccusationView| {
        layer.pre_render(context, &TransformParams::default());
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: shin_render::shaders::types::texture::TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Murderer fast-forward presentation",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), None);
            for kind in [PassKind::Opaque, PassKind::Transparent] {
                layer.render(&mut pass, &TransformParams::default(), 1, kind);
            }
        }
        super::TileLayer::readback_reference_texture(context, &target)
    };
    for kind in 0..7 {
        let mut actual = shin_tasks::block_on(MurdererAccusationView::load_menu(server, 20));
        let mut settled = shin_tasks::block_on(MurdererAccusationView::load_menu(server, 20));
        if kind != 0 {
            actual.reference_open_menu(kind - 1);
            settled.reference_open_menu(kind - 1);
        }
        if kind == 2 {
            actual.back();
        } else {
            actual.navigate(1);
            settled.navigate(1);
            settled.advance_animation(1000.0);
        }
        actual.take_sounds();
        actual.fast_forward();
        actual.fast_forward();
        assert!(actual.take_sounds().is_empty());
        assert_eq!(actual.result(), None);
        assert_eq!(
            draw(context, &mut actual),
            draw(context, &mut settled),
            "Menu{kind}: forwarded focus/flash/scroll pixels match completed native tween targets"
        );
    }
    for index in [0, 5] {
        let mut original = shin_tasks::block_on(MurdererAccusationView::load_menu(server, 20));
        original.navigate(index);
        original.confirm();
        for ticks in [7.0, 0.0] {
            original.advance_animation(ticks);
        }
        if index == 5 {
            for ticks in [30.0, 0.0] {
                original.advance_animation(ticks);
            }
        }
        original
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(80.0);
        original
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .enqueue(180.0, Tween::linear(Ticks::from_f32(60.0)));
        original.pre_render(context, &TransformParams::default());
        let frozen = original.reference_frozen_pixels(context);
        let (serial, _, duration) = original.reference_wipe_state().unwrap();
        let physics = original
            .inner_ref()
            .model
            .lock()
            .unwrap()
            .wipe
            .as_ref()
            .unwrap()
            .glass
            .as_ref()
            .map(|g| g.reference_motion());
        let mut alias = original.render_clone(&mut context.render_clone_ctx());
        alias.fast_forward();
        original.fast_forward();
        assert_eq!(
            original.reference_wipe_state(),
            Some((serial, duration, duration))
        );
        assert_eq!(
            original
                .inner_ref()
                .model
                .lock()
                .unwrap()
                .wipe
                .as_ref()
                .unwrap()
                .glass
                .as_ref()
                .map(|g| g.reference_motion()),
            physics
        );
        assert_eq!(
            original.properties().get_value(LayerProperty::TranslateX),
            80.0
        );
        assert!(
            !original
                .properties()
                .property_tweener(LayerProperty::TranslateX)
                .is_idle()
        );
        assert_eq!(original.reference_frozen_pixels(context), frozen);
        let mut settled = shin_tasks::block_on(MurdererAccusationView::load_menu(server, 20));
        settled.reference_open_menu(index as usize);
        settled
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(80.0);
        let expected = draw(context, &mut settled);
        assert_eq!(
            draw(context, &mut alias),
            expected,
            "Completed wipe shows the live staged child before cleanup"
        );
        assert_eq!(original.reference_frozen_pixels(context), frozen);
        assert!(
            original
                .inner_ref()
                .transition
                .lock()
                .source
                .get()
                .is_some()
        );
        let mut frame_id = FrameId::default();
        frame_id.advance();
        alias.update(&AdvUpdateContext {
            frame_id,
            delta_ticks: Ticks::ZERO,
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: false,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        });
        assert!(original.reference_wipe_state().is_none());
        {
            let renderer = original.inner_ref().transition.lock();
            assert!(renderer.source.get().is_none() && renderer.composite.get().is_none());
            assert!(renderer.live.get().is_some());
        }
        assert_eq!(
            draw(context, &mut original),
            expected,
            "Next paused update retires the shared frozen/wiper state without changing the live child pixels"
        );
    }
    println!(
        "Murderer fast-forward: all7menu full-frame comparisons and retained Default/Glass alias transitions pass; staged installation, protected parent property clocks, unchanged Glass physics, frozen capture before cleanup and unchanged live pixels after paused next-update retirement"
    );
}

/// Exercise native retained ownership while real menu captures are allocated.
#[cfg(test)]
pub(crate) fn reference_retained_wipe_ownership(
    context: &mut PreRenderContext,
    server: &Arc<AssetServer>,
) {
    use crate::layer::{DrawableLayer, Layer};
    use shin_core::{primitives::update::FrameId, vm::command::types::LayerProperty};

    for index in [0, 5] {
        let mut original = shin_tasks::block_on(MurdererAccusationView::load_menu(server, 20));
        original.navigate(index);
        original.confirm();
        original.advance_animation(7.0);
        original.advance_animation(0.0);
        if index == 5 {
            // Finish the preliminary ten-tick wipe and thirty-tick hold.
            original.advance_animation(30.0);
            original.advance_animation(0.0);
        }
        let weak_model = Arc::downgrade(&original.inner_ref().model);
        let weak_transition = Arc::downgrade(&original.inner_ref().transition);
        let (serial, duration, source_focus) = {
            let model = original.inner_ref().model.lock().unwrap();
            let wipe = model.wipe.as_ref().expect("Confirmation staged a wipe");
            assert_eq!(
                wipe.kind,
                if index == 5 {
                    MurdererWipeKind::Glass
                } else {
                    MurdererWipeKind::Default
                }
            );
            assert_eq!(wipe.elapsed, 0.0);
            (wipe.serial, wipe.duration, wipe.source.menu_focus)
        };
        original.pre_render(context, &TransformParams::default());
        let initial_frozen_pixels = {
            let renderer = original.inner_ref().transition.lock();
            assert_eq!(renderer.serial, Some(serial));
            assert!(renderer.live.get().is_some());
            assert!(renderer.composite.get().is_some());
            frozen_pixels(context, &renderer)
        };
        assert!(
            initial_frozen_pixels
                .chunks_exact(4)
                .any(|pixel| pixel[..3] != [0, 0, 0]),
            "Frozen capture contains supplied menu artwork"
        );
        let mut alias = original.render_clone(&mut context.render_clone_ctx());
        let mut frame_id = FrameId::default();
        frame_id.advance();
        let mut advance = AdvUpdateContext {
            frame_id,
            delta_ticks: Ticks::from_f32(duration / 2.0),
            asset_server: server,
            device: context.device,
            queue: context.queue,
            are_animations_allowed: true,
            voice_callback: None,
            movie_volume_settings: Default::default(),
        };
        alias.update(&advance);
        let mut late_alias = original.render_clone(&mut context.render_clone_ctx());
        original.update(&advance);
        late_alias.update(&advance);
        {
            let model = late_alias.inner_ref().model.lock().unwrap();
            let wipe = model.wipe.as_ref().unwrap();
            assert_eq!(
                wipe.elapsed,
                duration / 2.0,
                "Retained aliases advance the wipe once"
            );
            assert_eq!(wipe.source.menu, MurdererMenu::Top);
            assert_eq!(wipe.source.menu_focus, source_focus);
            assert_ne!(model.menu, MurdererMenu::Top);
        }
        late_alias
            .properties_mut()
            .property_tweener_mut(LayerProperty::TranslateX)
            .fast_forward_to(80.0);
        late_alias.pre_render(context, &TransformParams::default());
        alias.pre_render(context, &TransformParams::default());
        {
            let renderer = original.inner_ref().transition.lock();
            assert_eq!(renderer.serial, Some(serial));
            assert_eq!(renderer.elapsed, duration / 2.0);
            assert_eq!(
                frozen_pixels(context, &renderer),
                initial_frozen_pixels,
                "Live transform changes must preserve the frozen capture"
            );
        }
        if index == 5 {
            // The native child capture composes each caller's parent transform,
            // while frozen capture and wiper remain owned by the retained layer.
            let parent_a = TransformParams {
                transform: Mat4::from_translation(glam::Vec3::new(-140.0, 65.0, 0.0)),
                ..Default::default()
            };
            let parent_b = TransformParams {
                transform: Mat4::from_translation(glam::Vec3::new(210.0, -90.0, 0.0)),
                ..Default::default()
            };
            original.pre_render(context, &parent_a);
            let (live_a, composite_a) = {
                let renderer = original.inner_ref().transition.lock();
                (
                    capture_pixels(context, &renderer.live),
                    capture_pixels(context, &renderer.composite),
                )
            };
            alias.pre_render(context, &parent_b);
            {
                let renderer = alias.inner_ref().transition.lock();
                assert_eq!(frozen_pixels(context, &renderer), initial_frozen_pixels);
                assert_ne!(
                    capture_pixels(context, &renderer.live),
                    live_a,
                    "Each retained caller supplies the live child's parent transform"
                );
                assert_ne!(
                    capture_pixels(context, &renderer.composite),
                    composite_a,
                    "Glass presents the newly transformed live child"
                );
            }
            late_alias.pre_render(context, &parent_a);
            let (settled_live, settled_composite) = {
                let renderer = late_alias.inner_ref().transition.lock();
                assert_eq!(renderer.serial, Some(serial));
                assert_eq!(renderer.elapsed, duration / 2.0);
                assert_eq!(frozen_pixels(context, &renderer), initial_frozen_pixels);
                (
                    capture_pixels(context, &renderer.live),
                    capture_pixels(context, &renderer.composite),
                )
            };
            // Native captures preserve color outside the current transformed
            // child's geometry. A after B can differ from the earlier A; a
            // repeated A against that same retained history must be stable.
            assert!(
                settled_live != live_a,
                "A redraw retains prior B pixels outside A"
            );
            original.pre_render(context, &parent_a);
            {
                let renderer = original.inner_ref().transition.lock();
                assert!(capture_pixels(context, &renderer.live) == settled_live);
                assert!(
                    capture_pixels(context, &renderer.composite) == settled_composite,
                    "Repeated preparation does not advance Glass pieces or apply the parent twice"
                );
            }
            advance.frame_id.advance();
            advance.delta_ticks = Ticks::from_f32(2.5);
            advance.are_animations_allowed = false;
            alias.update(&advance);
            original.update(&advance);
            late_alias.update(&advance);
            late_alias.pre_render(context, &parent_a);
            {
                let renderer = late_alias.inner_ref().transition.lock();
                assert_eq!(renderer.elapsed, duration / 2.0, "Glass progress pauses");
                assert_eq!(frozen_pixels(context, &renderer), initial_frozen_pixels);
                assert_ne!(
                    capture_pixels(context, &renderer.composite),
                    settled_composite,
                    "Native Glass pieces keep moving while the progress clock pauses"
                );
            }
        }
        drop(original);
        drop(alias);
        assert!(weak_model.upgrade().is_some());
        assert!(weak_transition.upgrade().is_some());
        if index == 0 {
            // Natural completion releases captures while the retained layer lives.
            advance.frame_id.advance();
            late_alias.update(&advance);
            assert!(late_alias.inner_ref().model.lock().unwrap().wipe.is_none());
            let renderer = late_alias.inner_ref().transition.lock();
            assert!(renderer.serial.is_none());
            assert!(renderer.source.get().is_none());
            assert!(
                renderer.live.get().is_some(),
                "Live target survives native wipe completion"
            );
            assert!(renderer.composite.get().is_none());
        }
        // The Glass case drops its final owner with an active allocated wipe.
        drop(late_alias);
        assert!(
            weak_model.upgrade().is_none(),
            "Last retained owner releases its controller"
        );
        assert!(
            weak_transition.upgrade().is_none(),
            "Last retained owner releases capture storage"
        );
    }
    eprintln!(
        "Native retained Murderer wipes: Default/Glass real captures, late aliases update once, distinct parent transforms preserve frozen pixels and Glass physics, completion and last-owner release pass"
    );
}

impl NewDrawableLayerNeedsSeparatePass for MurdererAccusationImpl {
    fn needs_separate_pass(&self, _props: &crate::layer::LayerProperties) -> bool {
        self.model.lock().unwrap().wipe.is_some()
    }
}
impl NewDrawableLayer for MurdererAccusationImpl {
    fn property_policy(&self) -> crate::layer::new_drawable_layer::DrawablePropertyPolicy {
        crate::layer::new_drawable_layer::DrawablePropertyPolicy::Captured
    }

    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        params: &DrawableParams,
        clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        self.render_model(
            &self.model.lock().unwrap(),
            pass,
            transform,
            params,
            clip,
            stencil_ref,
            pass_kind,
        );
    }

    fn render_drawable_indirect(
        &mut self,
        context: &mut PreRenderContext,
        props: &crate::layer::LayerProperties,
        target: TextureTarget,
        depth_stencil: DepthStencilTarget,
        transform: &TransformParams,
    ) -> PassKind {
        let mut transition_guard = self.transition.lock();
        let transition = &mut *transition_guard;
        let model = self.model.lock().unwrap().clone();
        let composed = props.get_composed_transform_params(transform);
        let Some(wipe) = &model.wipe else {
            self.capture_child(context, &mut transition.live, &model, &composed);
            self.present_capture(
                context,
                target,
                depth_stencil,
                transition.live.get().unwrap(),
            );
            return PassKind::Opaque;
        };
        let canonical = TransformParams {
            transform: Mat4::IDENTITY,
            ..Default::default()
        };
        if transition.serial != Some(wipe.serial) {
            transition.source.clear();
            transition.serial = Some(wipe.serial);
            transition.elapsed = 0.0;
            // Native8cb60 builds its own canonical parent for the frozen
            // child. The live caller's parent is applied only by9b250.
            let frozen_transform = props.get_composed_transform_params(&TransformParams::default());
            self.capture_child(
                context,
                &mut transition.source,
                &wipe.source,
                &frozen_transform,
            );
        }
        // Owner+38 is one persistent target in normal and wipe rendering.
        self.capture_child(context, &mut transition.live, &model, &composed);
        transition.elapsed = wipe.elapsed;
        let composite = transition
            .composite
            .get_or_init(context)
            .as_texture_target();
        {
            let mut pass =
                context.begin_pass(composite, Some(depth_stencil), "Murderer transition");
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            let builder = RenderRequestBuilder::new().depth_stencil_shorthand(1, true, false);
            let live = transition.live.get().unwrap().as_texture_source();
            let source = transition.source.get().unwrap().as_texture_source();
            if wipe.kind == MurdererWipeKind::Glass {
                wipe.glass
                    .as_ref()
                    .expect("Glass state belongs to the active wipe")
                    .render(&mut pass, builder, live, source);
            } else {
                let mut wiper = crate::wiper::DefaultWiper::new(Ticks::from_f32(wipe.duration));
                wiper.advance_ticks(wipe.elapsed);
                wiper.render(&mut pass, builder, live, source);
            }
        }
        let mut pass = context.begin_pass(target, Some(depth_stencil), "Murderer transformed wipe");
        pass.clear(Some(UnormColor::from_rgba(0, 0, 0, 0)), Some(0), None);
        QuadVertices::new()
            .with_box(-960.0, -540.0, 960.0, 540.0)
            .render_sprite(
                &mut pass,
                RenderRequestBuilder::new()
                    .depth_stencil_shorthand(1, true, false)
                    .color_blend_type(ColorBlendType::Opaque),
                transition.composite.get().unwrap().as_texture_source(),
                // Native9b2e0 captures transformed geometry;9b4a0 wipes the
                // whole viewport. Do not apply the layer transform a second time.
                canonical.compute_final_transform(),
            );
        PassKind::Opaque
    }
}
impl MurdererAccusationImpl {
    fn render_model(
        &self,
        model: &MurdererModel,
        pass: &mut RenderPass,
        transform: &TransformParams,
        _params: &DrawableParams,
        _clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        match model.menu {
            MurdererMenu::Hints => {
                self.render_hints(model, pass, transform, stencil_ref, pass_kind)
            }
            MurdererMenu::Replay => {
                self.render_replay(model, pass, transform, stencil_ref, pass_kind)
            }
            MurdererMenu::Top => self.render_top(model, pass, transform, stencil_ref, pass_kind),
            MurdererMenu::Rules => {
                self.render_rules(model, pass, transform, stencil_ref, pass_kind)
            }
            MurdererMenu::PurpleByPerson | MurdererMenu::PurpleByChapter => {
                self.render_purple(model, pass, transform, stencil_ref, pass_kind)
            }
            MurdererMenu::Accusation => {
                self.render_accusation(model, pass, transform, stencil_ref, pass_kind)
            }
        }
    }
}

impl NewDrawableLayerFastForward for MurdererAccusationImpl {
    fn fast_forward(&mut self) {
        self.model.lock().unwrap().fast_forward();
    }
}
impl MurdererAccusationImpl {
    fn purple_text_height(&self, model: &MurdererModel) -> f32 {
        model.purple_original_index().map_or(0.0, |index| {
            if model.menu == MurdererMenu::PurpleByChapter {
                self.pages.chapters[index].height
            } else {
                self.pages.persons[index].height
            }
        })
    }
}
impl AdvUpdatable for MurdererAccusationImpl {
    fn update(&mut self, context: &AdvUpdateContext) {
        let mut model = self.model.lock().unwrap();
        let height = self.purple_text_height(&model);
        model.set_purple_text_height(height);
        model.update_for_frame(
            context.delta_ticks.as_f32(),
            context.are_animations_allowed,
            context.frame_id,
        );
        drop(model);
        if self.model.lock().unwrap().wipe.is_none() {
            self.transition.lock().finish_wipe();
        }
    }
}

#[cfg(test)]
#[path = "murderer_rule_draw_tests.rs"]
mod rule_draw_tests;
#[cfg(test)]
pub(crate) use rule_draw_tests::reference_rule_draw;

#[path = "murderer_rule.rs"]
mod rule;

#[cfg(test)]
#[path = "murderer_top_draw_tests.rs"]
mod top_draw_tests;
#[cfg(test)]
pub(crate) use top_draw_tests::reference_top_draw;

#[path = "murderer_top.rs"]
mod top;

#[cfg(test)]
#[path = "murderer_replay_draw_tests.rs"]
mod replay_draw_tests;
#[cfg(test)]
pub(crate) use replay_draw_tests::reference_replay_draw;

#[path = "murderer_replay.rs"]
mod replay;

#[cfg(test)]
#[path = "murderer_hints_draw_tests.rs"]
mod hints_draw_tests;
#[cfg(test)]
pub(crate) use hints_draw_tests::reference_hints_draw;

#[path = "murderer_hints.rs"]
mod hints;

#[path = "murderer_capture.rs"]
mod capture;

#[cfg(test)]
#[path = "murderer_capture_tests.rs"]
mod capture_tests;
#[cfg(test)]
pub(crate) use capture_tests::reference_capture_lifecycle;

#[cfg(test)]
#[path = "murderer_purple_draw_tests.rs"]
mod purple_draw_tests;
#[cfg(test)]
pub(crate) use purple_draw_tests::reference_purple_draw;

#[path = "murderer_purple.rs"]
mod purple;

#[cfg(test)]
#[path = "murderer_accusation_draw_tests.rs"]
mod accusation_draw_tests;
#[cfg(test)]
pub(crate) use accusation_draw_tests::reference_accusation_draw;

#[path = "murderer_select.rs"]
mod select;
