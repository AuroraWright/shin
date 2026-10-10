//! Native SaveMenu thumbnail visual reconstruction and staged downsampling.
use std::sync::Arc;

use super::VmState;
use crate::{
    asset::{picture::Picture, system::AssetServer},
    layer::{
        DrawableLayer, Layer, LayerGroup, LayerProperties, ScreenLayer, render_layer_without_bg,
        render_params::TransformParams,
        user::{NullLayer, PictureLayer, UserLayer},
    },
    render::{PreRenderContext, top_left_projection_matrix},
};
use anyhow::Result;
use glam::Mat4;
use shin_audio::AudioManager;
use shin_core::{
    format::scenario::Scenario,
    primitives::color::UnormColor,
    vm::command::types::{LayerId, LayerType, LayerbankId},
};
use shin_render::{
    ColorBlendType, RenderRequestBuilder,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    render_texture::RenderTexture,
    resize::{SurfaceResizeSource, ViewportParams},
    shaders::types::texture::DepthStencilTarget,
};

/// Native27230/27260 host canvas scale and d5d50..d5d74 ceil.
pub(super) fn thumbnail_size(canvas: (u32, u32)) -> (u32, u32) {
    (
        (canvas.0 as f32 / 1920.0 * 288.0).ceil().max(1.0) as u32,
        (canvas.1 as f32 / 1080.0 * 162.0).ceil().max(1.0) as u32,
    )
}
/// Native102718..102848: min960x540, integer half per axis, clamped
/// independently to final size. The final per-slot texture is appended at draw.
pub(super) fn thumbnail_stages(target: (u32, u32)) -> Vec<(u32, u32)> {
    assert!(target.0 > 0 && target.1 > 0);
    let mut size = (target.0.max(960), target.1.max(540));
    let mut stages = vec![size];
    while size != target {
        size = ((size.0 / 2).max(target.0), (size.1 / 2).max(target.1));
        stages.push(size);
    }
    stages
}

/// A separate root has only its screen child. No message/voice player or live
/// ADV update context is installed. Movie preview uses the native linked still.
pub(super) struct SaveMenuPreviewScene {
    root: LayerGroup<ScreenLayer>,
}
impl SaveMenuPreviewScene {
    #[cfg(test)]
    pub(super) fn reference_layer(
        &self,
        plane: shin_core::vm::command::types::PlaneId,
        bank: LayerbankId,
    ) -> Option<&UserLayer> {
        self.root
            .get_layer(LayerbankId::new(2))
            .unwrap()
            .page_layer()
            .get_plane(plane)
            .get_layer(bank)
    }

