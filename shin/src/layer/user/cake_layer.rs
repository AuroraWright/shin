use crate::{
    asset::{system::AssetServer, texture_archive::TextureArchive},
    layer::{
        NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipParams, DrawableParams, TransformParams},
    },
    update::{AdvUpdatable, AdvUpdateContext},
};
use glam::{Vec2, Vec4, vec4};
use shin_core::time::{Ticks, Tween, Tweener};
use shin_render::{
    ColorBlendType, PassKind, RenderRequestBuilder, gpu_texture::GpuTexture,
    quad_vertices::QuadVertices, render_pass::RenderPass, shaders::types::RenderClone,
};
use std::sync::{Arc, Mutex};

// Exact packed 240x240 hit map at NSO address 0x166529. Each cell covers
// 4x4 pixels inside screen rect (480,0,960,960); high nibble is even X.
const HIT_MAP: &[u8; 28800] = include_bytes!("cake_hit_map.bin");
// Sprite placement recovered from native initializer 0x46d3c..0x470cc.
const SLICE_RECTS: [Vec4; 15] = [
    vec4(384.0, 336.0, 256.0, 592.0),
    vec4(240.0, 336.0, 384.0, 544.0),
    vec4(160.0, 320.0, 464.0, 464.0),
    vec4(144.0, 272.0, 480.0, 384.0),
    vec4(176.0, 176.0, 448.0, 208.0),
    vec4(256.0, 80.0, 368.0, 304.0),
    vec4(368.0, 32.0, 256.0, 336.0),
    vec4(496.0, 16.0, 208.0, 368.0),
    vec4(576.0, 32.0, 256.0, 352.0),
    vec4(576.0, 96.0, 368.0, 288.0),
    vec4(592.0, 176.0, 432.0, 208.0),
    vec4(576.0, 288.0, 480.0, 368.0),
    vec4(576.0, 320.0, 464.0, 464.0),
    vec4(576.0, 336.0, 384.0, 544.0),
    vec4(576.0, 336.0, 240.0, 576.0),
];

