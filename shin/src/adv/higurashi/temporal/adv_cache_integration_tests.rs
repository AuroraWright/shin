//! Production retained ADV cache against all64 original resource/packet frames.
//! Direct Tile publication, blank Message artwork, admission/request and SDK
//! dimension services are explicit fixture seams. Real Frame/Screen callbacks,
//! eager renderer, cache getter/copy/composition and outer draws are production.
use super::adv_cache_pixels::{self as reference, Operation};
use crate::adv::higurashi::{
    frame::HigurashiFrame, frame_renderer::FrameRenderer, layers::HigurashiLayers,
    message_owner::HigurashiMessageOwner, message_playback::MessageVoice, reference_gpu_assets,
    reference_render_to_texture, scene::HigurashiScene, scene_textures::HigurashiSceneTextures,
    touch::Overlay,
};
use crate::layer::{
    render_params::higurashi::HigurashiTransform, user::higurashi::HigurashiUserLayer,
};
use glam::{Vec2, Vec3, Vec4};
use shin_core::{
    primitives::color::{FloatColor4, UnormColor},
    time::Ticks,
};
use shin_render::{
    DrawPrimitive, LayerFragmentShader, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    render_texture::RenderTexture,
    shaders::types::{
        buffer::VertexSource,
        texture::{TextureTarget, TextureTargetKind},
        vertices::PosTexVertex,
    },
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Voice;
impl MessageVoice for Voice {
    fn is_playing(&self) -> bool {
        false
    }
    fn position_micros(&self) -> u64 {
        0
    }
    fn play(&mut self, _: &str, _: f32, _: bool, _: u32, _: u32) -> bool {
        false
    }
    fn stop(&mut self) {}
}
struct TileOverlay(HigurashiUserLayer);
impl Overlay for TileOverlay {
    fn owner_id(&self) -> i32 {
        self.0.owner_id()
    }
    fn input_permissions(&self) -> crate::adv::higurashi::input::OverlayInput {
        crate::adv::higurashi::input::OverlayInput {
            menus: true,
            hide_window: true,
        }
    }
    fn probe(&mut self, _: Vec2) -> bool {
        false
    }
    fn press(&mut self, _: Vec2) {}
    fn release(&mut self, _: Vec2) {}
    fn cancel(&mut self) {}
    fn priority(&self) -> f32 {
        self.0.properties.values()[5]
    }
    fn advance(&mut self, delta: Ticks) {
        self.0.advance(delta, None);
    }
    fn render(
        &self,
        pass: &mut shin_render::render_pass::RenderPass,
        stencil: u8,
        kind: PassKind,
    ) -> anyhow::Result<()> {
        self.0.render(pass, &Default::default(), stencil, kind)
    }
}
fn publish_copy(
    ctx: &mut crate::render::PreRenderContext,
    target: &wgpu::TextureView,
    source: &RenderTexture,
) {
    let projection = crate::layer::render_params::multiply_transforms(
        HigurashiTransform::default().compute_native_final_transform(),
        glam::Mat4::from_translation(Vec3::new(-640., -720., 0.)),
    );
    let vertices = [(0., 0.), (1280., 0.), (0., 720.), (1280., 720.)].map(|(x, y)| PosTexVertex {
        position: Vec2::new(x, y),
        texture_position: Vec2::new(x / 1280., y / 720.),
    });
    let mut pass = ctx.begin_pass(
        TextureTarget {
            view: target,
            kind: TextureTargetKind::Screen,
        },
        None,
        "Retained native observation copy",
    );
    pass.run(RenderRequestBuilder::new().build(
        RenderProgramWithArguments::HigurashiPostprocess {
            vertices: VertexSource::VertexData {
                vertices: &vertices,
            },
            texture: source.as_texture_source(),
            transform: HigurashiTransform::convert_clip_depth(projection),
            color: FloatColor4::WHITE,
            operation: LayerFragmentShader::Default,
            param: Vec4::ZERO,
        },
        DrawPrimitive::TrianglesStrip,
    ));
}

#[test]
#[ignore = "128 original ADV cache operations/64 exact packet-replayed frames; production cache/eager renderer, explicit blank-art/control/dimension services; no audio/window/saves"]
fn higurashi_live_adv_cache_updates_captures_and_outer_frames_match_original() {
    let corpus = reference::corpus();
    let (device, queue, assets) = reference_gpu_assets();
    let mut frame: Option<HigurashiFrame> = None;
    let mut renderer: Option<FrameRenderer> = None;
    let mut screens: Option<HigurashiSceneTextures> = None;
    let mut overlay: Option<TileOverlay> = None;
    let mut weak = None;
    let mut extent = [0, 0];
    let (mut enabled, mut paused) = (true, false);
    let (mut modal, mut navigation): (Option<RenderTexture>, Option<RenderTexture>) = (None, None);
    let mut voice = Voice;
    let mut prepares = 0;
    let mut ids = BTreeMap::new();
    let mut observations = Vec::new();
    for row in &corpus.rows {
        match row.operation {
            Operation::New { dimension_mode } => {
                assert!(frame.is_none());
                extent = if dimension_mode == 0 {
                    [1920, 1080]
                } else {
                    [1280, 720]
                };
                let new = HigurashiFrame::new(
                    HigurashiScene::new(HigurashiLayers::new(|| [1, 2, 3, 4])),
                    shin_tasks::block_on(HigurashiMessageOwner::load(&assets)).unwrap(),
                );
                new.cache.reference_set_extent(extent).unwrap();
                weak = Some(new.cache.reference_weak());
                screens = Some(
                    HigurashiSceneTextures::for_scene(&device, &queue, extent, &new.scene).unwrap(),
                );
                renderer = Some(FrameRenderer::new(&device, &queue, &new).unwrap());
                assert_eq!(new.cache.reference_snapshot().color_id, None);
                assert_eq!(new.cache.reference_snapshot().depth_id, None);
                frame = Some(new);
                enabled = true;
                paused = false;
                prepares = 0;
                ids.clear();
            }
            Operation::Tile {
                where_,
                slot,
                id,
                params,
            } => {
                let mut owner = HigurashiUserLayer::tile(
                    FloatColor4::from_4bpp_property(params[0]),
                    Vec4::new(
                        params[1] as f32,
                        params[2] as f32,
                        params[3] as f32,
                        params[4] as f32,
                    ),
                    [0; 4],
                );
                owner.set_owner_id(id);
                owner
                    .properties
                    .tweener_mut(0)
                    .unwrap()
                    .fast_forward_to(-640.);
                owner
                    .properties
                    .tweener_mut(1)
                    .unwrap()
                    .fast_forward_to(720.);
                if where_ == 2 {
                    assert_eq!(slot, 0);
                    overlay = Some(TileOverlay(owner));
                } else {
                    frame
                        .as_mut()
                        .unwrap()
                        .scene
                        .layers_mut()
                        .unwrap()
                        .reference_publish_physical(where_ as u8, slot as u8, owner)
                        .unwrap();
                }
            }
            Operation::Property { actor, index, bits } => {
                let layers = frame.as_mut().unwrap().scene.layers_mut().unwrap();
                let props = match actor {
                    0..=2 => layers.reference_physical_properties_mut(actor).unwrap(),
                    _ => panic!("Corpus does not change ADV98 identity/Page properties"),
                };
                props
                    .tweener_mut(index as usize)
                    .unwrap()
                    .fast_forward_to(f32::from_bits(bits));
            }
            Operation::Update { request, delta, .. } | Operation::Callback { request, delta } => {
                // The native oracle's checked logical-control endpoint runs
                // before its full visual gate. Preserve that same explicit seam.
                if let Operation::Update {
                    enabled: e,
                    paused: p,
                    ..
                } = row.operation
                {
                    enabled = e;
                    paused = p;
                }
                if request == 1 {
                    reference_render_to_texture(
                        &device,
                        &queue,
                        (extent[0], extent[1]),
                        |ctx, _| {
                            modal = Some(
                                screens
                                    .as_ref()
                                    .unwrap()
                                    .reference_capture_modal_with_extent(
                                        ctx,
                                        (extent[0] / 2, extent[1] / 2),
                                    )
                                    .unwrap(),
                            );
                        },
                    );
                    enabled = false;
                    paused = true;
                } else if request == 2 {
                    reference_render_to_texture(
                        &device,
                        &queue,
                        (extent[0], extent[1]),
                        |ctx, _| {
                            navigation = Some(frame.as_ref().unwrap().cache.capture(ctx));
                        },
                    );
                    enabled = false;
                }
                let frame = frame.as_mut().unwrap();
                let before = frame.visual_epoch();
                if enabled && !paused {
                    if let Some(o) = &mut overlay {
                        o.advance(Ticks::from_f32(f32::from_bits(delta)));
                    }
                }
                frame.advance(
                    Ticks::from_f32(f32::from_bits(delta)),
                    enabled,
                    paused,
                    &mut voice,
                    None,
                );
                if frame.visual_epoch() != before {
                    renderer
                        .as_mut()
                        .unwrap()
                        .prepare(frame, overlay.as_ref().map(|o| o as &dyn Overlay))
                        .unwrap();
                    prepares += 1;
                }
            }
            Operation::ScreenUpdate { delta } => frame.as_mut().unwrap().scene.advance(
                Ticks::from_f32(f32::from_bits(delta)),
                true,
                false,
                None,
            ),
            Operation::Prepare => {
                renderer
                    .as_mut()
                    .unwrap()
                    .prepare(
                        frame.as_ref().unwrap(),
                        overlay.as_ref().map(|o| o as &dyn Overlay),
                    )
                    .unwrap();
                prepares += 1;
            }
            Operation::ModalEnter => {
                reference_render_to_texture(&device, &queue, (extent[0], extent[1]), |ctx, _| {
                    modal = Some(
                        screens
                            .as_ref()
                            .unwrap()
                            .reference_capture_modal_with_extent(
                                ctx,
                                (extent[0] / 2, extent[1] / 2),
                            )
                            .unwrap(),
                    );
                });
                enabled = false;
                paused = true;
            }
            Operation::ModalExit => {
                enabled = true;
                paused = false;
            }
            Operation::NavigationEndpointFinish => {
                enabled = true;
            }
            Operation::Capture { kind, index } => {
                let o = &corpus.manifest.observations[observations.len()];
                assert_eq!((o.row, o.index), (row.index, observations.len()));
                assert!(index < 4);
                let before = frame.as_ref().unwrap().visual_epoch();
                let actual = reference_render_to_texture(
                    &device,
                    &queue,
                    (o.extent[0], o.extent[1]),
                    |ctx, target| {
                        let copy = match kind {
                            0 => screens
                                .as_ref()
                                .unwrap()
                                .reference_capture_current(ctx)
                                .unwrap(),
                            1 => frame.as_ref().unwrap().cache.capture(ctx),
                            2 => modal
                                .as_ref()
                                .unwrap()
                                .copy_snapshot(&mut ctx.render_clone_ctx()),
                            3 => navigation
                                .as_ref()
                                .unwrap()
                                .copy_snapshot(&mut ctx.render_clone_ctx()),
                            _ => unreachable!(),
                        };
                        assert_eq!(copy.dimensions(), (o.extent[0], o.extent[1]));
                        publish_copy(ctx, target, &copy);
                    },
                );
                assert_eq!(frame.as_ref().unwrap().visual_epoch(), before);
                observations.push(actual);
            }
            Operation::Draw {
                extent: display,
                pass,
                stencil,
            } => {
                let o = &corpus.manifest.observations[observations.len()];
                assert_eq!(o.row, row.index);
                let before = frame.as_ref().unwrap().cache.reference_snapshot();
                let actual = reference_render_to_texture(
                    &device,
                    &queue,
                    (display[0], display[1]),
                    |ctx, target| {
                        let mut output = ctx.begin_pass(
                            TextureTarget {
                                view: target,
                                kind: TextureTargetKind::Screen,
                            },
                            Some(ctx.depth_stencil),
                            "Actual cached ADV outer draw",
                        );
                        output.clear(
                            Some(UnormColor::from_rgba(0x57, 0x35, 0x21, 255)),
                            Some(0),
                            Some(1.),
                        );
                        if pass == 1 {
                            screens.as_ref().unwrap().render_cached_frame(
                                &mut output,
                                frame.as_ref().unwrap(),
                                stencil as u8,
                            );
                        }
                    },
                );
                assert_eq!(frame.as_ref().unwrap().cache.reference_snapshot(), before);
                observations.push(actual);
            }
            Operation::Dispose => {
                modal = None;
                navigation = None;
                overlay = None;
                renderer = None;
                screens = None;
                frame = None;
                assert!(weak.take().unwrap().upgrade().is_none());
            }
        }
        if let Some(frame) = &frame {
            let cached = frame.cache.reference_snapshot();
            assert_eq!(cached.extent, extent);
            assert_eq!(
                cached.prepares, prepares,
                "row{} prepare callbacks",
                row.index
            );
            assert_eq!(
                cached.color_id.is_some(),
                row.snapshot[4] != 0,
                "row{} cached color",
                row.index
            );
            assert_eq!(
                cached.depth_id.is_some(),
                row.snapshot[5] != 0,
                "row{} cached depth",
                row.index
            );
            assert_eq!(
                (u32::from(enabled), u32::from(paused)),
                (row.snapshot[8], row.snapshot[9])
            );
            let history = frame.scene.history().reference_snapshot();
            assert_eq!(history.0, extent);
            for (native, local) in [
                (row.snapshot[2], history.1),
                (row.snapshot[3], history.2.unwrap_or(0)),
            ] {
                if native == 0 {
                    assert_eq!(local, 0);
                } else {
                    assert_eq!(
                        *ids.entry(native).or_insert(local),
                        local,
                        "same Screen history image across row{}",
                        row.index
                    );
                }
            }
        }
        if let Some(o) = corpus
            .manifest
            .observations
            .iter()
            .find(|o| o.row == row.index)
        {
            let original = reference_render_to_texture(
                &device,
                &queue,
                (o.extent[0], o.extent[1]),
                |ctx, target| reference::gpu::replay(ctx, target, &corpus, o),
            );
            let actual = &observations[o.index];
            let different = actual.iter().zip(&original).filter(|(a, b)| a != b).count();
            if different != 0 {
                let path = std::env::temp_dir()
                    .join(format!("higurashi-live-adv-cache-mismatch-{}", o.index));
                std::fs::write(path.with_extension("actual.rgba"), actual).unwrap();
                std::fs::write(path.with_extension("native.rgba"), &original).unwrap();
            }
            assert_eq!(different, 0, "{} atrow{}", o.name, row.index);
            if o.require_visible_pattern {
                let colors = actual.chunks_exact(4).collect::<BTreeSet<_>>();
                assert!(colors.len() >= 3, "{} visible pattern", o.name);
            }
        }
    }
    assert_eq!(observations.len(), 64);
    for control in &corpus.manifest.comparisons {
        assert_eq!(
            observations[control.baseline] == observations[control.effect],
            control.equal,
            "{}",
            control.reason
        );
    }
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_LIVE_ADV_CACHE_INTEGRATION_SUMMARY") {
        std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"actions":128,"observations":64,"controls":34,"exact_frames":64,"production_cache_and_renderer":true,"dimension_and_control_services_explicit":true})).unwrap()).unwrap();
    }
    println!(
        "Production ADV cache:128 actions/64 exact observations/34 controls; eager admitted callbacks, initial color-only getter, retained Screen/cache identity, paused/modal/navigation captures and output resize; blank Message artwork, physical Tile publication and SDK/control services are explicit."
    );
}