    pub async fn load(
        server: &AssetServer,
        audio: &Arc<AudioManager>,
        scenario: &Scenario,
        state: &VmState,
    ) -> Result<Self> {
        let mut screen = ScreenLayer::new(4);
        screen.set_properties(LayerProperties::from_state(
            &state.layers.screen_layer,
            LayerId::new(0),
            0,
        ));
        screen
            .page_layer_mut()
            .set_properties(LayerProperties::from_state(
                &state.layers.page_layer,
                LayerId::new(0),
                0,
            ));
        for (plane, group) in &state.layers.plane_layergroups {
            let dest = screen.page_layer_mut().get_plane_mut(plane);
            dest.set_properties(LayerProperties::from_state(
                &group.properties,
                LayerId::new(0),
                0,
            ));
            if let Some(mask) = group
                .mask_id
                .repr()
                .and_then(|id| scenario.info_tables().mask_info_opt(id))
            {
                dest.set_mask_texture(server.load(mask.path()).await?, group.mask_flags);
            }
        }
        for (bank, saved) in &state.layers.layerbanks {
            let Some(ty) = saved.layer_type else {
                continue;
            };
            let mut layer = if ty == LayerType::Movie {
                // Native103670..1036a4 maps type6 Movie to type2 Picture.
                // Native103694/1036a8 also changes a rejected Movie ID to Null.
                // Read the full signed32-bit value before looking up its link.
                let still = scenario
                    .info_tables()
                    .movie_info
                    .get(saved.params.0 as usize)
                    .and_then(|movie| {
                        scenario
                            .info_tables()
                            .picture_info
                            .get(movie.linked_picture_id.into_i32() as usize)
                    });
                if let Some(still) = still {
                    let picture: Arc<Picture> = server.load(still.path()).await?;
                    UserLayer::from(PictureLayer::new(picture, Some(still.name.to_string())))
                } else {
                    // Preview changes Movie to Picture; a rejected metadata ID
                    // therefore follows the Picture factory's Null fallback.
                    UserLayer::from(NullLayer::new())
                }
            } else {
                UserLayer::load(
                    &server.context().wgpu_device,
                    server,
                    audio,
                    scenario,
                    (&state.settings).into(),
                    ty,
                    saved.params,
                )
                .await
            };
            layer.set_properties(LayerProperties::from_state(
                &saved.properties,
                saved.layer_id,
                saved.layer_load_counter,
            ));
            screen
                .page_layer_mut()
                .get_plane_mut(saved.plane)
                .add_layer(bank, layer);
        }
        let mut root = LayerGroup::new(Some("SaveMenu preview root".into()));
        root.set_properties(LayerProperties::from_state(
            &state.layers.root_layer_group,
            LayerId::new(0),
            0,
        ));
        root.add_layer(LayerbankId::new(2), screen);
        Ok(Self { root })
    }
    pub fn capture(
        &mut self,
        context: &mut PreRenderContext,
        canvas: (u32, u32),
    ) -> SaveMenuPreview {
        let sizes = thumbnail_stages(thumbnail_size(canvas));
        let source_size = sizes[0];
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            source_size.0,
            source_size.1,
        )));
        let depth = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("SaveMenu preview depth"),
            size: wgpu::Extent3d {
                width: source_size.0,
                height: source_size.1,
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
        let mut ctx = PreRenderContext {
            device: context.device,
            queue: context.queue,
            breakup_texture: context.breakup_texture,
            resize_source: &resize,
            sampler_store: context.sampler_store,
            depth_stencil: DepthStencilTarget { view: &depth_view },
            pipeline_storage: context.pipeline_storage,
            dynamic_buffer: context.dynamic_buffer,
            encoder: context.encoder,
        };
        let mut source = ctx.new_render_texture("SaveMenu preview scene".into());
        let transform = TransformParams::default();
        self.root.pre_render(&mut ctx, &transform);
        {
            let mut pass = ctx.begin_pass(
                source.as_texture_target(),
                Some(ctx.depth_stencil),
                "SaveMenu preview scene",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
            render_layer_without_bg(&mut pass, &transform, &self.root, 0);
        }
        for (width, height) in sizes.into_iter().skip(1) {
            let step_resize = SurfaceResizeSource::new(ViewportParams::both(
                winit::dpi::PhysicalSize::new(width, height),
            ));
            let mut target = RenderTexture::new(
                ctx.device.clone(),
                step_resize.handle(),
                "SaveMenu thumbnail downsample".into(),
            );
            {
                let mut pass = ctx.begin_pass(
                    target.as_texture_target(),
                    None,
                    "SaveMenu thumbnail downsample",
                );
                QuadVertices::new()
                    .with_box(0.0, 0.0, 1920.0, 1080.0)
                    .with_tex_box(0.0, 0.0, 1.0, 1.0)
                    .render_sprite(
                        &mut pass,
                        RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                        source.as_texture_source(),
                        top_left_projection_matrix(),
                    );
            }
            source = target;
        }
        SaveMenuPreview { texture: source }
    }
}