#[derive(TextureArchive)]
pub struct CakeTextures {
    #[txa(name = "base")]
    base: GpuTexture,
    #[txa(name = "msg")]
    msg: GpuTexture,
    #[txa(name = "finger")]
    finger: GpuTexture,
    #[txa(name = "00")]
    slice_0: GpuTexture,
    #[txa(name = "01")]
    slice_1: GpuTexture,
    #[txa(name = "02")]
    slice_2: GpuTexture,
    #[txa(name = "03")]
    slice_3: GpuTexture,
    #[txa(name = "04")]
    slice_4: GpuTexture,
    #[txa(name = "05")]
    slice_5: GpuTexture,
    #[txa(name = "06")]
    slice_6: GpuTexture,
    #[txa(name = "07")]
    slice_7: GpuTexture,
    #[txa(name = "08")]
    slice_8: GpuTexture,
    #[txa(name = "09")]
    slice_9: GpuTexture,
    #[txa(name = "10")]
    slice_10: GpuTexture,
    #[txa(name = "11")]
    slice_11: GpuTexture,
    #[txa(name = "12")]
    slice_12: GpuTexture,
    #[txa(name = "13")]
    slice_13: GpuTexture,
    #[txa(name = "14")]
    slice_14: GpuTexture,
}
impl CakeTextures {
    fn slices(&self) -> [&GpuTexture; 15] {
        [
            &self.slice_0,
            &self.slice_1,
            &self.slice_2,
            &self.slice_3,
            &self.slice_4,
            &self.slice_5,
            &self.slice_6,
            &self.slice_7,
            &self.slice_8,
            &self.slice_9,
            &self.slice_10,
            &self.slice_11,
            &self.slice_12,
            &self.slice_13,
            &self.slice_14,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct CakeModel {
    pub selected: Option<usize>,
    pub result: Option<i32>,
    last_pointer: Option<Vec2>,
    restored_slice: Option<usize>,
    completed_restoration: bool,
    finger: Vec2,
    focus: [Tweener; 15],
    flash: [Tweener; 15],
    instruction_opacity: Tweener,
    confirming: bool,
    sounds: Vec<&'static str>,
    pointer_area: Option<crate::adv::input_area::InputArea>,
    pointer_extent: (u16, u16),
    pointer_row: Option<usize>,
    pointer_consumed: bool,
}
impl Default for CakeModel {
    fn default() -> Self {
        Self {
            selected: None,
            result: None,
            last_pointer: None,
            restored_slice: None,
            completed_restoration: false,
            finger: Vec2::new(1440.0, 900.0),
            focus: std::array::from_fn(|_| Tweener::new(0.0)),
            flash: std::array::from_fn(|_| Tweener::new(0.0)),
            instruction_opacity: Tweener::new(1.0),
            confirming: false,
            sounds: Vec::new(),
            pointer_area: Some(crate::adv::input_area::InputArea::canvas()),
            pointer_extent: (1920, 1080),
            pointer_row: None,
            pointer_consumed: false,
        }
    }
}
impl CakeModel {
    pub fn restore_completed_answer(&mut self, answer: i32) {
        // Native45330 restores row focus, clears flashes and ends input. It
        // preserves the constructor's unselected response index (a0=-1).
        self.restored_slice = usize::try_from(answer).ok().filter(|&row| row < 15);
        self.completed_restoration = true;
        self.confirming = false;
        for row in 0..15 {
            self.focus[row].fast_forward_to(if Some(row) == self.restored_slice {
                1.0
            } else {
                0.0
            });
            self.flash[row].fast_forward_to(0.0);
        }
        self.instruction_opacity.fast_forward_to(0.0);
        self.result = Some(self.selected.map_or(-1, |row| row as i32));
    }
    pub(crate) fn visual_slice(&self) -> Option<usize> {
        if self.completed_restoration {
            self.restored_slice
        } else {
            self.selected
        }
    }
    pub fn hit_test(position: Vec2) -> Option<usize> {
        if !position.is_finite()
            || position.x < 480.0
            || position.x >= 1440.0
            || position.y < 0.0
            || position.y >= 960.0
        {
            return None;
        }
        let x = ((position.x - 480.0) as usize) / 4;
        let y = position.y as usize / 4;
        let packed = HIT_MAP[y * 120 + x / 2];
        let value = if x % 2 == 0 { packed >> 4 } else { packed & 15 };
        (value != 0).then(|| value as usize - 1)
    }
    pub fn navigate(&mut self, delta: i32) {
        self.controller_input(
            if delta < 0 {
                1
            } else if delta > 0 {
                2
            } else {
                0
            },
            false,
            Vec2::ZERO,
        );
    }
    pub fn confirm(&mut self) {
        if self.result.is_some() || self.confirming {
            return;
        }
        if let Some(row) = self.selected {
            Self::retarget(&mut self.flash[row], 0.7, 0.2);
            self.flash[row].enqueue(0.0, Tween::linear(Ticks::from_f32(0.7 / 0.2)));
            Self::retarget(&mut self.instruction_opacity, 0.0, 0.1);
            self.confirming = true;
            self.sounds.push("decide");
        } else {
            // Native GOT2246e8 (1.03:226748) resolves to /sysse.bin:error.
            self.sounds.push("error");
        }
    }
    fn retarget(tween: &mut Tweener, value: f32, speed: f32) {
        tween.enqueue_now(
            value,
            Tween::linear(Ticks::from_f32((value - tween.value()).abs() / speed)),
        );
    }
    fn select(&mut self, row: Option<usize>) {
        if self.selected == row {
            return;
        }
        if let Some(old) = self.selected {
            Self::retarget(&mut self.focus[old], 0.0, 0.05);
        }
        self.selected = row;
        if let Some(new) = row {
            Self::retarget(&mut self.focus[new], 1.0, 0.25);
        }
    }
    fn clamp_finger(position: Vec2) -> Vec2 {
        position.clamp(Vec2::new(28.0, 38.0), Vec2::new(1863.0, 1015.0))
    }
    pub(crate) fn controller_input(&mut self, held: u32, confirm: bool, axes: Vec2) -> bool {
        if self.pointer_consumed || self.result.is_some() || self.confirming {
            return false;
        }
        // Native45e40: confirm wins, otherwise move the finger every held frame.
        if confirm {
            self.confirm();
            return true;
        }
        let dx = if held & 4 != 0 {
            -8.0
        } else if held & 8 != 0 {
            8.0
        } else {
            0.0
        };
        let dy = if held & 1 != 0 {
            -8.0
        } else if held & 2 != 0 {
            8.0
        } else {
            0.0
        };
        let proposed = self.finger + Vec2::new(dx, dy) + axes * 12.0;
        self.finger = Self::clamp_finger(proposed);
        // Native clamps stored40, but passes the unclamped stack point to45080.
        self.select(Self::hit_test(proposed));
        true
    }
    fn action_input(
        &mut self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
        axes: Vec2,
    ) {
        use crate::app::AppAction;
        let held = (state[AppAction::DpadUp].is_held as u32)
            | ((state[AppAction::DpadDown].is_held as u32) << 1)
            | ((state[AppAction::DpadLeft].is_held as u32) << 2)
            | ((state[AppAction::DpadRight].is_held as u32) << 3);
        self.controller_input(held, state[AppAction::Enter].is_clicked, axes);
    }
    pub(crate) fn update(&mut self, delta: Ticks) {
        // Native44af0 checks the flash BEFORE advancing any clock, even delta0.
        if self.confirming && self.flash[self.selected.unwrap()].is_idle() {
            self.result = self.selected.map(|row| row as i32);
            self.confirming = false;
        }
        for row in 0..15 {
            self.focus[row].update(delta);
            self.flash[row].update(delta);
        }
        self.instruction_opacity.update(delta);
    }
    pub(crate) fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }
    #[cfg(test)]
    pub(crate) fn reference_tween_values(&self) -> [f32; 31] {
        std::array::from_fn(|i| {
            if i == 30 {
                self.instruction_opacity.value()
            } else if i % 2 == 0 {
                self.focus[i / 2].value()
            } else {
                self.flash[i / 2].value()
            }
        })
    }
    pub fn pointer(&mut self, position: Vec2, clicked: bool) {
        if self.result.is_some() || self.confirming || !position.is_finite() {
            return;
        }
        if self.last_pointer != Some(position) || clicked {
            if clicked {
                // Complete-gesture convenience; actual host contacts use InputArea.
                use crate::adv::input_area::InputAreaTarget;
                self.probe(position);
                self.release(position);
                InputAreaTarget::reset(self);
            } else {
                self.finger = Self::clamp_finger(position);
                self.select(Self::hit_test(position));
            }
        }
        self.last_pointer = Some(position);
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    fn pointer_frame(
        &mut self,
        frame: &crate::adv::input_area::PointerFrame<'_>,
        extent: (u16, u16),
    ) {
        let mut area = self
            .pointer_area
            .take()
            .expect("Cake pointer dispatcher reentered");
        if extent != self.pointer_extent && extent.0 > 0 && extent.1 > 0 {
            area.disable(self);
            area = crate::adv::input_area::InputArea::new(extent.0, extent.1);
            self.pointer_extent = extent;
        }
        if self.result.is_some() || self.confirming {
            area.disable(self);
            self.pointer_consumed = true;
        } else {
            // Native Cake has contact callbacks, but no passive hover callback.
            self.pointer_consumed = area.update(self, frame);
        }
        self.pointer_area = Some(area);
    }
    fn disable_pointer(&mut self) {
        let mut area = self.pointer_area.take().unwrap();
        area.disable(self);
        self.pointer_area = Some(area);
        self.pointer_consumed = false;
    }
}
impl crate::adv::input_area::InputAreaTarget for CakeModel {
    fn probe(&mut self, position: Vec2) -> bool {
        if self.result.is_some() || self.confirming {
            return false;
        }
        self.pointer_row = Self::hit_test(position);
        true // Native460b0 captures plate/outside taps too, with row-1.
    }
    fn reset(&mut self) {
        self.pointer_row = None;
    }
    fn press(&mut self, position: Vec2) {
        self.finger = Self::clamp_finger(position);
        self.select(self.pointer_row);
        if let Some(row) = self.pointer_row {
            Self::retarget(&mut self.flash[row], 0.7, 0.2);
        }
    }
    fn release(&mut self, position: Vec2) {
        self.finger = Self::clamp_finger(position);
        self.select(self.pointer_row);
        self.confirm();
    }
    fn cancel(&mut self) {
        if let Some(row) = self.pointer_row.take() {
            Self::retarget(&mut self.flash[row], 0.0, 0.2);
        }
    }
    fn long_press(&mut self, _: Vec2) {
        self.cancel();
    }
    fn drag(&mut self, _: Vec2, _: Vec2) {
        self.cancel();
    }
    fn gesture(&mut self, _: Vec2) {}
}

pub struct CakeLayerImpl {
    textures: Arc<CakeTextures>,
    model: Arc<Mutex<CakeModel>>,
}
#[cfg(test)]
impl CakeLayerImpl {
    pub(crate) fn reference_texture(&self, name: &str) -> &GpuTexture {
        match name {
            "base" => &self.textures.base,
            "msg" => &self.textures.msg,
            "finger" => &self.textures.finger,
            _ => self.textures.slices()[name.parse::<usize>().expect("Native Cake slice name")],
        }
    }
}
impl Clone for CakeLayerImpl {
    fn clone(&self) -> Self {
        Self {
            textures: self.textures.clone(),
            model: Arc::new(Mutex::new(self.model.lock().unwrap().clone())),
        }
    }
}
impl RenderClone for CakeLayerImpl {
    fn render_clone(&self, _: &mut shin_render::shaders::types::RenderCloneCtx) -> Self {
        self.clone()
    }
}
impl std::fmt::Debug for CakeLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CakeLayer")
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}
pub type CakeLayer = NewDrawableLayerWrapper<CakeLayerImpl>;
impl CakeLayer {
    pub async fn load(server: &AssetServer) -> Self {
        Self::new(
            server
                .load("/cake.txa")
                .await
                .expect("Failed to load cake textures"),
        )
    }
    pub(crate) fn new(textures: Arc<CakeTextures>) -> Self {
        Self::from_plain_layer_constructor(|| CakeLayerImpl {
            textures,
            model: Arc::new(Mutex::new(CakeModel::default())),
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_draw_command(&self, command: &str) {
        use crate::adv::input_area::InputAreaTarget;
        let mut model = self.inner_ref().model.lock().unwrap();
        let mut parts = command.split_whitespace();
        let op = parts.next().unwrap();
        let args: Vec<f32> = parts.map(|p| p.parse().unwrap()).collect();
        match op {
            "C" => {
                model.controller_input(args[0] as u32, args[1] != 0.0, Vec2::new(args[2], args[3]));
            }
            "P" => {
                assert!(model.probe(Vec2::new(args[0], args[1])));
            }
            "H" => model.press(Vec2::new(args[0], args[1])),
            "R" => model.release(Vec2::new(args[0], args[1])),
            _ => panic!("Unsupported native Cake command {command}"),
        }
    }
    #[cfg(test)]
    pub(crate) fn reference_controller_state(&self) -> (usize, usize, i32) {
        let inner = self.inner_ref();
        (
            Arc::as_ptr(&inner.textures) as usize,
            Arc::as_ptr(&inner.model) as usize,
            inner
                .model
                .lock()
                .unwrap()
                .selected
                .map_or(-1, |x| x as i32),
        )
    }
    pub fn reset(&self) {
        self.inner_ref().model.lock().unwrap().reset();
    }
    pub fn restore_completed_answer(&self, answer: i32) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .restore_completed_answer(answer);
    }
    #[cfg(test)]
    pub(crate) fn reference_visual_slice(&self) -> Option<usize> {
        self.inner_ref().model.lock().unwrap().visual_slice()
    }
    pub fn pointer(&self, position: Vec2, clicked: bool) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .pointer(position, clicked);
    }
    pub fn navigate(&self, delta: i32) {
        self.inner_ref().model.lock().unwrap().navigate(delta);
    }
    pub fn confirm(&self) {
        self.inner_ref().model.lock().unwrap().confirm();
    }
    pub(crate) fn handle_controller_input(
        &self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
        axes: Vec2,
    ) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .action_input(state, axes);
    }
    pub(crate) fn pointer_frame(
        &self,
        frame: &crate::adv::input_area::PointerFrame<'_>,
        extent: (u16, u16),
    ) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .pointer_frame(frame, extent);
    }
    pub(crate) fn ignore_pointer_state(&self) {
        self.inner_ref().model.lock().unwrap().disable_pointer();
    }
    pub(crate) fn take_sounds(&self) -> Vec<&'static str> {
        self.inner_ref().model.lock().unwrap().take_sounds()
    }
    #[cfg(test)]
    pub(crate) fn reference_feedback(
        &self,
    ) -> (Vec2, Option<usize>, bool, f32, [f32; 15], [f32; 15]) {
        let model = self.inner_ref().model.lock().unwrap();
        (
            model.finger,
            model.pointer_row,
            model.confirming,
            model.instruction_opacity.value(),
            std::array::from_fn(|row| model.focus[row].value()),
            std::array::from_fn(|row| model.flash[row].value()),
        )
    }
    #[cfg(test)]
    pub(crate) fn reference_update(&self, ticks: f32) {
        self.inner_ref()
            .model
            .lock()
            .unwrap()
            .update(Ticks::from_f32(ticks));
    }
    pub fn result(&self) -> Option<i32> {
        self.inner_ref().model.lock().unwrap().result
    }
}
impl NewDrawableLayerNeedsSeparatePass for CakeLayerImpl {}
impl NewDrawableLayer for CakeLayerImpl {
    fn property_policy(&self) -> crate::layer::new_drawable_layer::DrawablePropertyPolicy {
        // Native44af0 updates only the model;44ae0/710d0 are RET.
        //44b90 reads transforms/flags and uses white RGB/internal alpha.
        crate::layer::new_drawable_layer::DrawablePropertyPolicy::TransformOnly
    }
    fn renders_pass(&self, pass_kind: PassKind) -> bool {
        pass_kind == PassKind::Transparent
    }
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        _params: &DrawableParams,
        _clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if pass_kind != PassKind::Transparent {
            return;
        }
        let builder = RenderRequestBuilder::new()
            .depth_stencil_shorthand(stencil_ref, true, false)
            .color_blend_type(ColorBlendType::Layer1);
        let matrix = transform.compute_top_left_transform();
        let draw =
            |pass: &mut RenderPass, texture: &GpuTexture, rect: Vec4, alpha: f32, flash: f32| {
                QuadVertices::new()
                    .with_box(rect.x, rect.y, rect.x + rect.z, rect.y + rect.w)
                    .with_color(
                        shin_core::primitives::color::FloatColor4::WHITE
                            .with_alpha(alpha)
                            .into_unorm(),
                    )
                    .render_button(
                        pass,
                        builder,
                        texture.as_source(),
                        matrix,
                        Vec4::ZERO,
                        vec4(1.0, 1.0, 1.0, flash),
                    );
            };
        draw(
            pass,
            &self.textures.base,
            vec4(360.0, 0.0, 1200.0, 1080.0),
            1.0,
            0.0,
        );
        let model = self.model.lock().unwrap();
        for row in 0..15 {
            let focus = model.focus[row].value();
            if focus > 0.0 {
                let mut rect = SLICE_RECTS[row];
                rect.x += 360.0;
                draw(
                    pass,
                    self.textures.slices()[row],
                    rect,
                    focus,
                    model.flash[row].value(),
                );
            }
        }
        // Native44e98..44ea8 gates both message and pointer on owner48.
        // The completed-save callback45330 resets that tween to zero.
        let opacity = model.instruction_opacity.value();
        if opacity <= 0.0 {
            return;
        }
        draw(
            pass,
            &self.textures.msg,
            vec4(616.0, 920.0, 688.0, 96.0),
            opacity,
            0.0,
        );
        let pointer = model.finger;
        draw(
            pass,
            &self.textures.finger,
            vec4(pointer.x - 16.0, pointer.y - 16.0, 96.0, 96.0),
            opacity,
            0.0,
        );
    }
}
impl NewDrawableLayerFastForward for CakeLayerImpl {
    fn fast_forward(&mut self) {}
}
impl AdvUpdatable for CakeLayerImpl {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.model.lock().unwrap().update(context.delta_ticks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires supplied ROM, Metal and exact Loopback Audio"]
    fn reference_cake_outside_clicks_play_supplied_error_cue() {
        use crate::{
            adv::input_area::PointerSampler,
            asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache},
            audio::SystemSoundPlayer,
        };
        crate::init_test_task_pools();
        let adapter =
            shin_tasks::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
                .unwrap();
        let (device, queue) =
            shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .unwrap();
        let mut io = LayeredAssetIo::new();
        if let Ok(patch) = std::env::var("SHIN_REFERENCE_PATCH_ROM") {
            io.try_with_rom(patch).unwrap();
        }
        io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        let server = AssetServer::new(
            io.into(),
            AssetLoadContext {
                game: crate::cli::Game::Umineko,
                higurashi_bustup_cache: Default::default(),
                wgpu_device: device,
                wgpu_queue: queue,
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        );
        let bank = server
            .load_sync::<shin_core::format::sysse::SysSe>("/sysse.bin")
            .unwrap();
        assert!(bank.sounds.contains_key("error"));
        assert!(!bank.sounds.contains_key("reject"));
        let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
        let mut player = SystemSoundPlayer::new(manager);
        let mut clicks = 0;
        for extent in [(1280, 720), (3840, 2160)] {
            let mut model = CakeModel::default();
            let mut pointer = PointerSampler::default();
            let scale = Vec2::new(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0);
            for point in [
                Vec2::ZERO,
                Vec2::new(100.0, 100.0),
                Vec2::new(480.0, 0.0),
                Vec2::new(1440.0, 500.0),
                Vec2::new(960.0, 960.0),
                Vec2::new(1900.0, 1000.0),
            ] {
                assert_eq!(CakeModel::hit_test(point), None);
                for _ in 0..3 {
                    pointer.sample(point * scale, true, 0.0);
                    model.pointer_frame(&pointer.frame(), extent);
                    pointer.sample(point * scale, false, 0.01);
                    model.pointer_frame(&pointer.frame(), extent);
                    let cues = model.take_sounds();
                    assert_eq!(cues, ["error"]);
                    for cue in cues {
                        player.play(&server, cue, shin_core::vm::command::types::Volume(1.0));
                    }
                    assert_eq!(model.result, None);
                    assert!(!model.confirming);
                    clicks += 1;
                }
            }
            let point = Vec2::new(850.0, 700.0) * scale;
            pointer.sample(point, true, 0.0);
            model.pointer_frame(&pointer.frame(), extent);
            pointer.sample(point, false, 0.01);
            model.pointer_frame(&pointer.frame(), extent);
            assert_eq!(model.take_sounds(), ["decide"]);
            player.play(
                &server,
                "decide",
                shin_core::vm::command::types::Volume(1.0),
            );
            model.update(Ticks::from_f32(7.0));
            model.update(Ticks::ZERO);
            assert_eq!(model.result, Some(0));
        }
        assert_eq!(player.sounds.len(), clicks + 2);
        println!(
            "{clicks} outside/plate mouse clicks play the supplied error cue without completing Cake; valid selection after rejection succeeds at both viewports."
        );
    }

    #[test]
    fn raw_stick_moves_cake_without_synthesized_dpad_step_but_real_dpad_adds_eight_pixels() {
        use crate::app::AppAction;
        use shin_input::{
            Action, RawInputState,
            inputs::{GamepadButton, VirtualGamepadButton},
        };
        let mut raw = RawInputState::new();
        raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLLeft] = true;
        raw.gamepads.unified.virtual_keys[VirtualGamepadButton::StickLDown] = true;
        let state = |raw: &RawInputState| {
            let lowered = AppAction::lower(raw);
            enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
                is_held: lowered[action],
                ..Default::default()
            })
        };
        let mut model = CakeModel::default();
        model.action_input(&state(&raw), Vec2::new(-0.75, 0.75));
        assert_eq!(model.finger, Vec2::new(1431.0, 909.0));
        raw.gamepads.unified.buttons[GamepadButton::Left] = true;
        raw.gamepads.unified.buttons[GamepadButton::Up] = true;
        model.action_input(&state(&raw), Vec2::new(-0.75, 0.75));
        assert_eq!(model.finger, Vec2::new(1414.0, 910.0));
        raw.gamepads.unified.buttons[GamepadButton::Left] = false;
        raw.gamepads.unified.buttons[GamepadButton::Up] = false;
        raw.keyboard.insert(winit::keyboard::KeyCode::ArrowRight);
        model.action_input(&state(&raw), Vec2::ZERO);
        assert_eq!(model.finger, Vec2::new(1422.0, 910.0));
    }
    #[test]
    fn native_hit_map_has_fifteen_slices_and_rejects_plate_and_outside() {
        for (point, index) in [
            (Vec2::new(850.0, 700.0), 0),
            (Vec2::new(960.0, 700.0), 14),
            (Vec2::new(1100.0, 400.0), 12),
            (Vec2::new(1000.0, 100.0), 7),
        ] {
            assert_eq!(CakeModel::hit_test(point), Some(index));
        }
        let mut seen = [false; 15];
        for y in 0..240 {
            for x in 0..240 {
                if let Some(index) =
                    CakeModel::hit_test(Vec2::new(482.0 + x as f32 * 4.0, 2.0 + y as f32 * 4.0))
                {
                    seen[index] = true;
                }
            }
        }
        assert!(seen.into_iter().all(|value| value));
        for point in [
            Vec2::new(479.0, 500.0),
            Vec2::new(1440.0, 500.0),
            Vec2::new(900.0, 960.0),
            Vec2::new(480.0, 0.0),
            Vec2::new(f32::NAN, 0.0),
        ] {
            assert_eq!(CakeModel::hit_test(point), None);
        }
    }
    #[test]
    fn selection_waits_for_confirmation_and_reload_clears_previous_answer() {
        let mut model = CakeModel::default();
        model.confirm();
        assert_eq!(model.result, None);
        assert_eq!(model.take_sounds(), ["error"]);
        model.navigate(-1);
        assert_eq!(model.finger, Vec2::new(1440.0, 892.0));
        assert_eq!(model.selected, None);
        model.pointer(Vec2::new(850.0, 700.0), false);
        model.update(Ticks::from_f32(4.0));
        assert_eq!(model.focus[0].value(), 1.0);
        model.confirm();
        assert_eq!(model.result, None);
        model.update(Ticks::from_f32(7.0));
        assert_eq!(model.result, None);
        assert_eq!(model.flash[0].value(), 0.0);
        assert!((model.instruction_opacity.value() - 0.3).abs() < 1e-6);
        model.update(Ticks::ZERO);
        assert_eq!(model.result, Some(0));
        model.navigate(5);
        assert_eq!(model.result, Some(0));
        model.reset();
        assert_eq!(model.selected, None);
        assert_eq!(model.result, None);
        model.pointer(Vec2::new(480.0, 0.0), true);
        assert_eq!(model.result, None);
    }

    #[test]
    fn captured_cake_contacts_consume_controls_delay_feedback_and_cancel_on_drag_or_resize() {
        use crate::adv::input_area::PointerSampler;
        for extent in [(1280, 720), (3840, 2160)] {
            let mut model = CakeModel::default();
            let mut pointer = PointerSampler::default();
            let point = Vec2::new(850.0, 700.0);
            let scale = Vec2::new(extent.0 as f32 / 1920.0, extent.1 as f32 / 1080.0);
            let sample = |model: &mut CakeModel,
                          sampler: &mut PointerSampler,
                          point: Vec2,
                          held,
                          seconds| {
                sampler.sample(point * scale, held, seconds);
                model.pointer_frame(&sampler.frame(), extent);
            };
            sample(&mut model, &mut pointer, point, false, 0.0);
            assert_eq!(
                model.selected, None,
                "Cake has no passive touch-hover callback"
            );
            sample(&mut model, &mut pointer, point, true, 0.0);
            assert_eq!(model.pointer_row, Some(0));
            assert_eq!(model.selected, None);
            assert!(!model.controller_input(15, true, Vec2::ONE));
            sample(&mut model, &mut pointer, point, true, 0.151);
            model.update(Ticks::from_f32(1.0));
            assert_eq!(model.selected, Some(0));
            assert!((model.flash[0].value() - 0.2).abs() < 1e-6);
            sample(&mut model, &mut pointer, point, true, 0.02);
            model.update(Ticks::from_f32(3.0));
            assert_eq!(model.flash[0].value(), 0.7);
            sample(
                &mut model,
                &mut pointer,
                point + Vec2::new(20.0, 0.0),
                true,
                0.01,
            );
            assert_eq!(model.pointer_row, None);
            model.update(Ticks::from_f32(4.0));
            assert_eq!(model.flash[0].value(), 0.0);
            sample(&mut model, &mut pointer, point, false, 0.01);
            assert_eq!(model.result, None);
            assert!(model.take_sounds().is_empty());
            sample(&mut model, &mut pointer, point, true, 0.0);
            sample(
                &mut model,
                &mut pointer,
                Vec2::new(1900.0, 1000.0),
                false,
                0.01,
            );
            assert!(model.confirming);
            assert_eq!(
                model.selected,
                Some(0),
                "Release coordinates do not replace captured row"
            );
            assert_eq!(model.take_sounds(), ["decide"]);
            model.update(Ticks::from_f32(7.0));
            assert_eq!(model.result, None);
            model.update(Ticks::ZERO);
            assert_eq!(model.result, Some(0));

            model.reset();
            pointer = PointerSampler::default();
            sample(&mut model, &mut pointer, point, true, 0.0);
            sample(&mut model, &mut pointer, point, true, 0.151);
            model.update(Ticks::from_f32(1.0));
            model.pointer_frame(&pointer.frame(), (1920, 1080));
            assert_eq!(model.pointer_row, None);
            model.update(Ticks::from_f32(1.0));
            assert_eq!(model.flash[0].value(), 0.0);
            assert!(!model.confirming);
        }
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "compares production CakeModel with guarded original/1.03 native input and clocks"]
    fn reference_native_cake_input_focus_flash_clocks_match_model() {
        use crate::adv::input_area::InputAreaTarget;
        use std::{
            io::Write,
            process::{Command, Stdio},
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let binary =
            std::env::temp_dir().join(format!("shin-cake-model-native-{}", std::process::id()));
        let output = Command::new("/usr/bin/clang")
            .args(["-O0", "-Wall", "-Wextra"])
            .arg(root.join("reverse-engineering/native-cake-input.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut commands = vec!["N".to_string()];
        for y in 0..240 {
            for x in 0..240 {
                commands.push(format!("P {} {}", 482 + x * 4, 2 + y * 4));
            }
        }
        for point in [
            Vec2::new(850.0, 700.0),
            Vec2::new(960.0, 700.0),
            Vec2::new(1000.0, 100.0),
            Vec2::new(28.0, 38.0),
            Vec2::new(1863.0, 1015.0),
            Vec2::ZERO,
            Vec2::new(2000.0, 1100.0),
        ] {
            for mask in 0..16 {
                for axes in [Vec2::ZERO, Vec2::ONE, Vec2::NEG_ONE, Vec2::new(0.5, -0.5)] {
                    commands.extend([
                        "N".to_string(),
                        format!("M {} {}", point.x, point.y),
                        format!("C {mask} 0 {} {}", axes.x, axes.y),
                        "T 1".to_string(),
                    ]);
                }
            }
        }
        for point in [
            Vec2::new(850.0, 700.0),
            Vec2::new(960.0, 700.0),
            Vec2::new(1000.0, 100.0),
            Vec2::ZERO,
        ] {
            for held in [0.0, 0.25, 1.0, 2.0, 3.5, 4.0, 10.0] {
                commands.extend([
                    "N".to_string(),
                    format!("P {} {}", point.x, point.y),
                    format!("H {} {}", point.x, point.y),
                    format!("T {held}"),
                    "R 2000 -50".to_string(),
                ]);
                for ticks in [0.0, 0.25, 0.5, 1.0, 1.75, 3.5, 0.0, 3.0] {
                    commands.push(format!("T {ticks}"));
                }
                commands.push("C 15 1 1 1".to_string());
            }
            for cancel in ["X", "B", "D"] {
                commands.extend([
                    "N".to_string(),
                    format!("P {} {}", point.x, point.y),
                    format!("H {} {}", point.x, point.y),
                    "T 2".to_string(),
                    cancel.to_string(),
                    "T 1".to_string(),
                    "T 1".to_string(),
                    "G".to_string(),
                    "L".to_string(),
                ]);
            }
        }
        commands.extend(
            [
                "N",
                "C 15 1 1 1",
                "M 850 700",
                "C 0 0 0 0",
                "T 4",
                "M 960 700",
                "C 0 0 0 0",
                "T 1",
                "T 3",
                "T 16",
                "C 15 1 1 1",
                "T 7",
                "C 15 1 1 1",
                "T 0",
                "C 15 1 1 1",
                "T 3",
            ]
            .map(str::to_string),
        );
        let input = commands.join("\n") + "\n";
        for name in ["uncompressed_executable", "uncompressed_executable_update"] {
            let mut child = Command::new(&binary)
                .arg(root.join("umineko_switch_files").join(name))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            // Stream input concurrently: the native runner also writes multi-MB output.
            let mut stdin = child.stdin.take().unwrap();
            let payload = input.clone();
            let writer = std::thread::spawn(move || stdin.write_all(payload.as_bytes()).unwrap());
            let output = child.wait_with_output().unwrap();
            writer.join().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = String::from_utf8(output.stdout).unwrap();
            assert_eq!(output.lines().count(), commands.len());
            let mut model = CakeModel::default();
            let (mut decide, mut reject) = (0, 0);
            for (step, (command, line)) in commands.iter().zip(output.lines()).enumerate() {
                let mut parts = command.split_whitespace();
                let op = parts.next().unwrap();
                let args: Vec<f32> = parts.map(|s| s.parse().unwrap()).collect();
                let mut consumed = -1;
                match op {
                    "N" => {
                        model.reset();
                        decide = 0;
                        reject = 0;
                    }
                    "M" => model.finger = Vec2::new(args[0], args[1]),
                    "C" => {
                        consumed = model.controller_input(
                            args[0] as u32,
                            args[1] != 0.0,
                            Vec2::new(args[2], args[3]),
                        ) as i32
                    }
                    "P" => consumed = model.probe(Vec2::new(args[0], args[1])) as i32,
                    "H" => model.press(Vec2::new(args[0], args[1])),
                    "R" => model.release(Vec2::new(args[0], args[1])),
                    "X" => model.cancel(),
                    "B" => model.long_press(Vec2::ZERO),
                    "D" => model.drag(Vec2::ZERO, Vec2::ZERO),
                    "G" => model.gesture(Vec2::ZERO),
                    "L" => InputAreaTarget::reset(&mut model),
                    "T" => model.update(Ticks::from_f32(args[0])),
                    _ => panic!("{command}"),
                }
                for cue in model.take_sounds() {
                    match cue {
                        "decide" => decide += 1,
                        "error" => reject += 1,
                        _ => panic!("{cue}"),
                    }
                }
                let fields: Vec<&str> = line.split_whitespace().collect();
                let integer = |i: usize| fields[i].parse::<i32>().unwrap();
                let scalar = |i: usize| fields[i].parse::<f32>().unwrap();
                let selection = model.selected.map_or(-1, |row| row as i32);
                assert_eq!(
                    [
                        integer(1),
                        integer(2),
                        integer(3),
                        integer(4),
                        integer(5),
                        integer(8),
                        integer(9),
                        integer(10)
                    ],
                    [
                        selection,
                        if model.result.is_some() {
                            -1
                        } else if model.confirming {
                            1
                        } else {
                            0
                        },
                        model.pointer_row.map_or(-1, |row| row as i32),
                        model.result.is_some() as i32,
                        selection,
                        consumed,
                        decide,
                        reject
                    ],
                    "{name} step{step} {command}"
                );
                let mut expected = vec![model.finger.x, model.finger.y];
                let mut actual = vec![scalar(6), scalar(7)];
                for row in 0..15 {
                    expected.extend([model.focus[row].value(), model.flash[row].value()]);
                    actual.extend([scalar(11 + row * 2), scalar(12 + row * 2)]);
                }
                expected.push(model.instruction_opacity.value());
                actual.push(scalar(41));
                for (field, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
                    assert!(
                        (actual - expected).abs() < 2e-6,
                        "{name} step{step} {command} field{field}: {actual} vs {expected}"
                    );
                }
            }
            eprintln!(
                "{name}:{} production/native Cake command rows, phase/getter/captured row/cues/finger and31native tween values match",
                commands.len()
            );
        }
        std::fs::remove_file(binary).unwrap();
    }
}

#[cfg(test)]
impl CakeLayer {
    pub(crate) fn reference_plain_owner_probe(&self) -> (usize, Box<dyn Fn() -> usize>) {
        let model = &self.inner_ref().model;
        let id = Arc::as_ptr(model) as usize;
        let weak = Arc::downgrade(model);
        (id, Box::new(move || weak.strong_count()))
    }
    pub(crate) fn reference_plain_owner_clocks(
        &self,
        command: &str,
        queued: usize,
        delta: f32,
    ) -> Vec<(u32, bool)> {
        let mut model = self.inner_ref().model.lock().unwrap();
        if command == "selection" {
            model.selected = Some(7);
        }
        let CakeModel {
            focus,
            flash,
            instruction_opacity,
            ..
        } = &mut *model;
        let clocks = focus
            .iter_mut()
            .zip(flash)
            .flat_map(|(a, b)| [a, b])
            .chain(std::iter::once(instruction_opacity))
            .collect::<Vec<_>>();
        clocks
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                super::plain_owner_tests::clock_command(t, command, i, queued, delta);
                (t.value().to_bits(), !t.is_idle())
            })
            .collect()
    }
}