pub(super) struct SaveMenuPreview {
    texture: RenderTexture,
}
impl SaveMenuPreview {
    #[cfg(test)]
    pub(super) fn dimensions(&self) -> (u32, u32) {
        self.texture.dimensions()
    }
    pub fn render(&self, pass: &mut RenderPass, transform: Mat4, builder: RenderRequestBuilder) {
        QuadVertices::new()
            .with_box(6.0, 0.0, 294.0, 162.0)
            .with_tex_box(0.0, 0.0, 1.0, 1.0)
            .render_sprite(
                pass,
                builder.color_blend_type(ColorBlendType::Opaque),
                self.texture.as_texture_source(),
                transform,
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_thumbnail_canvas_scaling_and_halving_chain() {
        assert_eq!(thumbnail_size((1920, 1080)), (288, 162));
        assert_eq!(thumbnail_size((1280, 720)), (192, 108));
        assert_eq!(thumbnail_size((1921, 1081)), (289, 163));
        assert_eq!(
            thumbnail_stages((288, 162)),
            [(960, 540), (480, 270), (288, 162)]
        );
        assert_eq!(
            thumbnail_stages((192, 108)),
            [(960, 540), (480, 270), (240, 135), (192, 108)]
        );
        assert_eq!(
            thumbnail_stages((1200, 200)),
            [(1200, 540), (1200, 270), (1200, 200)]
        );
        assert_eq!(thumbnail_stages((1920, 1080)), [(1920, 1080)]);
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_thumbnail_host_scale_halving_movie_still_and_draw() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0xd5d54, 0xbd4a7921),
            (0xd5d58, 0x1e210921),
            (0xd5d68, 0x1e290021),
            (0xd5d6c, 0xbd466501),
            (0xd5d70, 0x1e210800),
            (0xd5d74, 0x1e290002),
            (0x102718, 0x710f003f),
            (0x102720, 0x71086c5f),
            (0x102728, 0x7108705f),
            (0x102730, 0x321a0fea),
            (0x102820, 0x53017ecb),
            (0x102824, 0x53017ea9),
            (0x10282c, 0x1a8b3196),
            (0x102834, 0x1a893155),
            (0x103670, 0x71001b3f),
            (0x10368c, 0xb94053e1),
            (0x103690, 0x97ff9b2c),
            (0x103698, 0x79c01008),
            (0x10369c, 0x321f03f9),
            (0x1036a0, 0x290a7fe8),
            (0x104134, 0xd63f0100),
            (0x1041e8, 0x97fdb3be),
            (0x1042e8, 0x97ff2d36),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native thumbnail anchor at {address:x}"
            );
        }
        for (address, expected) in [
            (0x159a78, 288.0),
            (0x159664, 162.0),
            (0x159758, 1920.0),
            (0x159614, 1080.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
    }
}

#[cfg(test)]
pub(super) fn assert_reference_preview(
    context: &mut PreRenderContext,
    output: &wgpu::Texture,
    server: &Arc<AssetServer>,
) {
    use shin_core::vm::command::types::{LayerProperty, PlaneId};
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    let scenario: Arc<Scenario> = server.load_sync("/main.snr").unwrap();
    let audio = Arc::new(AudioManager::new_for_tests());
    let mut state = VmState::new();
    state
        .layers
        .root_layer_group
        .set_property(LayerProperty::TranslateX, 100);
    state
        .layers
        .screen_layer
        .set_property(LayerProperty::TranslateX, 30);
    state
        .layers
        .page_layer
        .set_property(LayerProperty::TranslateX, 20);
    state.layers.plane_layergroups[PlaneId::new(0)]
        .properties
        .set_property(LayerProperty::TranslateX, 10);
    for (bank, color, width) in [(0, 0xfedc, 960), (1, 0xf123, 1920)] {
        let layer = &mut state.layers.layerbanks[LayerbankId::new(bank)];
        layer.layer_type = Some(LayerType::Tile);
        layer.layer_id = LayerId::new(bank as u16);
        layer.params = (
            color,
            -960,
            -540,
            width,
            if bank == 0 { 540 } else { 1080 },
            0,
            0,
            0,
        );
        layer.properties.set_property(LayerProperty::TranslateX, 40);
        // Native comparator811b0 sorts RenderPosition descending then logical
        // ID ascending. Put the full-canvas background behind the light patch;
        // equal positions would correctly let logical layer1 cover layer0.
        layer
            .properties
            .set_property(LayerProperty::RenderPosition, if bank == 1 { 1 } else { 0 });
    }
    let mut scene = shin_tasks::block_on(SaveMenuPreviewScene::load(
        server, &audio, &scenario, &state,
    ))
    .unwrap();
    let view = output.create_view(&Default::default());
    for canvas in [(1920, 1080), (1280, 720), (1921, 1081)] {
        let preview = scene.capture(context, canvas);
        assert_eq!(preview.texture.dimensions(), thumbnail_size(canvas));
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                None,
                "SaveMenu thumbnail scalar patches",
            );
            pass.clear(Some(UnormColor::BLACK), None, None);
            preview.render(
                &mut pass,
                top_left_projection_matrix(),
                RenderRequestBuilder::new(),
            );
        }
        let pixels = crate::layer::user::readback_test_texture(context, output);
        // Independently known solid patches: ARGB4 colors, composed translation
        // 100+30+20+10+40=200, displayed in native X6..294/Y0..162.
        for (x, expected) in [
            (10, [0, 0, 0]),
            (45, [238, 221, 204]),
            (145, [238, 221, 204]),
            (165, [238, 221, 204]),
            (170, [238, 221, 204]),
            (190, [17, 34, 51]),
            (260, [17, 34, 51]),
            (300, [0, 0, 0]),
        ] {
            for y in [20, 60, 140] {
                let expected = if y > 81 && expected == [238, 221, 204] {
                    [17, 34, 51]
                } else {
                    expected
                };
                let at = (y * 1920 + x) * 4;
                for channel in 0..3 {
                    assert!(
                        (pixels[at + channel] as i32 - expected[channel]).abs() <= 1,
                        "thumbnail{canvas:?} pixel{x},{y} channel{channel}:{} expected{}",
                        pixels[at + channel],
                        expected[channel]
                    );
                }
            }
        }
    }
    // Real supplied Movie entry must become its linked Picture and never create
    // a MovieLayer/audio source. Use the authored door movie exercised by ADV.
    let movie_id = scenario
        .info_tables()
        .movie_info
        .iter()
        .position(|item| item.name.as_str().eq_ignore_ascii_case("door"))
        .unwrap();
    let still_info = scenario.info_tables().picture_info(
        scenario.info_tables().movie_info[movie_id]
            .linked_picture_id
            .repr()
            .unwrap(),
    );
    eprintln!("SaveMenu Movie door linked still: {}", still_info.path());
    let mut movie_state = VmState::new();
    let movie = &mut movie_state.layers.layerbanks[LayerbankId::new(0)];
    movie.layer_type = Some(LayerType::Movie);
    movie.layer_id = LayerId::new(0);
    movie.params = (movie_id as i32, 1000, 1, 0, 0, 0, 0, 0);
    let mut movie_scene = shin_tasks::block_on(SaveMenuPreviewScene::load(
        server,
        &audio,
        &scenario,
        &movie_state,
    ))
    .unwrap();
    assert!(matches!(
        movie_scene
            .root
            .get_layer(LayerbankId::new(2))
            .unwrap()
            .page_layer()
            .get_plane(PlaneId::new(0))
            .get_layer(LayerbankId::new(0)),
        Some(UserLayer::Picture(_))
    ));
    let preview = movie_scene.capture(context, (1920, 1080));
    {
        let mut pass = context.begin_pass(
            TextureTarget {
                kind: TextureTargetKind::Screen,
                view: &view,
            },
            None,
            "SaveMenu linked movie still",
        );
        pass.clear(Some(UnormColor::BLACK), None, None);
        preview.render(
            &mut pass,
            top_left_projection_matrix(),
            RenderRequestBuilder::new(),
        );
    }
    let pixels = crate::layer::user::readback_test_texture(context, output);
    assert!(
        pixels
            .chunks_exact(4)
            .filter(|p| p[0] > 16 || p[1] > 16 || p[2] > 16)
            .count()
            > 10000
    );
    std::fs::write("/private/tmp/shin-save-menu-movie-preview.rgba", pixels).unwrap();
    eprintln!(
        "SaveMenu thumbnail: three host sizes, independent solid patches and five-level transforms; actual Movie door replaced with linked still Picture"
    );
}