// Owned interactive views dispatch through the same actual layer.
impl super::InteractiveView<'_, CakeLayer> {
    #[cfg(test)]
    pub(crate) fn reference_draw_command(&self, command: &str) {
        self.with(|layer| layer.reference_draw_command(command))
    }
    #[cfg(test)]
    pub(crate) fn reference_controller_state(&self) -> (usize, usize, i32) {
        self.with(|layer| layer.reference_controller_state())
    }
    pub fn reset(&self) {
        self.with(|layer| layer.reset())
    }
    pub fn restore_completed_answer(&self, answer: i32) {
        self.with(|layer| layer.restore_completed_answer(answer))
    }
    #[cfg(test)]
    pub(crate) fn reference_visual_slice(&self) -> Option<usize> {
        self.with(|layer| layer.reference_visual_slice())
    }
    pub fn pointer(&self, position: Vec2, clicked: bool) {
        self.with(|layer| layer.pointer(position, clicked))
    }
    pub fn navigate(&self, delta: i32) {
        self.with(|layer| layer.navigate(delta))
    }
    pub fn confirm(&self) {
        self.with(|layer| layer.confirm())
    }
    pub(crate) fn handle_controller_input(
        &self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
        axes: Vec2,
    ) {
        self.with(|layer| layer.handle_controller_input(state, axes))
    }
    pub(crate) fn pointer_frame(
        &self,
        frame: &crate::adv::input_area::PointerFrame<'_>,
        extent: (u16, u16),
    ) {
        self.with(|layer| layer.pointer_frame(frame, extent))
    }
    pub(crate) fn ignore_pointer_state(&self) {
        self.with(|layer| layer.ignore_pointer_state())
    }
    pub(crate) fn take_sounds(&self) -> Vec<&'static str> {
        self.with(|layer| layer.take_sounds())
    }
    #[cfg(test)]
    pub(crate) fn reference_feedback(
        &self,
    ) -> (Vec2, Option<usize>, bool, f32, [f32; 15], [f32; 15]) {
        self.with(|layer| layer.reference_feedback())
    }
    #[cfg(test)]
    pub(crate) fn reference_update(&self, ticks: f32) {
        self.with(|layer| layer.reference_update(ticks))
    }
    pub fn result(&self) -> Option<i32> {
        self.with(|layer| layer.result())
    }
}
