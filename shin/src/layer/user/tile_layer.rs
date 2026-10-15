use std::fmt::Debug;

use glam::{Vec4, vec3};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerBlendType, PassKind, RenderProgramWithArguments,
    RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{RenderClone, buffer::VertexSource, vertices::PosColVertex},
};

use crate::{
    layer::{
        NewDrawableLayer, NewDrawableLayerWrapper,
        new_drawable_layer::{NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass},
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
    },
    update::{AdvUpdatable, AdvUpdateContext},
};

#[derive(Clone, RenderClone)]
pub struct TileLayerImpl {
    color: FloatColor4,
    rect: Vec4,
}

impl TileLayerImpl {
    pub fn new(color: FloatColor4, rect: Vec4) -> Self {
        Self { color, rect }
    }
}

pub type TileLayer = NewDrawableLayerWrapper<TileLayerImpl>;

impl TileLayer {
    #[cfg(test)]
    pub(crate) fn reference_geometry_words(&self) -> [u32; 8] {
        let inner = self.inner_ref();
        let color = inner.color.into_array();
        let rect = inner.rect.to_array();
        std::array::from_fn(|i| {
            if i < 4 {
                color[i].to_bits()
            } else {
                rect[i - 4].to_bits()
            }
        })
    }
    #[cfg(test)]
    pub(crate) fn reference_with_retained_owner(retained: bool) -> Self {
        let inner = || TileLayerImpl::new(FloatColor4::WHITE, Vec4::ZERO);
        if retained {
            Self::from_retained_plain_layer_constructor(inner)
        } else {
            Self::from_plain_layer_constructor(inner)
        }
    }

    pub fn new(color: FloatColor4, rect: Vec4) -> Self {
        Self::from_inner(TileLayerImpl::new(color, rect))
    }
}

impl NewDrawableLayerNeedsSeparatePass for TileLayerImpl {}

impl NewDrawableLayer for TileLayerImpl {
    #[tracing::instrument(skip_all)]
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        &DrawableParams {
            color_multiplier,
            blend_type,
            fragment_shader,
            shader_param,
        }: &DrawableParams,
        clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        let tinted_color = color_multiplier * self.color;
        let fragment_shader = fragment_shader.simplify(shader_param);

        if tinted_color.a <= 0.0 {
            return;
        }

        let target_pass = if blend_type == LayerBlendType::Type1 && tinted_color.a >= 1.0 {
            PassKind::Opaque
        } else {
            PassKind::Transparent
        };

        if pass_kind != target_pass {
            return;
        }

        assert_eq!(
            clip.mode,
            DrawableClipMode::None,
            "Clipping effect is not implemented"
        );

        let blend_type = match pass_kind {
            PassKind::Opaque => ColorBlendType::Opaque,
            PassKind::Transparent => ColorBlendType::from_regular_layer(blend_type),
        };

        let color = fragment_shader
            .evaluate(tinted_color, shader_param)
            .into_unorm();

        let transform = transform.compute_final_transform();

        let left = self.rect.x;
        let right = self.rect.x + self.rect.z;
        let top = self.rect.y;
        let bottom = self.rect.y + self.rect.w;

        let vertices = &[
            PosColVertex {
                position: vec3(left, top, 0.0),
                color,
            },
            PosColVertex {
                position: vec3(right, top, 0.0),
                color,
            },
            PosColVertex {
                position: vec3(left, bottom, 0.0),
                color,
            },
            PosColVertex {
                position: vec3(right, bottom, 0.0),
                color,
            },
        ];

        pass.push_debug("TileLayer");

        pass.run(
            RenderRequestBuilder::new()
                .depth_stencil_shorthand(stencil_ref, false, false)
                .color_blend_type(blend_type)
                .build(
                    RenderProgramWithArguments::Fill {
                        vertices: VertexSource::VertexData { vertices },
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );

        pass.pop_debug();
    }
}

impl NewDrawableLayerFastForward for TileLayerImpl {
    fn fast_forward(&mut self) {}
}

impl AdvUpdatable for TileLayerImpl {
    fn update(&mut self, _ctx: &AdvUpdateContext) {}
}

impl Debug for TileLayerImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let color = self.color.into_array().map(|v| (v * 255.0) as u8);
        let color = format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            color[0], color[1], color[2], color[3]
        );

        f.debug_tuple("TileLayer").field(&color).finish()
    }
}

#[cfg(test)]
impl TileLayer {
    pub(crate) fn assert_reference_ghosting(
        context: &mut crate::render::PreRenderContext,
        picture: &mut dyn crate::layer::DrawableLayer,
    ) {
        use crate::layer::DrawableLayer;
        use shin_core::{primitives::color::UnormColor, vm::command::types::LayerProperty};
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};

        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Reference drawable history"),
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
        let mut tile = TileLayer::new(FloatColor4::WHITE, Vec4::new(-50.0, -50.0, 100.0, 100.0));
        let set = |tile: &mut dyn DrawableLayer, property, value| {
            tile.properties_mut()
                .property_tweener_mut(property)
                .fast_forward_to(value);
        };
        let render = |tile: &mut dyn DrawableLayer,
                      context: &mut crate::render::PreRenderContext| {
            tile.pre_render(context, &TransformParams::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Reference drawable composite",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), None);
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    tile.render(&mut pass, &TransformParams::default(), 1, kind);
                }
            }
            super::movie_layer::readback_test_texture(context, &target)
        };
        let pixel = |pixels: &[u8], x: usize, y: usize, expected: [u8; 3]| {
            let actual = &pixels[(y * 1920 + x) * 4..][..3];
            for (a, e) in actual.iter().zip(expected) {
                assert!(
                    a.abs_diff(e) <= 2,
                    "pixel ({x}, {y}): {actual:?}, expected {expected:?}"
                );
            }
        };
        // Native CPU fragment evaluation is distinct from the texture shader:
        // Negative ignores W, and Fill2 uses RGB weights toward scalar W.
        for (operation, params, expected) in [
            (4.0, [500.0, 250.0, 750.0, 0.0], [96, 32, 48]),
            (2.0, [100.0, 700.0, 200.0, 500.0], [45, 153, 121]),
            (3.0, [500.0, 250.0, 750.0, 400.0], [83, 121, 124]),
        ] {
            for (property, value) in [
                (LayerProperty::MulColorRed, 250.0),
                (LayerProperty::MulColorGreen, 500.0),
                (LayerProperty::MulColorBlue, 750.0),
                (LayerProperty::FragmentShader, operation),
                (LayerProperty::ShaderParamX, params[0]),
                (LayerProperty::ShaderParamY, params[1]),
                (LayerProperty::ShaderParamZ, params[2]),
                (LayerProperty::ShaderParamW, params[3]),
            ] {
                set(&mut tile, property, value);
            }
            let pixels = render(&mut tile, context);
            pixel(&pixels, 960, 540, expected);
            assert_eq!(pixels[(540 * 1920 + 960) * 4 + 3], 255);
        }
        for property in [
            LayerProperty::MulColorRed,
            LayerProperty::MulColorGreen,
            LayerProperty::MulColorBlue,
            LayerProperty::ShaderParamX,
            LayerProperty::ShaderParamY,
            LayerProperty::ShaderParamZ,
            LayerProperty::ShaderParamW,
        ] {
            set(&mut tile, property, 1000.0);
        }
        set(&mut tile, LayerProperty::FragmentShader, 0.0);
        set(&mut tile, LayerProperty::MulColorAlpha, 500.0);
        set(&mut tile, LayerProperty::MulColorRed, 500.0);
        let direct = render(&mut tile, context);
        pixel(&direct, 960, 540, [64, 128, 128]);
        set(&mut tile, LayerProperty::GhostingAlpha, 500.0);
        let first = render(&mut tile, context);
        let differences: Vec<_> = direct
            .chunks_exact(4)
            .zip(first.chunks_exact(4))
            .enumerate()
            .filter(|(_, (a, b))| a.iter().zip(b.iter()).any(|(a, b)| a.abs_diff(*b) > 2))
            .take(10)
            .collect();
        assert!(
            differences.is_empty(),
            "Neutral source changed pixels: {differences:?}"
        );
        set(&mut tile, LayerProperty::TranslateX, 200.0);
        let second = render(&mut tile, context);
        pixel(&second, 960, 540, [32, 64, 64]);
        pixel(&second, 1160, 540, [64, 128, 128]);
        pixel(&second, 100, 100, [0, 0, 0]);
        set(&mut tile, LayerProperty::TranslateX, 400.0);
        let third = render(&mut tile, context);
        pixel(&third, 960, 540, [16, 32, 32]);
        pixel(&third, 1160, 540, [32, 64, 64]);
        pixel(&third, 1360, 540, [64, 128, 128]);
        set(&mut tile, LayerProperty::GhostingAlpha, 0.0);
        let disabled = render(&mut tile, context);
        pixel(&disabled, 960, 540, [0, 0, 0]);
        pixel(&disabled, 1160, 540, [0, 0, 0]);
        pixel(&disabled, 1360, 540, [64, 128, 128]);
        // Re-enabling must start fresh, rather than reviving the disabled history.
        set(&mut tile, LayerProperty::GhostingAlpha, 500.0);
        assert!(
            disabled
                .iter()
                .zip(render(&mut tile, context))
                .all(|(a, b)| a.abs_diff(b) <= 2),
            "Re-enabled history must start fresh"
        );

        // Native 0x752c0 scales and rotates history around the supplied origin.
        tile = TileLayer::new(FloatColor4::WHITE, Vec4::new(-25.0, -25.0, 50.0, 50.0));
        set(&mut tile, LayerProperty::GhostingAlpha, 500.0);
        set(&mut tile, LayerProperty::TranslateX, 200.0);
        render(&mut tile, context);
        set(&mut tile, LayerProperty::TranslateX, -200.0);
        set(&mut tile, LayerProperty::GhostingZoom, 2000.0);
        set(&mut tile, LayerProperty::GhostingRotation, 250.0);
        set(&mut tile, LayerProperty::GhostingTransformOriginX, 100.0);
        let transformed = render(&mut tile, context);
        pixel(&transformed, 760, 540, [255, 255, 255]);
        pixel(&transformed, 1060, 740, [128, 128, 128]);
        pixel(&transformed, 1160, 540, [0, 0, 0]);

        // Exercise decoded PIC blocks from data.rom through both geometry passes.
        set(picture, LayerProperty::MulColorAlpha, 500.0);
        set(picture, LayerProperty::MulColorRed, 500.0);
        set(picture, LayerProperty::GhostingAlpha, 0.0);
        let direct = render(picture, context);
        assert!(
            direct
                .chunks_exact(4)
                .any(|p| p[0].max(p[1]).max(p[2]) > 10)
        );
        set(picture, LayerProperty::GhostingAlpha, 500.0);
        let indirect = render(picture, context);
        let mismatches = direct
            .iter()
            .zip(&indirect)
            .filter(|(a, b)| a.abs_diff(**b) > 2)
            .count();
        assert_eq!(
            mismatches, 0,
            "PIC tint/alpha must agree across direct and offscreen paths"
        );
    }
}

#[cfg(test)]
impl TileLayer {
    pub(crate) fn assert_reference_dissolve(context: &mut crate::render::PreRenderContext) {
        use crate::layer::{DrawableLayer, Layer};
        use shin_core::{primitives::color::UnormColor, vm::command::types::LayerProperty};
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Reference dissolve"),
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
        let render = |tile: &mut TileLayer, context: &mut crate::render::PreRenderContext| {
            tile.pre_render(context, &TransformParams::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Reference dissolve composite",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), None);
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    tile.render(&mut pass, &TransformParams::default(), 1, kind);
                }
            }
            super::movie_layer::readback_test_texture(context, &target)
        };
        for mode in 0..4 {
            let mut tile = TileLayer::new(
                FloatColor4::WHITE,
                Vec4::new(-960.0, -540.0, 1920.0, 1080.0),
            );
            tile.properties_mut()
                .property_tweener_mut(LayerProperty::DissolveMode)
                .fast_forward_to(mode as f32);
            tile.properties_mut()
                .property_tweener_mut(LayerProperty::MulColorAlpha)
                .fast_forward_to(500.0);
            let mut previous_visible = 1920 * 1080;
            let mut halfway = Vec::new();
            for intensity in [100.0, 500.0, 900.0, 1000.0, 0.0] {
                tile.properties_mut()
                    .property_tweener_mut(LayerProperty::DissolveIntensity)
                    .fast_forward_to(intensity);
                let pixels = render(&mut tile, context);
                let visible = pixels.chunks_exact(4).filter(|p| p[0] > 5).count();
                if intensity > 0.0 {
                    assert!(
                        visible <= previous_visible,
                        "Mode{mode}, intensity{intensity}: coverage{visible} after{previous_visible}"
                    );
                    previous_visible = visible;
                    if intensity == 1000.0 {
                        // Native rank endpoints149/150 exceed denominator148:
                        // modes0/2/3 retain1/3/1 quads smaller than one pixel.
                        assert!(visible <= 3, "Native endpoint remnant: {visible} pixels");
                        if mode == 1 {
                            assert_eq!(visible, 0);
                        }
                    }
                    if intensity == 500.0 {
                        halfway = pixels.clone();
                        assert!(visible > 1000 && visible < 1920 * 1080 - 1000);
                        let x = if mode & 1 != 0 { 1900 } else { 20 };
                        let y = if mode & 2 != 0 { 20 } else { 1060 };
                        assert!(
                            pixels[(y * 1920 + x) * 4].abs_diff(128) <= 2,
                            "Intact corner must retain opacity exactly once"
                        );
                        let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
                        for p in pixels.chunks_exact(4) {
                            ppm.extend_from_slice(&p[..3]);
                        }
                        std::fs::write(format!("/tmp/shin-dissolve-mode-{mode}.ppm"), ppm).unwrap();
                    }
                } else {
                    assert_eq!(
                        visible,
                        1920 * 1080,
                        "Disabling dissolve must restore direct rendering"
                    );
                }
            }
            tile.properties_mut()
                .property_tweener_mut(LayerProperty::DissolveIntensity)
                .fast_forward_to(500.0);
            tile.properties_mut()
                .property_tweener_mut(LayerProperty::GhostingAlpha)
                .fast_forward_to(500.0);
            let combined_first = render(&mut tile, context);
            assert!(
                halfway
                    .iter()
                    .zip(&combined_first)
                    .all(|(a, b)| a.abs_diff(*b) <= 2),
                "First dissolve+ghost frame must retain the same neutral source"
            );
            let combined_second = render(&mut tile, context);
            let first_brightness: u64 = combined_first
                .chunks_exact(4)
                .map(|p| u64::from(p[0]))
                .sum();
            let second_brightness: u64 = combined_second
                .chunks_exact(4)
                .map(|p| u64::from(p[0]))
                .sum();
            assert!(
                second_brightness > first_brightness,
                "History must accumulate after dissolve"
            );
            assert!(
                combined_second.chunks_exact(4).all(|p| p[0] <= 130),
                "Layer opacity must still be applied once after combined effects"
            );
        }
    }
}

#[cfg(test)]
impl TileLayer {
    pub(crate) fn assert_reference_mosaic(context: &mut crate::render::PreRenderContext) {
        use crate::layer::{DrawableLayer, Layer};
        use shin_core::{primitives::color::UnormColor, vm::command::types::LayerProperty};
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Reference centered mosaic"),
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
        let mut tile = TileLayer::new(FloatColor4::WHITE, Vec4::new(20.0, 20.0, 60.0, 60.0));
        let set = |tile: &mut TileLayer, property, value| {
            tile.properties_mut()
                .property_tweener_mut(property)
                .fast_forward_to(value);
        };
        let render = |tile: &mut TileLayer, context: &mut crate::render::PreRenderContext| {
            tile.pre_render(context, &TransformParams::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Reference mosaic composite",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), None);
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    tile.render(&mut pass, &TransformParams::default(), 1, kind);
                }
            }
            super::movie_layer::readback_test_texture(context, &target)
        };
        let pixel = |pixels: &[u8], x: usize, y: usize, expected: [u8; 3]| {
            let actual = &pixels[(y * 1920 + x) * 4..][..3];
            assert!(
                actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                "Mosaic pixel({x},{y}): {actual:?}, expected{expected:?}"
            );
        };
        set(&mut tile, LayerProperty::MulColorAlpha, 500.0);
        set(&mut tile, LayerProperty::MulColorRed, 500.0);
        let direct = render(&mut tile, context);
        pixel(&direct, 960, 540, [0, 0, 0]);
        pixel(&direct, 1010, 590, [64, 128, 128]);
        set(&mut tile, LayerProperty::MosaicSize, 99.0);
        let mosaic = render(&mut tile, context);
        for (x, y) in [(960, 540), (1059, 639), (1010, 590)] {
            pixel(&mosaic, x, y, [64, 128, 128]);
        }
        for (x, y) in [(959, 540), (1060, 540), (960, 539), (960, 640), (910, 590)] {
            pixel(&mosaic, x, y, [0, 0, 0]);
        }
        let visible = mosaic.chunks_exact(4).filter(|p| p[1] > 5).count();
        assert_eq!(visible, 100 * 100, "Size99 forms centered 100pixel cells");
        set(&mut tile, LayerProperty::MosaicSize, 99.9);
        assert!(
            mosaic == render(&mut tile, context),
            "Native truncates fractional property values"
        );
        set(&mut tile, LayerProperty::MosaicSize, 100.0);
        let enlarged = render(&mut tile, context);
        pixel(&enlarged, 1060, 590, [64, 128, 128]);
        pixel(&enlarged, 1061, 590, [0, 0, 0]);
        set(&mut tile, LayerProperty::MosaicSize, 0.0);
        assert!(
            direct == render(&mut tile, context),
            "Disabling restores original geometry"
        );
        set(&mut tile, LayerProperty::MosaicSize, 99.0);
        set(&mut tile, LayerProperty::GhostingAlpha, 500.0);
        let first = render(&mut tile, context);
        let second = render(&mut tile, context);
        assert!(mosaic.iter().zip(&first).all(|(a, b)| a.abs_diff(*b) <= 2));
        assert!(
            first.iter().zip(&second).all(|(a, b)| a.abs_diff(*b) <= 2),
            "History of opaque source cells must not apply layer opacity twice"
        );
    }
}

#[cfg(test)]
impl TileLayer {
    pub(crate) fn assert_reference_raster(
        context: &mut crate::render::PreRenderContext,
        server: &std::sync::Arc<crate::asset::system::AssetServer>,
    ) {
        use crate::layer::{DrawableLayer, Layer};
        use shin_core::{
            primitives::{color::UnormColor, update::FrameId},
            time::Ticks,
            vm::command::types::LayerProperty::*,
        };
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Reference raster"),
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
        let set = |tile: &mut TileLayer, property, value| {
            tile.properties_mut()
                .property_tweener_mut(property)
                .fast_forward_to(value);
        };
        let render = |tile: &mut TileLayer, context: &mut crate::render::PreRenderContext| {
            tile.pre_render(context, &TransformParams::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Reference raster composite",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), None);
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    tile.render(&mut pass, &TransformParams::default(), 1, kind);
                }
            }
            super::movie_layer::readback_test_texture(context, &target)
        };
        let pixel = |pixels: &[u8], x: usize, y: usize, expected: [u8; 3]| {
            let actual = &pixels[(y * 1920 + x) * 4..][..3];
            assert!(
                actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 3),
                "Raster pixel({x},{y}): {actual:?}, expected{expected:?}"
            );
        };
        let mut frame = FrameId::default();
        let mut advance =
            |tile: &mut TileLayer, context: &crate::render::PreRenderContext, delta: f32| {
                frame.advance();
                tile.update(&AdvUpdateContext {
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(delta),
                    asset_server: server,
                    device: context.device,
                    queue: context.queue,
                    are_animations_allowed: false,
                    voice_callback: None,
                    movie_volume_settings: Default::default(),
                });
            };
        let mut tile = TileLayer::new(FloatColor4::WHITE, Vec4::new(-2.5, -540.0, 5.0, 1080.0));
        set(&mut tile, MulColorAlpha, 500.0);
        set(&mut tile, MulColorRed, 500.0);
        set(&mut tile, RasterHorizontalAmplitude, 80.0);
        set(&mut tile, RasterHorizontalLPeriod, 400.0);
        set(&mut tile, RasterHorizontalTPeriod, 100.0);
        let initial = render(&mut tile, context);
        pixel(&initial, 880, 100, [64, 128, 128]);
        pixel(&initial, 960, 100, [0, 0, 0]);
        advance(&mut tile, context, 25.0);
        let quarter = render(&mut tile, context);
        pixel(&quarter, 880, 0, [64, 128, 128]);
        advance(&mut tile, context, 25.0);
        let half = render(&mut tile, context);
        pixel(&half, 1040, 100, [64, 128, 128]);
        pixel(&half, 880, 100, [0, 0, 0]);
        set(&mut tile, RasterHorizontalTPeriod, 0.0);
        advance(&mut tile, context, 100.0);
        assert!(
            half == render(&mut tile, context),
            "Zero period must freeze the retained phase"
        );
        set(&mut tile, RasterHorizontalTPeriod, -100.0);
        advance(&mut tile, context, 25.0);
        assert!(
            quarter == render(&mut tile, context),
            "Negative period must reverse without resetting phase"
        );
        set(&mut tile, RasterHorizontalLPeriod, 0.0);
        let translated = render(&mut tile, context);
        pixel(&translated, 880, 300, [64, 128, 128]);
        pixel(&translated, 960, 300, [0, 0, 0]);
        set(&mut tile, RasterHorizontalAmplitude, 0.0);
        let disabled = render(&mut tile, context);
        pixel(&disabled, 960, 300, [64, 128, 128]);
        pixel(&disabled, 880, 300, [0, 0, 0]);

        let mut vertical = TileLayer::new(FloatColor4::WHITE, Vec4::new(-960.0, -2.5, 1920.0, 5.0));
        set(&mut vertical, RasterVerticalAmplitude, 60.0);
        set(&mut vertical, RasterVerticalLPeriod, 400.0);
        let pixels = render(&mut vertical, context);
        pixel(&pixels, 100, 480, [255, 255, 255]);
        pixel(&pixels, 100, 540, [0, 0, 0]);
        set(&mut vertical, RasterVerticalTPeriod, 100.0);
        advance(&mut vertical, context, 25.0);
        let quarter = render(&mut vertical, context);
        pixel(&quarter, 0, 480, [255, 255, 255]);
        set(&mut vertical, RasterVerticalLPeriod, 0.0);
        set(&mut vertical, RasterVerticalAmplitude, -60.0);
        let reversed = render(&mut vertical, context);
        pixel(&reversed, 100, 600, [255, 255, 255]);
        pixel(&reversed, 100, 480, [0, 0, 0]);
    }
}

#[cfg(test)]
impl TileLayer {
    pub(crate) fn assert_reference_ripple(
        context: &mut crate::render::PreRenderContext,
        server: &std::sync::Arc<crate::asset::system::AssetServer>,
    ) {
        use crate::layer::{DrawableLayer, Layer};
        use shin_core::{
            primitives::{color::UnormColor, update::FrameId},
            time::Ticks,
            vm::command::types::LayerProperty::*,
        };
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Reference ripple"),
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
        let set = |tile: &mut TileLayer, property, value| {
            tile.properties_mut()
                .property_tweener_mut(property)
                .fast_forward_to(value);
        };
        let render = |tile: &mut TileLayer, context: &mut crate::render::PreRenderContext| {
            tile.pre_render(context, &TransformParams::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Reference ripple composite",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), None);
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    tile.render(&mut pass, &TransformParams::default(), 1, kind);
                }
            }
            super::movie_layer::readback_test_texture(context, &target)
        };
        let pixel = |pixels: &[u8], x: usize, y: usize, expected: [u8; 3]| {
            let actual = &pixels[(y * 1920 + x) * 4..][..3];
            assert!(
                actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 3),
                "Ripple pixel({x},{y}): {actual:?}, expected{expected:?}"
            );
        };
        let mut frame = FrameId::default();
        let mut advance =
            |tile: &mut TileLayer, context: &crate::render::PreRenderContext, delta: f32| {
                frame.advance();
                tile.update(&AdvUpdateContext {
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(delta),
                    asset_server: server,
                    device: context.device,
                    queue: context.queue,
                    are_animations_allowed: false,
                    voice_callback: None,
                    movie_volume_settings: Default::default(),
                });
            };
        let mut tile = TileLayer::new(FloatColor4::WHITE, Vec4::new(195.0, -540.0, 10.0, 1080.0));
        set(&mut tile, MulColorAlpha, 500.0);
        set(&mut tile, MulColorRed, 500.0);
        set(&mut tile, RippleAmplitude, 80.0);
        set(&mut tile, RippleLPeriod, 500.0);
        let initial = render(&mut tile, context);
        pixel(&initial, 1080, 540, [64, 128, 128]);
        pixel(&initial, 1160, 540, [0, 0, 0]);
        set(&mut tile, RippleLPeriod, 500.9);
        assert!(
            initial == render(&mut tile, context),
            "Spatial period must truncate before normalization"
        );
        set(&mut tile, RippleTPeriod, 100.0);
        advance(&mut tile, context, 25.0);
        let quarter = render(&mut tile, context);
        pixel(&quarter, 1080, 540, [0, 0, 0]);
        set(&mut tile, RippleTPeriod, 0.0);
        advance(&mut tile, context, 100.0);
        assert!(
            quarter == render(&mut tile, context),
            "Zero temporal period freezes the retained ripple phase"
        );
        set(&mut tile, RippleAmplitude, -80.0);
        let reversed = render(&mut tile, context);
        pixel(&reversed, 1080, 540, [64, 128, 128]);
        set(&mut tile, RippleAmplitude, 80.0);
        set(&mut tile, RippleTPeriod, -100.0);
        advance(&mut tile, context, 25.0);
        let restored_phase = render(&mut tile, context);
        pixel(&restored_phase, 1080, 540, [64, 128, 128]);
        set(&mut tile, RippleLPeriod, 0.9);
        let zero_frequency = render(&mut tile, context);
        pixel(&zero_frequency, 1160, 540, [64, 128, 128]);
        pixel(&zero_frequency, 1080, 540, [0, 0, 0]);
        set(&mut tile, RippleAmplitude, 0.0);
        let disabled = render(&mut tile, context);
        pixel(&disabled, 1160, 540, [64, 128, 128]);

        // Same UV amplitude gives45 vertical pixels for80 horizontal pixels.
        let mut vertical =
            TileLayer::new(FloatColor4::WHITE, Vec4::new(-960.0, 110.0, 1920.0, 5.0));
        set(&mut vertical, RippleAmplitude, 80.0);
        set(&mut vertical, RippleLPeriod, 500.0);
        let pixels = render(&mut vertical, context);
        pixel(&pixels, 960, 607, [255, 255, 255]);
        pixel(&pixels, 960, 652, [0, 0, 0]);
        let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
        for p in initial.chunks_exact(4) {
            ppm.extend_from_slice(&p[..3]);
        }
        std::fs::write("/tmp/shin-ripple-reference.ppm", ppm).unwrap();
    }
}

#[cfg(test)]
impl TileLayer {
    pub(crate) fn assert_reference_blur(
        context: &mut crate::render::PreRenderContext,
        _server: &std::sync::Arc<crate::asset::system::AssetServer>,
    ) {
        use crate::layer::{DrawableLayer, Layer};
        use shin_core::{primitives::color::UnormColor, vm::command::types::LayerProperty::*};
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Reference blur"),
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
        let set = |tile: &mut TileLayer, property, value| {
            tile.properties_mut()
                .property_tweener_mut(property)
                .fast_forward_to(value);
        };
        let render = |tile: &mut TileLayer, context: &mut crate::render::PreRenderContext| {
            tile.pre_render(context, &TransformParams::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Reference blur composite",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), None);
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    tile.render(&mut pass, &TransformParams::default(), 1, kind);
                }
            }
            super::movie_layer::readback_test_texture(context, &target)
        };
        // Independent scalar model of native full-sized targets with half-size
        // viewports. It retains pixels outside each viewport across the swaps.
        fn linear(sample: &impl Fn(i32, i32) -> f32, x: f32, y: f32) -> f32 {
            let ix = x.floor() as i32;
            let iy = y.floor() as i32;
            let fx = x - ix as f32;
            let fy = y - iy as f32;
            let get = |x: i32, y: i32| sample(x.clamp(0, 1919), y.clamp(0, 1079));
            let top = get(ix, iy) * (1.0 - fx) + get(ix + 1, iy) * fx;
            let bottom = get(ix, iy + 1) * (1.0 - fx) + get(ix + 1, iy + 1) * fx;
            top * (1.0 - fy) + bottom * fy
        }
        fn native_pixel(rect: [i32; 4], x: i32, y: i32, radius: f32) -> u8 {
            let original = |x: i32, y: i32| {
                if x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3] {
                    1.0
                } else {
                    0.0
                }
            };
            let quantize = |v: f32| (v * 255.0).round() / 255.0;
            let down = |x: i32, y: i32| {
                if x < 960 && y < 540 {
                    quantize(linear(
                        &original,
                        2.0 * x as f32 + 0.5,
                        2.0 * y as f32 + 0.5,
                    ))
                } else {
                    0.0
                }
            };
            let weights = [0x3eceed6bu32, 0x3ea127be, 0x3e437dbe, 0x3db8afdf];
            let horizontal = |x: i32, y: i32| {
                if x >= 960 || y >= 540 {
                    return original(x, y);
                }
                let mut sum = 0.0;
                for (i, bits) in weights.iter().enumerate() {
                    let offset = (i as f32 + 0.5) * radius;
                    sum += (linear(&down, x as f32 - offset, y as f32)
                        + linear(&down, x as f32 + offset, y as f32))
                        * (f32::from_bits(*bits) * 0.5);
                }
                quantize(sum)
            };
            let vertical = |x: i32, y: i32| {
                if x >= 960 || y >= 540 {
                    return down(x, y);
                }
                let mut sum = 0.0;
                for (i, bits) in weights.iter().enumerate() {
                    let offset = (i as f32 + 0.5) * radius;
                    sum += (linear(&horizontal, x as f32, y as f32 - offset)
                        + linear(&horizontal, x as f32, y as f32 + offset))
                        * (f32::from_bits(*bits) * 0.5);
                }
                quantize(sum)
            };
            (linear(&vertical, x as f32 * 0.5 - 0.25, y as f32 * 0.5 - 0.25) * 255.0).round() as u8
        }
        for rect in [[955, 0, 10, 1080], [0, 535, 1920, 10], [0, 0, 1920, 1080]] {
            let mut tile = TileLayer::new(
                FloatColor4::WHITE,
                Vec4::new(
                    (rect[0] - 960) as f32,
                    (rect[1] - 540) as f32,
                    rect[2] as f32,
                    rect[3] as f32,
                ),
            );
            set(&mut tile, BlurRadius, 10_000.0);
            let pixels = render(&mut tile, context);
            for (x, y) in [
                (890, 540),
                (915, 540),
                (950, 540),
                (960, 540),
                (965, 540),
                (1000, 540),
                (1025, 540),
                (960, 470),
                (960, 495),
                (960, 530),
                (960, 545),
                (960, 580),
                (960, 605),
                (0, 0),
                (1919, 0),
                (0, 1079),
                (1919, 1079),
                (1910, 1060),
            ] {
                let expected = native_pixel(rect, x, y, 10.0);
                let actual = &pixels[((y * 1920 + x) * 4) as usize..][..3];
                assert!(
                    actual.iter().all(|a| a.abs_diff(expected) <= 3),
                    "Blur rect{rect:?} pixel({x},{y}): {actual:?}, native scalar{expected}"
                );
            }
            // Symmetric sample pairs make a negative radius equivalent.
            // New layer keeps the same initial scratch contents as the first render.
            let mut negative = TileLayer::new(
                FloatColor4::WHITE,
                Vec4::new(
                    (rect[0] - 960) as f32,
                    (rect[1] - 540) as f32,
                    rect[2] as f32,
                    rect[3] as f32,
                ),
            );
            set(&mut negative, BlurRadius, -10_000.0);
            assert!(
                pixels == render(&mut negative, context),
                "Negative blur radius changes symmetric filter"
            );
            set(&mut tile, BlurRadius, 0.0);
            let disabled = render(&mut tile, context);
            assert_eq!(disabled[((540 * 1920 + 960) * 4) as usize], 255);
            if rect[2] == 10 {
                assert_eq!(disabled[(540 * 1920 + 950) * 4], 0);
            }
            if rect[3] == 10 {
                assert_eq!(disabled[(530 * 1920 + 960) * 4], 0);
            }
            // Keep a visible fixture with a vertical stripe and one-time tint/opacity.
            if rect[2] == 10 {
                set(&mut negative, MulColorAlpha, 500.0);
                set(&mut negative, MulColorRed, 500.0);
                let tinted = render(&mut negative, context);
                for x in [915, 950, 960, 965, 1000] {
                    let index = (540 * 1920 + x) * 4;
                    let base = pixels[index] as f32;
                    assert!(tinted[index].abs_diff((base * 0.25).round() as u8) <= 2);
                    assert!(tinted[index + 1].abs_diff((base * 0.5).round() as u8) <= 2);
                }
                let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
                for p in tinted.chunks_exact(4) {
                    ppm.extend_from_slice(&p[..3]);
                }
                std::fs::write("/tmp/shin-blur-reference.ppm", ppm).unwrap();
            }
        }
    }
}

#[cfg(test)]
impl TileLayer {
    pub(crate) fn assert_reference_zoom_blur(
        context: &mut crate::render::PreRenderContext,
        _server: &std::sync::Arc<crate::asset::system::AssetServer>,
    ) {
        use crate::layer::{DrawableLayer, Layer};
        use shin_core::{primitives::color::UnormColor, vm::command::types::LayerProperty::*};
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Reference zoom blur"),
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
        let set = |tile: &mut TileLayer, property, value| {
            tile.properties_mut()
                .property_tweener_mut(property)
                .fast_forward_to(value);
        };
        let render = |tile: &mut TileLayer, context: &mut crate::render::PreRenderContext| {
            tile.pre_render(context, &TransformParams::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Reference zoom blur composite",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), None);
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    tile.render(&mut pass, &TransformParams::default(), 1, kind);
                }
            }
            super::movie_layer::readback_test_texture(context, &target)
        };
        fn linear(sample: &impl Fn(i32, i32) -> f32, x: f32, y: f32) -> f32 {
            let ix = x.floor() as i32;
            let iy = y.floor() as i32;
            let fx = x - ix as f32;
            let fy = y - iy as f32;
            let get = |x: i32, y: i32| sample(x.clamp(0, 1919), y.clamp(0, 1079));
            let top = get(ix, iy) * (1.0 - fx) + get(ix + 1, iy) * fx;
            let bottom = get(ix, iy + 1) * (1.0 - fx) + get(ix + 1, iy + 1) * fx;
            top * (1.0 - fy) + bottom * fy
        }
        fn native_pixel(rect: [i32; 4], x: i32, y: i32, strength: f32, center: [f32; 2]) -> u8 {
            let original = |x: i32, y: i32| {
                if x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3] {
                    1.0
                } else {
                    0.0
                }
            };
            let quantize = |v: f32| (v * 255.0).round() / 255.0;
            let down = |x: i32, y: i32| {
                if x < 960 && y < 540 {
                    quantize(linear(
                        &original,
                        2.0 * x as f32 + 0.5,
                        2.0 * y as f32 + 0.5,
                    ))
                } else {
                    0.0
                }
            };
            let focus = [center[0] * 0.5 + 479.5, center[1] * 0.5 + 269.5];
            let zoom = |x: i32, y: i32| {
                if x >= 960 || y >= 540 {
                    return original(x, y);
                }
                let mut sum = 0.0;
                let mut scale = 1.0;
                for index in 0..8 {
                    if index == 1 {
                        scale = 1.0 / (1.0 + strength * 0.000005);
                    } else if index > 1 {
                        scale *= 0.99;
                    }
                    let sx = focus[0] + (x as f32 - focus[0]) * scale;
                    let sy = focus[1] + (y as f32 - focus[1]) * scale;
                    sum += linear(&down, sx, sy) * (8 - index) as f32 / 36.0;
                }
                quantize(sum)
            };
            (linear(&zoom, x as f32 * 0.5 - 0.25, y as f32 * 0.5 - 0.25) * 255.0).round() as u8
        }
        for (rect, strength, center) in [
            ([1155, 0, 10, 1080], 20_000.0, [0.0, 0.0]),
            ([1155, 0, 10, 1080], -20_000.0, [0.0, 0.0]),
            ([1155, 0, 10, 1080], 20_000.0, [560.0, -100.0]),
            ([0, 645, 1920, 10], 20_000.0, [0.0, 0.0]),
            ([0, 645, 1920, 10], -20_000.0, [-300.0, 260.0]),
            ([0, 0, 1920, 1080], 20_000.0, [0.0, 0.0]),
        ] {
            let mut tile = TileLayer::new(
                FloatColor4::WHITE,
                Vec4::new(
                    (rect[0] - 960) as f32,
                    (rect[1] - 540) as f32,
                    rect[2] as f32,
                    rect[3] as f32,
                ),
            );
            set(&mut tile, Prop70, strength);
            set(&mut tile, Prop71, center[0]);
            set(&mut tile, Prop72, center[1]);
            let pixels = render(&mut tile, context);
            let mut points = vec![
                (0, 0),
                (1919, 0),
                (0, 1079),
                (1919, 1079),
                (1910, 1060),
                (960, 540),
            ];
            points.extend((1090..1260).step_by(5).map(|x| (x, 540)));
            points.extend((610..725).step_by(5).map(|y| (960, y)));
            for (x, y) in points {
                let expected = native_pixel(rect, x, y, strength, center);
                let actual = &pixels[((y * 1920 + x) * 4) as usize..][..3];
                assert!(
                    actual.iter().all(|a| a.abs_diff(expected) <= 3),
                    "Zoom blur rect{rect:?} strength{strength} center{center:?} pixel({x},{y}): {actual:?}, native scalar{expected}"
                );
            }
            if rect[0] == 1155 && strength > 0.0 && center == [0.0, 0.0] {
                set(&mut tile, MulColorAlpha, 500.0);
                set(&mut tile, MulColorRed, 500.0);
                let tinted = render(&mut tile, context);
                for x in (1100..1250).step_by(5) {
                    let i = (540 * 1920 + x) * 4;
                    assert!(tinted[i].abs_diff((pixels[i] as f32 * 0.25).round() as u8) <= 2);
                    assert!(tinted[i + 1].abs_diff((pixels[i] as f32 * 0.5).round() as u8) <= 2);
                }
                let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
                for pixel in tinted.chunks_exact(4) {
                    ppm.extend_from_slice(&pixel[..3]);
                }
                std::fs::write("/tmp/shin-zoom-blur-reference.ppm", ppm).unwrap();
            }
            set(&mut tile, MulColorAlpha, 1000.0);
            set(&mut tile, MulColorRed, 1000.0);
            set(&mut tile, Prop70, 0.0);
            let disabled = render(&mut tile, context);
            if rect[0] == 1155 {
                assert_eq!(disabled[(540 * 1920 + 1160) * 4], 255);
            }
            if rect[1] == 645 {
                assert_eq!(disabled[(650 * 1920 + 960) * 4], 255);
            }
        }
    }
}

#[cfg(test)]
impl TileLayer {
    pub(crate) fn assert_reference_clipping(
        context: &mut crate::render::PreRenderContext,
        _server: &std::sync::Arc<crate::asset::system::AssetServer>,
    ) {
        use crate::layer::{DrawableLayer, Layer};
        use shin_core::{primitives::color::UnormColor, vm::command::types::LayerProperty::*};
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Reference clipping"),
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
        let set = |tile: &mut TileLayer, property, value| {
            tile.properties_mut()
                .property_tweener_mut(property)
                .fast_forward_to(value);
        };
        let render = |tile: &mut TileLayer, context: &mut crate::render::PreRenderContext| {
            tile.pre_render(context, &TransformParams::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Reference clipping composite",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), None);
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    tile.render(&mut pass, &TransformParams::default(), 1, kind);
                }
            }
            super::movie_layer::readback_test_texture(context, &target)
        };
        let pixel = |pixels: &[u8], x: usize, y: usize, expected: u8| {
            let actual = &pixels[(y * 1920 + x) * 4..][..3];
            assert!(
                actual.iter().all(|a| a.abs_diff(expected) <= 2),
                "Clip pixel({x},{y}): {actual:?}, expected{expected}"
            );
        };
        let mut tile = TileLayer::new(
            FloatColor4::WHITE,
            Vec4::new(-2000.0, -2000.0, 4000.0, 4000.0),
        );
        set(&mut tile, ClipMode, 1.0);
        // Raw bank IDs follow executable order62=x1,63=y1,64=x2,65=y2.
        for (property, value) in [
            (ClipFromX, -100.0),
            (ClipFromY, -50.0),
            (ClipToX, 100.0),
            (ClipToY, 50.0),
        ] {
            set(&mut tile, property, value);
        }
        assert_eq!(ClipFromY as u8, 63);
        assert_eq!(ClipToX as u8, 64);
        let initial = render(&mut tile, context);
        for (x, y, e) in [
            (960, 540, 255),
            (860, 490, 255),
            (1059, 589, 255),
            (859, 540, 0),
            (1060, 540, 0),
            (960, 489, 0),
            (960, 590, 0),
        ] {
            pixel(&initial, x, y, e);
        }
        // Both rectangle endpoints are normalized independently.
        set(&mut tile, ClipFromX, 100.0);
        set(&mut tile, ClipToX, -100.0);
        set(&mut tile, ClipFromY, 50.0);
        set(&mut tile, ClipToY, -50.0);
        assert!(initial == render(&mut tile, context));
        set(&mut tile, TranslateX, 200.0);
        let translated = render(&mut tile, context);
        pixel(&translated, 960, 540, 0);
        pixel(&translated, 1160, 540, 255);
        pixel(&translated, 1259, 589, 255);
        set(&mut tile, ClipMode, 2.0);
        let screen = render(&mut tile, context);
        assert!(screen == initial, "Mode2 must ignore layer translation");
        set(&mut tile, ClipMode, 1.0);
        set(&mut tile, TranslateX, 0.0);
        set(&mut tile, Rotation, 250.0);
        let rotated = render(&mut tile, context);
        pixel(&rotated, 960, 630, 255);
        pixel(&rotated, 1040, 540, 0);
        set(&mut tile, Rotation, 125.0);
        let diagonal = render(&mut tile, context);
        pixel(&diagonal, 1015, 595, 255);
        pixel(&diagonal, 1015, 485, 0);
        set(&mut tile, Rotation, 0.0);
        set(&mut tile, ScaleX, -2000.0);
        let flipped = render(&mut tile, context);
        pixel(&flipped, 1130, 540, 255);
        pixel(&flipped, 1170, 540, 0);
        // Offscreen effects are clipped only during final composition.
        set(&mut tile, ScaleX, 1000.0);
        set(&mut tile, MosaicSize, 99.0);
        set(&mut tile, MulColorAlpha, 500.0);
        let indirect = render(&mut tile, context);
        pixel(&indirect, 960, 540, 128);
        pixel(&indirect, 1060, 540, 0);
        pixel(&indirect, 960, 590, 0);
        set(&mut tile, ClipMode, 0.0);
        let disabled = render(&mut tile, context);
        pixel(&disabled, 1500, 800, 128);
        // Native zero-area rectangles skip the clip mask rather than hide the layer.
        set(&mut tile, ClipMode, 1.0);
        set(&mut tile, ClipToX, 100.0);
        assert!(
            disabled == render(&mut tile, context),
            "Zero width must skip native clipping"
        );
        set(&mut tile, ClipToX, -100.0);
        set(&mut tile, ClipFromY, f32::NAN);
        assert!(
            disabled == render(&mut tile, context),
            "NaN rectangles must skip native clipping"
        );
        // Clearing the reserved clip bit must preserve existing lower stencil bits.
        set(&mut tile, ClipFromY, 50.0);
        set(&mut tile, MosaicSize, 0.0);
        set(&mut tile, MulColorAlpha, 1000.0);
        tile.pre_render(context, &TransformParams::default());
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Clip stencil preservation",
            );
            pass.clear(Some(UnormColor::BLACK), Some(3), None);
            for kind in [PassKind::Opaque, PassKind::Transparent] {
                tile.render(&mut pass, &TransformParams::default(), 2, kind);
            }
            let follower = TileLayer::new(
                FloatColor4::WHITE,
                Vec4::new(-960.0, -540.0, 1920.0, 1080.0),
            );
            for kind in [PassKind::Opaque, PassKind::Transparent] {
                follower.render(&mut pass, &TransformParams::default(), 2, kind);
            }
        }
        let preserved = super::movie_layer::readback_test_texture(context, &target);
        pixel(&preserved, 960, 540, 0);
        pixel(&preserved, 1500, 800, 0);
        // A higher order following layer must render outside the earlier clip.
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Clip mask cleanup",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), None);
            for kind in [PassKind::Opaque, PassKind::Transparent] {
                tile.render(&mut pass, &TransformParams::default(), 1, kind);
            }
            let follower = TileLayer::new(
                FloatColor4::WHITE,
                Vec4::new(-960.0, -540.0, 1920.0, 1080.0),
            );
            for kind in [PassKind::Opaque, PassKind::Transparent] {
                follower.render(&mut pass, &TransformParams::default(), 2, kind);
            }
        }
        let cleaned = super::movie_layer::readback_test_texture(context, &target);
        pixel(&cleaned, 1500, 800, 255);
        pixel(&cleaned, 960, 540, 255);
    }
}

#[cfg(test)]
impl TileLayer {
    pub(crate) fn assert_reference_containers(
        context: &mut crate::render::PreRenderContext,
        server: &std::sync::Arc<crate::asset::system::AssetServer>,
    ) {
        use crate::layer::{DrawableLayer, Layer};
        use shin_core::{primitives::color::UnormColor, vm::command::types::LayerProperty::*};
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        let target = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Reference containers"),
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
        let set = |tile: &mut dyn DrawableLayer, property, value| {
            tile.properties_mut()
                .property_tweener_mut(property)
                .fast_forward_to(value);
        };
        let render = |tile: &mut dyn Layer, context: &mut crate::render::PreRenderContext| {
            tile.pre_render(context, &TransformParams::default());
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Reference containers composite",
                );
                pass.clear(Some(UnormColor::BLACK), Some(0), None);
                for kind in [PassKind::Opaque, PassKind::Transparent] {
                    tile.render(&mut pass, &TransformParams::default(), 1, kind);
                }
            }
            super::movie_layer::readback_test_texture(context, &target)
        };
        use crate::layer::{LayerGroup, PageLayer, ScreenLayer, user::UserLayer};
        use shin_core::vm::command::types::{LayerbankId, MaskFlags, PlaneId};
        let pixel = |pixels: &[u8], x: usize, y: usize, expected: u8| {
            let actual = &pixels[(y * 1920 + x) * 4..][..3];
            assert!(
                actual.iter().all(|a| a.abs_diff(expected) <= 2),
                "Container pixel({x},{y}): {actual:?}, expected{expected}"
            );
        };
        let clip = |layer: &mut dyn DrawableLayer| {
            for (p, v) in [
                (ClipMode, 1.0),
                (ClipFromX, -100.0),
                (ClipFromY, -100.0),
                (ClipToX, 100.0),
                (ClipToY, 100.0),
            ] {
                set(layer, p, v);
            }
        };
        let mut page = PageLayer::new(1);
        page.get_plane_mut(PlaneId::new(0)).add_layer(
            LayerbankId::new(0),
            UserLayer::Tile(TileLayer::new(
                FloatColor4::WHITE,
                Vec4::new(-10.0, -10.0, 20.0, 20.0),
            )),
        );
        set(&mut page, TranslateX, 200.0);
        clip(&mut page);
        let pixels = render(&mut page, context);
        pixel(&pixels, 1160, 540, 255);
        pixel(&pixels, 1360, 540, 0);
        pixel(&pixels, 960, 540, 0);
        // Nested screen/page/group captures must compose each ancestor once.
        let mut screen = ScreenLayer::new(1);
        set(&mut screen, TranslateX, 200.0);
        clip(&mut screen);
        // Screen clip is deliberately wider than the child position.
        set(&mut screen, ClipFromX, -500.0);
        set(&mut screen, ClipToX, 500.0);
        let page = screen.page_layer_mut();
        set(page, TranslateX, 100.0);
        clip(page);
        let group = page.get_plane_mut(PlaneId::new(0));
        set(group, TranslateX, 50.0);
        clip(group);
        let mut tile = TileLayer::new(FloatColor4::WHITE, Vec4::new(-10.0, -10.0, 20.0, 20.0));
        set(&mut tile, TranslateX, 25.0);
        group.add_layer(LayerbankId::new(0), UserLayer::Tile(tile));
        let pixels = render(&mut screen, context);
        pixel(&pixels, 1335, 540, 255);
        pixel(&pixels, 1535, 540, 0);
        pixel(&pixels, 1435, 540, 0);
        let mask: std::sync::Arc<crate::asset::mask::MaskTexture> =
            shin_tasks::block_on(server.load("/mask/1.msk")).unwrap();
        let mut outside_visible = false;
        for flags in [MaskFlags::SCALE, MaskFlags::SCALE | MaskFlags::INVERT] {
            let mut group = LayerGroup::new(Some("Reference mask clip".into()));
            group.add_layer(
                LayerbankId::new(0),
                TileLayer::new(
                    FloatColor4::WHITE,
                    Vec4::new(-960.0, -540.0, 1920.0, 1080.0),
                ),
            );
            group.set_mask_texture(mask.clone(), flags);
            let baseline = render(&mut group, context);
            clip(&mut group);
            let clipped = render(&mut group, context);
            for y in 0..1080usize {
                for x in 0..1920usize {
                    let i = (y * 1920 + x) * 4;
                    if (860..1060).contains(&x) && (440..640).contains(&y) {
                        assert_eq!(
                            &clipped[i..i + 3],
                            &baseline[i..i + 3],
                            "Mask/clip interior({x},{y})"
                        );
                    } else {
                        outside_visible |= baseline[i..i + 3].iter().any(|v| *v > 0);
                        assert_eq!(
                            &clipped[i..i + 3],
                            &[0, 0, 0],
                            "Mask/clip exterior({x},{y})"
                        );
                    }
                }
            }
        }
        assert!(
            outside_visible,
            "Mask fixture must contain visible content outside the clip"
        );
    }
}

#[cfg(test)]
impl TileLayer {
    pub(crate) fn assert_reference_whirl(
        context: &mut crate::render::PreRenderContext,
        server: &std::sync::Arc<crate::asset::system::AssetServer>,
        scenario: &shin_core::format::scenario::Scenario,
    ) {
        use crate::wiper::{AnyWiper, Wiper};
        use shin_core::{primitives::update::FrameId, time::Ticks, vm::command::types::WiperType};
        use shin_render::shaders::types::texture::{
            TextureSampler, TextureSource, TextureTarget, TextureTargetKind,
        };
        let make_texture = |label, usage| {
            context.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 1920,
                    height: 1080,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage,
                view_formats: &[],
            })
        };
        let source = make_texture(
            "Whirl source",
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let target = make_texture(
            "Whirl target",
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        for (texture, is_target) in [(&source, false), (&target, true)] {
            let mut pixels = Vec::with_capacity(1920 * 1080 * 4);
            for y in 0..1080 {
                for x in 0..1920 {
                    let r = (x * 255 / 1919) as u8;
                    let g = (y * 255 / 1079) as u8;
                    pixels.extend_from_slice(&if is_target {
                        [255 - g, r, 230, 255]
                    } else {
                        [r, g, 20, 255]
                    });
                }
            }
            context.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1920 * 4),
                    rows_per_image: Some(1080),
                },
                texture.size(),
            );
        }
        let source_view = source.create_view(&Default::default());
        let target_view = target.create_view(&Default::default());
        let output = make_texture(
            "Whirl output",
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let view = output.create_view(&Default::default());
        let render = |wiper: &AnyWiper, context: &mut crate::render::PreRenderContext| {
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Whirl fixture",
                );
                wiper.render(
                    &mut pass,
                    RenderRequestBuilder::new(),
                    TextureSource {
                        view: &target_view,
                        sampler: TextureSampler::Linear,
                    },
                    TextureSource {
                        view: &source_view,
                        sampler: TextureSampler::Linear,
                    },
                );
            }
            super::movie_layer::readback_test_texture(context, &output)
        };
        let mut frame = FrameId::default();
        let mut update =
            |wiper: &mut AnyWiper, context: &crate::render::PreRenderContext, delta, allowed| {
                frame.advance();
                wiper.update(&AdvUpdateContext {
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(delta),
                    asset_server: server,
                    device: context.device,
                    queue: context.queue,
                    are_animations_allowed: allowed,
                    voice_callback: None,
                    movie_volume_settings: Default::default(),
                });
            };
        for direction in [0, 1, 2] {
            let mut wiper = shin_tasks::block_on(AnyWiper::load(
                server,
                scenario,
                WiperType::Whirl,
                Ticks::from_f32(100.0),
                (direction, 0, 0, 0, 0, 0, 0, 0),
            ));
            for step in 0..=4 {
                if step != 0 {
                    update(&mut wiper, context, 25.0, true);
                }
                assert_eq!(wiper.is_running(), step < 4);
                let pixels = render(&wiper, context);
                let p = step as f32 * 0.25;
                let fade = p * p * (3.0 - 2.0 * p);
                let pulse = 1.0 - 2.0 * (p - 0.5).abs();
                let pulse = pulse * pulse * (3.0 - 2.0 * pulse);
                for y in [0usize, 160, 400, 540, 800, 1079] {
                    for x in [0usize, 180, 550, 960, 1200, 1650, 1919] {
                        let dx = (x as f32 + 0.5) / 1920.0 - 0.5;
                        let dy = (y as f32 + 0.5) / 1080.0 - 0.5;
                        let radius = (dx * dx + dy * dy).sqrt();
                        let angle = (fade * if direction == 1 { 1.0 } else { -1.0 }
                            + 0.3 * pulse * (radius * std::f32::consts::PI * 10.0).sin())
                            * std::f32::consts::TAU;
                        let ux = dx * angle.cos() - dy * angle.sin() + 0.5;
                        let uy = dx * angle.sin() + dy * angle.cos() + 0.5;
                        let r = ((ux * 1920.0 - 0.5).clamp(0.0, 1919.0) / 1919.0) * 255.0;
                        let g = ((uy * 1080.0 - 0.5).clamp(0.0, 1079.0) / 1079.0) * 255.0;
                        let expected = [
                            r * (1.0 - fade) + (255.0 - g) * fade,
                            g * (1.0 - fade) + r * fade,
                            20.0 * (1.0 - fade) + 230.0 * fade,
                        ];
                        let actual = &pixels[(y * 1920 + x) * 4..][..3];
                        assert!(
                            actual
                                .iter()
                                .zip(expected)
                                .all(|(a, b)| a.abs_diff(b.round() as u8) <= 3),
                            "Whirl direction{direction} progress{p} pixel({x},{y}): {actual:?}, scalar{expected:?}"
                        );
                    }
                }
                if step == 1 {
                    update(&mut wiper, context, 25.0, false);
                    assert!(
                        pixels == render(&wiper, context),
                        "Disabled animation freezes transition"
                    );
                }
                if direction == 0 && step == 2 {
                    let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
                    for p in pixels.chunks_exact(4) {
                        ppm.extend_from_slice(&p[..3]);
                    }
                    std::fs::write("/tmp/shin-whirl-reference.ppm", ppm).unwrap();
                }
            }
            let mut wiper = shin_tasks::block_on(AnyWiper::load(
                server,
                scenario,
                WiperType::Whirl,
                Ticks::from_f32(100.0),
                (direction, 0, 0, 0, 0, 0, 0, 0),
            ));
            update(&mut wiper, context, 25.0, true);
            wiper.fast_forward();
            assert!(!wiper.is_running());
            let end = render(&wiper, context);
            assert!(end[(540 * 1920 + 960) * 4 + 2].abs_diff(230) <= 1);
        }
    }
}

#[cfg(test)]
impl TileLayer {
    pub(crate) fn assert_reference_glass(
        context: &mut crate::render::PreRenderContext,
        server: &std::sync::Arc<crate::asset::system::AssetServer>,
        scenario: &shin_core::format::scenario::Scenario,
    ) {
        use crate::wiper::{AnyWiper, Wiper};
        use shin_core::{primitives::update::FrameId, time::Ticks, vm::command::types::WiperType};
        use shin_render::shaders::types::texture::{
            TextureSampler, TextureSource, TextureTarget, TextureTargetKind,
        };
        let make_texture = |label, usage| {
            context.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 1920,
                    height: 1080,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage,
                view_formats: &[],
            })
        };
        let source = make_texture(
            "Glass source",
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let target = make_texture(
            "Glass target",
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        for (texture, is_target) in [(&source, false), (&target, true)] {
            let mut pixels = Vec::with_capacity(1920 * 1080 * 4);
            for y in 0..1080 {
                for x in 0..1920 {
                    let r = (x * 255 / 1919) as u8;
                    let g = (y * 255 / 1079) as u8;
                    pixels.extend_from_slice(&if is_target {
                        [255 - g, r, 230, 255]
                    } else {
                        [r, g, 20, 80]
                    });
                }
            }
            context.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1920 * 4),
                    rows_per_image: Some(1080),
                },
                texture.size(),
            );
        }
        let source_view = source.create_view(&Default::default());
        let target_view = target.create_view(&Default::default());
        let output = make_texture(
            "Glass output",
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let view = output.create_view(&Default::default());
        let render = |wiper: &AnyWiper, context: &mut crate::render::PreRenderContext| {
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Glass fixture",
                );
                wiper.render(
                    &mut pass,
                    RenderRequestBuilder::new(),
                    TextureSource {
                        view: &target_view,
                        sampler: TextureSampler::Linear,
                    },
                    TextureSource {
                        view: &source_view,
                        sampler: TextureSampler::Linear,
                    },
                );
            }
            super::movie_layer::readback_test_texture(context, &output)
        };
        let mut frame = FrameId::default();
        let mut update =
            |wiper: &mut AnyWiper, context: &crate::render::PreRenderContext, delta, allowed| {
                frame.advance();
                wiper.update(&AdvUpdateContext {
                    frame_id: frame,
                    delta_ticks: Ticks::from_f32(delta),
                    asset_server: server,
                    device: context.device,
                    queue: context.queue,
                    are_animations_allowed: allowed,
                    voice_callback: None,
                    movie_volume_settings: Default::default(),
                });
            };
        let mut loaded = shin_tasks::block_on(AnyWiper::load(
            server,
            scenario,
            WiperType::Glass,
            Ticks::from_f32(100.0),
            (99, 88, 77, 66, 55, 44, 33, 22),
        ));
        assert!(matches!(loaded, AnyWiper::Glass(_)));
        loaded.fast_forward();
        assert!(!loaded.is_running());
        let target_pixels = render(&loaded, context);
        for &ticks in &[0.0, 25.0, 50.0, 87.5, 100.0] {
            let mut wiper = AnyWiper::Glass(crate::wiper::glass::GlassWiper::with_seed(
                Ticks::from_f32(100.0),
                1,
            ));
            update(&mut wiper, context, ticks, true);
            let pixels = render(&wiper, context);
            for y in [0usize, 160, 400, 540, 800, 1079] {
                for x in [0usize, 180, 550, 960, 1200, 1650, 1919] {
                    let expected = crate::wiper::glass::GlassWiper::reference_pixel(
                        ticks,
                        ticks / 100.0,
                        x,
                        y,
                    );
                    let actual = &pixels[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 4];
                    for i in 0..4 {
                        assert!(
                            (i16::from(actual[i]) - i16::from(expected[i])).abs() <= 3,
                            "Glass ticks={ticks} point=({x},{y}) channel={i} actual={actual:?} expected={expected:?}"
                        );
                    }
                }
            }
            if ticks == 50.0 {
                let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
                for pixel in pixels.chunks_exact(4) {
                    ppm.extend_from_slice(&pixel[..3]);
                }
                std::fs::write("/tmp/shin-glass-reference.ppm", ppm).unwrap();
                assert_ne!(pixels, target_pixels);
                let before = pixels;
                update(&mut wiper, context, 5.0, false);
                let paused = render(&wiper, context);
                assert!(wiper.is_running());
                assert_ne!(
                    before, paused,
                    "Native Glass physics continues while timer pauses"
                );
                wiper.fast_forward();
                assert_eq!(render(&wiper, context), target_pixels);
            }
        }
    }
}

#[cfg(test)]
impl TileLayer {
    pub(crate) fn assert_reference_notifications(
        context: &mut crate::render::PreRenderContext,
        server: &std::sync::Arc<crate::asset::system::AssetServer>,
    ) {
        use crate::adv::information::{NotificationKind, NotificationQueue};
        use shin_core::{
            format::rom::RomReader, primitives::stateless_reader::StatelessFile, time::Ticks,
        };
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        use std::{fs::File, io::Read};
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let entry = rom.find_file("/adv.txa").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let archive = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let original = &archive.textures[archive.name_to_index["notify"]];
        assert_eq!((original.width(), original.height()), (640, 128));
        let output = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Notification fixture"),
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
        let view = output.create_view(&Default::default());
        for kind in [NotificationKind::Characters, NotificationKind::Tips] {
            let mut queue = NotificationQueue::default();
            queue.load_textures(server);
            queue.enqueue(kind, 1);
            let mut previous = 0.0;
            for time in [0.0f32, 15.0, 30.0, 180.0, 210.0, 240.0] {
                let started = queue.update(Ticks::from_f32(time - previous));
                assert_eq!(started, if time == 0.0 { Some(kind as u8) } else { None });
                previous = time;
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view: &view,
                            kind: TextureTargetKind::Screen,
                        },
                        None,
                        "Native notification",
                    );
                    pass.clear(
                        Some(shin_core::primitives::color::UnormColor::BLACK),
                        None,
                        None,
                    );
                    queue.render(&mut pass);
                }
                let pixels = super::movie_layer::readback_test_texture(context, &output);
                let left = 1380.0
                    - 100.0
                        * if time < 30.0 {
                            (time / 30.0 * std::f32::consts::FRAC_PI_2).sin()
                        } else {
                            1.0
                        };
                let opacity = if time < 30.0 {
                    time / 30.0
                } else if time < 180.0 {
                    1.0
                } else {
                    ((240.0 - time) / 60.0).max(0.0)
                };
                let opacity = (opacity * 255.0).round() / 255.0;
                let row = if kind == NotificationKind::Characters {
                    0
                } else {
                    64
                };
                let mut visible = 0;
                for y in 0..1080usize {
                    for x in 0..1920usize {
                        let actual = &pixels[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 4];
                        let mut expected = [0.0f32; 3];
                        if time < 240.0
                            && (100..164).contains(&y)
                            && x as f32 + 0.5 >= left
                            && x as f32 + 0.5 < left + 640.0
                        {
                            let sample_x = (x as f32 - left).clamp(0.0, 639.0);
                            let a = original
                                .get_pixel(sample_x.floor() as u32, (y - 100 + row) as u32)
                                .0;
                            let b = original
                                .get_pixel(
                                    (sample_x.floor() as u32 + 1).min(639),
                                    (y - 100 + row) as u32,
                                )
                                .0;
                            let fraction = sample_x.fract();
                            let sampled: [f32; 4] = std::array::from_fn(|i| {
                                f32::from(a[i]) * (1.0 - fraction) + f32::from(b[i]) * fraction
                            });
                            for i in 0..3 {
                                expected[i] = sampled[i] * sampled[3] / 255.0 * opacity;
                            }
                        }
                        for i in 0..3 {
                            assert!(
                                (f32::from(actual[i]) - expected[i]).abs() <= 2.1,
                                "Notification kind={kind:?} time={time} pixel=({x},{y}) actual={actual:?} expected={expected:?}"
                            );
                        }
                        assert_eq!(actual[3], 255);
                        visible += usize::from(actual[..3].iter().any(|&c| c > 0));
                    }
                }
                assert_eq!(visible > 0, time > 0.0 && time < 240.0);
                if kind == NotificationKind::Characters && time == 30.0 {
                    let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
                    for p in pixels.chunks_exact(4) {
                        ppm.extend_from_slice(&p[..3]);
                    }
                    std::fs::write("/tmp/shin-notification-reference.ppm", ppm).unwrap();
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn assert_reference_character_portraits(
        context: &mut crate::render::PreRenderContext,
        server: &std::sync::Arc<crate::asset::system::AssetServer>,
    ) {
        use crate::adv::character_view::CharacterTextures;
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        use std::{fs::File, io::Read};
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let entry = rom.find_file("/chars.txa").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let original = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let atlas: std::sync::Arc<CharacterTextures> = server.load_sync("/chars.txa").unwrap();
        let output = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Native character portraits"),
            size: wgpu::Extent3d {
                width: 304,
                height: 258,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = output.create_view(&Default::default());
        let projection = glam::camera::rh::proj::directx::orthographic(0.0, 304.0, 258.0, 0.0, -1.0, 1.0);
        let names = ["bea0", "eva0", "null"];
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                None,
                "Character portrait modes",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                None,
                None,
            );
            for (row, name) in names.iter().enumerate() {
                for mode in 0..4u8 {
                    let x = f32::from(mode) * 76.0;
                    let y = row as f32 * 86.0;
                    atlas.render_portrait(
                        &mut pass,
                        name,
                        mode,
                        glam::vec4(x, y, x + 76.0, y + 86.0),
                        projection,
                    );
                }
            }
        }
        let pixels = super::movie_layer::readback_test_texture(context, &output);
        let frame = &original.textures[original.name_to_index["non"]];
        let mut transparent = 0;
        let mut opaque = 0;
        for (row, name) in names.iter().enumerate() {
            let source = &original.textures[original.name_to_index[*name]];
            assert_eq!((source.width(), source.height()), (76, 86));
            for mode in 0..4u8 {
                for y in 0..86u32 {
                    for x in 0..76u32 {
                        let src = source.get_pixel(x, y).0;
                        let base = frame.get_pixel(x, y).0;
                        let alpha = f32::from(src[3]) / 255.0;
                        let mut color: [f32; 3] = std::array::from_fn(|i| f32::from(src[i]));
                        if mode == 1
                            || (mode == 3
                                && (y as f32 + 0.5) / 86.0 > 1.0 - (x as f32 + 0.5) / 76.0)
                        {
                            color[1] *= 0.1;
                            color[2] *= 0.1;
                        }
                        if mode == 2 {
                            color = [color[0] * f32::from_bits(0x3e991687)
                                + color[1] * 0.587
                                + color[2] * 0.114; 3];
                        }
                        let offset =
                            ((row * 86 + y as usize) * 304 + mode as usize * 76 + x as usize) * 4;
                        let actual = &pixels[offset..offset + 4];
                        for i in 0..3 {
                            let expected = color[i] * alpha + f32::from(base[i]) * (1.0 - alpha);
                            assert!(
                                (f32::from(actual[i]) - expected).abs() <= 1.1,
                                "Portrait{name} mode{mode} ({x},{y}): {actual:?}, expected channel{i}={expected}"
                            );
                        }
                        assert_eq!(actual[3], 255);
                        transparent += usize::from(src[3] == 0);
                        opaque += usize::from(src[3] == 255);
                    }
                }
            }
        }
        assert!(transparent > 0 && opaque > 0);
        let mut ppm = b"P6\n304 258\n255\n".to_vec();
        for p in pixels.chunks_exact(4) {
            ppm.extend_from_slice(&p[..3]);
        }
        std::fs::write("/tmp/shin-character-portraits-reference.ppm", ppm).unwrap();
    }

    #[cfg(test)]
    pub(crate) fn readback_reference_texture(
        context: &mut crate::render::PreRenderContext,
        texture: &wgpu::Texture,
    ) -> Vec<u8> {
        super::movie_layer::readback_test_texture(context, texture)
    }

    pub(crate) fn assert_reference_character_buttons(
        context: &mut crate::render::PreRenderContext,
        server: &std::sync::Arc<crate::asset::system::AssetServer>,
    ) {
        use crate::adv::character_view::CharacterTextures;
        use shin_core::{
            format::rom::RomReader,
            primitives::{color::UnormColor, stateless_reader::StatelessFile},
        };
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        use std::{fs::File, io::Read};
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let entry = rom.find_file("/chars.txa").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let original = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let atlas: std::sync::Arc<CharacterTextures> = server.load_sync("/chars.txa").unwrap();
        let output = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Native button flashes"),
            size: wgpu::Extent3d {
                width: 384,
                height: 288,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = output.create_view(&Default::default());
        let controls = [
            ("button", 0usize, 372usize, 128usize),
            ("lr", 128, 368, 56),
            ("close", 184, 84, 84),
        ];
        let cases = [
            (Vec4::ZERO, Vec4::ZERO, [255u8; 4]),
            (Vec4::ZERO, glam::vec4(1.0, 1.0, 1.0, 0.5), [255; 4]),
            (Vec4::ZERO, glam::vec4(1.0, 1.0, 1.0, 1.5), [255; 4]),
            (
                glam::vec4(0.8, 0.2, 0.6, 0.25),
                glam::vec4(0.1, 0.9, 0.3, 0.5),
                [255; 4],
            ),
            (
                glam::vec4(0.8, 0.2, 0.6, 0.25),
                glam::vec4(0.1, 0.9, 0.3, 0.5),
                [123, 205, 61, 173],
            ),
            (Vec4::ZERO, Vec4::ZERO, [128, 128, 128, 255]),
        ];
        for (case, (flash1, flash2, tint)) in cases.into_iter().enumerate() {
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Native button RGB flashes",
                );
                pass.clear(Some(UnormColor::from_rgba(32, 64, 96, 128)), None, None);
                for (name, y, w, h) in controls {
                    let image = &original.textures[original.name_to_index[name]];
                    assert_eq!((image.width(), image.height()), (w as u32, h as u32));
                    // The small target preserves one source texel per output pixel.
                    let transform = glam::camera::rh::proj::directx::orthographic(0.0, 384.0, 288.0, 0.0, -1.0, 1.0,
                    );
                    atlas.render_button(
                        &mut pass,
                        name,
                        glam::vec4(0.0, y as f32, w as f32, (y + h) as f32),
                        glam::vec4(0.0, 0.0, 1.0, 1.0),
                        UnormColor::from_rgba(tint[0], tint[1], tint[2], tint[3]),
                        flash1,
                        flash2,
                        transform,
                    );
                }
            }
            let pixels = super::movie_layer::readback_test_texture(context, &output);
            let mut worst = 0.0f32;
            for y in 0..288 {
                for x in 0..384 {
                    let mut expected = [32.0f32, 64.0, 96.0, 128.0];
                    for (name, top, w, h) in controls {
                        if x >= w || y < top || y >= top + h {
                            continue;
                        }
                        let src = original.textures[original.name_to_index[name]]
                            .get_pixel(x as u32, (y - top) as u32)
                            .0;
                        let alpha = src[3] as f32 / 255.0 * tint[3] as f32 / 255.0;
                        for c in 0..3 {
                            let first =
                                src[c] as f32 / 255.0 * (1.0 - flash1.w) + flash1[c] * flash1.w;
                            let second = first * (1.0 - flash2.w) + flash2[c] * flash2.w;
                            let rgb = (second * tint[c] as f32 / 255.0).clamp(0.0, 1.0);
                            expected[c] =
                                (rgb * alpha * 255.0 + expected[c] * (1.0 - alpha)).round();
                        }
                        expected[3] = (alpha * (255.0 - 128.0) + 128.0).round();
                    }
                    for c in 0..4 {
                        let actual = pixels[(y * 384 + x) * 4 + c];
                        let error = (actual as f32 - expected[c]).abs();
                        worst = worst.max(error);
                        assert!(
                            error <= 2.1,
                            "button case{case} ({x},{y}) channel{c}: actual{actual}, expected{}, error{error}",
                            expected[c]
                        );
                    }
                }
            }
            println!("Native character button case{case}:110592 RGBA pixels, worst error{worst}");
        }
    }

    pub(crate) fn assert_reference_character_chrome(
        context: &mut crate::render::PreRenderContext,
        server: &std::sync::Arc<crate::asset::system::AssetServer>,
        scenario: &shin_core::format::scenario::Scenario,
    ) {
        use crate::adv::{character_view::CharacterTextures, characters::CharacterModel};
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        use std::{fs::File, io::Read};
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let tables = scenario.info_tables();
        let model =
            CharacterModel::new(&tables.chars_grid_info[0], &tables.chars_sprite_info, 0, 0);
        let full_name = &model
            .selected_cell()
            .unwrap()
            .definition()
            .state()
            .unwrap()
            .variant()
            .unwrap()
            .full_sprite;
        assert!(!full_name.is_empty());
        let full_path = format!("/chars/{}.txa", full_name.to_ascii_lowercase());
        let mut decode = |path: &str| {
            let entry = rom.find_file(path).unwrap();
            let mut bytes = Vec::new();
            rom.open_file(entry)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap()
        };
        let original = decode("/chars.txa");
        let full_original = decode(&full_path);
        // Native0x118220 selects archive texture index0, independently of its name.
        let full_texture_name = full_original
            .name_to_index
            .iter()
            .find(|(_, index)| **index == 0)
            .unwrap()
            .0;
        let atlas: std::sync::Arc<CharacterTextures> = server.load_sync("/chars.txa").unwrap();
        let full: std::sync::Arc<CharacterTextures> = server.load_sync(&full_path).unwrap();
        let output = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Native character screen chrome"),
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
        let view = output.create_view(&Default::default());
        for progress in [0.0f32, 0.5, 1.0] {
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    None,
                    "Native character chrome",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::from_rgba(
                        32, 64, 96, 255,
                    )),
                    None,
                    None,
                );
                let projection = crate::render::top_left_projection_matrix();
                atlas.render_background(&mut pass, progress, projection);
                full.render_full_sprite(
                    &mut pass,
                    CharacterTextures::body_transform(progress, projection),
                );
                atlas.render_header(&mut pass, progress, projection);
            }
            let actual = super::movie_layer::readback_test_texture(context, &output);
            let mut expected = vec![[32.0 / 255.0, 64.0 / 255.0, 96.0 / 255.0, 1.0]; 1920 * 1080];
            // Independent scalar rasterizer over original ROM pixels. No renderer geometry,
            // transformation helpers or GPU textures enter the expected image.
            let mut draw =
                |name: &str, rect: glam::Vec4, uv: glam::Vec4, alpha: f32, additive: bool| {
                    let image = if name == "full" {
                        &full_original.textures[0]
                    } else {
                        &original.textures[original.name_to_index[name]]
                    };
                    let left = (rect.x - 0.5).ceil().max(0.0) as usize;
                    let right = (rect.z - 0.5).ceil().clamp(0.0, 1920.0) as usize;
                    let top = (rect.y - 0.5).ceil().max(0.0) as usize;
                    let bottom = (rect.w - 0.5).ceil().clamp(0.0, 1080.0) as usize;
                    let vertex_alpha = (alpha * 255.0).round() / 255.0;
                    for y in top..bottom {
                        for x in left..right {
                            let u = uv.x
                                + (uv.z - uv.x) * (x as f32 + 0.5 - rect.x) / (rect.z - rect.x);
                            let v = uv.y
                                + (uv.w - uv.y) * (y as f32 + 0.5 - rect.y) / (rect.w - rect.y);
                            let sx = (u * image.width() as f32 - 0.5)
                                .clamp(0.0, image.width() as f32 - 1.0);
                            let sy = (v * image.height() as f32 - 0.5)
                                .clamp(0.0, image.height() as f32 - 1.0);
                            let ix = sx.floor() as u32;
                            let iy = sy.floor() as u32;
                            let dx = sx.fract();
                            let dy = sy.fract();
                            let a = image.get_pixel(ix, iy).0;
                            let b = image.get_pixel((ix + 1).min(image.width() - 1), iy).0;
                            let c = image.get_pixel(ix, (iy + 1).min(image.height() - 1)).0;
                            let d = image
                                .get_pixel(
                                    (ix + 1).min(image.width() - 1),
                                    (iy + 1).min(image.height() - 1),
                                )
                                .0;
                            let color = std::array::from_fn::<f32, 4, _>(|i| {
                                ((a[i] as f32 * (1.0 - dx) + b[i] as f32 * dx) * (1.0 - dy)
                                    + (c[i] as f32 * (1.0 - dx) + d[i] as f32 * dx) * dy)
                                    / 255.0
                            });
                            let alpha = color[3] * vertex_alpha;
                            let dst = &mut expected[y * 1920 + x];
                            for i in 0..3 {
                                dst[i] = color[i] * alpha
                                    + dst[i] * if additive { 1.0 } else { 1.0 - alpha };
                            }
                            dst[3] = alpha * (1.0 - dst[3]) + dst[3];
                            for component in dst {
                                *component = (*component * 255.0).round().clamp(0.0, 255.0) / 255.0;
                            }
                        }
                    }
                };
            draw(
                "bg",
                glam::vec4(0.0, 0.0, 1920.0, 1080.0),
                glam::vec4(0.0, 0.0, 1.0, 1.0),
                progress,
                false,
            );
            let top = 160.0 * progress - 160.0;
            draw(
                "obi",
                glam::vec4(0.0, top, 1920.0, top + 160.0),
                glam::vec4(0.0, 0.0, 1.0, 0.5),
                0.25,
                true,
            );
            let bottom = 1080.0 - 160.0 * progress;
            draw(
                "obi",
                glam::vec4(0.0, bottom, 1920.0, bottom + 160.0),
                glam::vec4(0.0, 0.5, 1.0, 1.0),
                0.25,
                true,
            );
            let shift = 1920.0 * (1.0 - progress);
            draw(
                "full",
                glam::vec4(1380.0 + shift, 100.0, 2082.0 + shift, 1072.0),
                glam::vec4(0.0, 0.0, 1.0, 1.0),
                1.0,
                false,
            );
            draw(
                "caption",
                glam::vec4(480.0, top, 1440.0, top + 128.0),
                glam::vec4(0.0, 0.0, 1.0, 1.0),
                1.0,
                false,
            );
            draw(
                "close",
                glam::vec4(1764.0, top + 32.0, 1848.0, top + 116.0),
                glam::vec4(0.0, 0.0, 1.0, 1.0),
                1.0,
                false,
            );
            let mut worst = 0.0f32;
            for (index, (actual, expected)) in actual.chunks_exact(4).zip(&expected).enumerate() {
                for channel in 0..4 {
                    let error = (actual[channel] as f32 - expected[channel] * 255.0).abs();
                    worst = worst.max(error);
                    assert!(
                        error <= 2.1,
                        "Character chrome{progress}, pixel{},{} channel{channel}: {:?} vs {:?}",
                        index % 1920,
                        index / 1920,
                        actual,
                        expected
                    );
                }
            }
            eprintln!(
                "Character chrome{progress}: worst error{worst} using{full_path}/{full_texture_name}"
            );
            if progress == 1.0 {
                let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
                for pixel in actual.chunks_exact(4) {
                    ppm.extend_from_slice(&pixel[..3]);
                }
                std::fs::write("/tmp/shin-character-chrome-reference.ppm", ppm).unwrap();
            }
        }
    }

    pub(crate) fn assert_reference_character_grid(
        context: &mut crate::render::PreRenderContext,
        server: &std::sync::Arc<crate::asset::system::AssetServer>,
        scenario: &shin_core::format::scenario::Scenario,
    ) {
        use crate::adv::{character_view::CharacterTextures, characters::CharacterModel};
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
        use shin_render::{
            DepthStencilState, StencilFunction, StencilMask, StencilPipelineState, StencilState,
        };
        use std::{fs::File, io::Read};
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let entry = rom.find_file("/chars.txa").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let original = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        let atlas: std::sync::Arc<CharacterTextures> = server.load_sync("/chars.txa").unwrap();
        let output = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Native character grid"),
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
        let view = output.create_view(&Default::default());
        // Independent native rectangle/UV rasterizer; quantize each draw like the UNORM target.
        let sample = |name: &str, u: f32, v: f32| {
            let image = &original.textures[original.name_to_index[name]];
            let sx = (u * image.width() as f32 - 0.5).clamp(0.0, image.width() as f32 - 1.0);
            let sy = (v * image.height() as f32 - 0.5).clamp(0.0, image.height() as f32 - 1.0);
            let x = sx.floor() as u32;
            let y = sy.floor() as u32;
            let dx = sx.fract();
            let dy = sy.fract();
            let a = image.get_pixel(x, y).0;
            let b = image.get_pixel((x + 1).min(image.width() - 1), y).0;
            let c = image.get_pixel(x, (y + 1).min(image.height() - 1)).0;
            let d = image
                .get_pixel(
                    (x + 1).min(image.width() - 1),
                    (y + 1).min(image.height() - 1),
                )
                .0;
            std::array::from_fn::<f32, 4, _>(|i| {
                ((a[i] as f32 * (1.0 - dx) + b[i] as f32 * dx) * (1.0 - dy)
                    + (c[i] as f32 * (1.0 - dx) + d[i] as f32 * dx) * dy)
                    / 255.0
            })
        };
        for (id, page, displacement, animated) in [
            (0, 0, 0.0f32, false),
            (0, 1, 37.25, false),
            (169, 0, 0.0, false),
            (0, 0, 0.0, true),
            (0, 1, 37.25, true),
        ] {
            let tables = scenario.info_tables();
            let model = CharacterModel::new(
                &tables.chars_grid_info[id],
                &tables.chars_sprite_info,
                id,
                page,
            );
            let scroll = model.page_scroll() + displacement;
            let hovered = model.selected_index();
            {
                let mut pass = context.begin_pass(
                    TextureTarget {
                        view: &view,
                        kind: TextureTargetKind::Screen,
                    },
                    Some(context.depth_stencil),
                    "Native character board",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::from_rgba(
                        32, 64, 96, 255,
                    )),
                    Some(128),
                    None,
                );
                atlas.render_window(&mut pass, crate::render::top_left_projection_matrix());
                if animated {
                    let selected: Vec<_> = model
                        .cells
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            if model.selection_on_page(c.page) == Some(i) {
                                0.4
                            } else {
                                0.0
                            }
                        })
                        .collect();
                    let clicked: Vec<_> = model
                        .cells
                        .iter()
                        .enumerate()
                        .map(|(i, _)| if hovered == Some(i) { 0.35 } else { 0.0 })
                        .collect();
                    atlas.render_grid_with_effects(
                        &mut pass,
                        &model,
                        scroll,
                        [161.0 / 255.0, 23.0 / 255.0, 0.0],
                        &selected,
                        &clicked,
                        crate::render::top_left_projection_matrix(),
                    );
                } else {
                    atlas.render_grid(
                        &mut pass,
                        &model,
                        scroll,
                        hovered,
                        crate::render::top_left_projection_matrix(),
                    );
                }

                atlas.render_controls(
                    &mut pass,
                    &model,
                    [0.0; 5],
                    crate::render::top_left_projection_matrix(),
                );
                atlas.render_footer(
                    &mut pass,
                    &model,
                    page == 0,
                    if animated { 0.5 } else { 1.0 },
                    crate::render::top_left_projection_matrix(),
                );
            }
            let pixels = super::movie_layer::readback_test_texture(context, &output);
            let mut expected = vec![[32.0 / 255.0, 64.0 / 255.0, 96.0 / 255.0, 1.0]; 1920 * 1080];
            let mut draw = |name: &str,
                            r: glam::Vec4,
                            uv: glam::Vec4,
                            tint: [f32; 4],
                            opaque: bool,
                            clipped: bool,
                            portrait_mode: Option<u8>| {
                let right =
                    r.z.ceil()
                        .min(if clipped { 490.0 } else { 1920.0 })
                        .max(0.0) as usize;
                let bottom =
                    r.w.ceil()
                        .min(if clipped { 724.0 } else { 1080.0 })
                        .max(0.0) as usize;
                // Pixel centers, including fractional page scrolling.
                let left = (r.x - 0.5)
                    .ceil()
                    .max(if clipped { 90.0 } else { 0.0 })
                    .max(0.0) as usize;
                let top = (r.y - 0.5)
                    .ceil()
                    .max(if clipped { 198.0 } else { 0.0 })
                    .max(0.0) as usize;
                let right = (r.z - 0.5).ceil().min(right as f32).max(0.0) as usize;
                let bottom = (r.w - 0.5).ceil().min(bottom as f32).max(0.0) as usize;
                for y in top..bottom {
                    for x in left..right {
                        let u = uv.x + (uv.z - uv.x) * (x as f32 + 0.5 - r.x) / (r.z - r.x);
                        let v = uv.y + (uv.w - uv.y) * (y as f32 + 0.5 - r.y) / (r.w - r.y);
                        let mut color = if name.is_empty() {
                            [1.0; 4]
                        } else {
                            sample(name, u, v)
                        };
                        if let Some(mode) = portrait_mode {
                            let base = sample("non", u, v);
                            let alpha = color[3];
                            if mode == 1 || (mode == 3 && v > 1.0 - u) {
                                color[1] *= 0.1;
                                color[2] *= 0.1;
                            }
                            if mode == 2 {
                                let luma = color[0] * f32::from_bits(0x3e991687)
                                    + color[1] * 0.587
                                    + color[2] * 0.114;
                                color[..3].fill(luma);
                            }
                            for i in 0..3 {
                                color[i] = color[i] * alpha + base[i] * (1.0 - alpha);
                            }
                            color[3] = 1.0;
                        }
                        for i in 0..4 {
                            color[i] *= tint[i];
                        }
                        let dst = &mut expected[y * 1920 + x];
                        for i in 0..3 {
                            dst[i] = if opaque {
                                color[i]
                            } else {
                                color[i] * color[3] + dst[i] * (1.0 - color[3])
                            };
                        }
                        dst[3] = if opaque {
                            color[3]
                        } else {
                            color[3] * (1.0 - dst[3]) + dst[3]
                        };
                        for v in dst {
                            *v = (*v * 255.0).round().clamp(0.0, 255.0) / 255.0;
                        }
                    }
                }
            };
            let xs = [0.0, 80.0, 1526.0, 1606.0];
            let ys = [0.0, 80.0, 810.0, 898.0];
            let us = [0.0, 80.0 / 1602.0, 1526.0 / 1602.0, 1606.0 / 1602.0];
            for row in 0..3 {
                for col in 0..3 {
                    draw(
                        "window",
                        glam::vec4(xs[col], ys[row] + 112.0, xs[col + 1], ys[row + 1] + 112.0),
                        glam::vec4(us[col], ys[row] / 898.0, us[col + 1], ys[row + 1] / 898.0),
                        [1.0; 4],
                        row == 1 && col == 1,
                        false,
                        None,
                    );
                }
            }
            draw(
                "separator",
                glam::vec4(510.0, 203.0, 528.0, 903.0),
                glam::vec4(0.0, 0.0, 1.0, 1.0),
                [1.0; 4],
                false,
                false,
                None,
            );
            draw(
                "",
                glam::vec4(90.0, 198.0, 490.0, 724.0),
                glam::vec4(0.0, 0.0, 1.0, 1.0),
                [0.0, 0.0, 0.0, 191.0 / 255.0],
                false,
                false,
                None,
            );
            let colors = if animated {
                [161.0, 23.0, 0.0]
            } else {
                [
                    [166.0, 0.0, 0.0],
                    [146.0, 92.0, 0.0],
                    [135.0, 135.0, 135.0],
                    [0.0, 0.0, 0.0],
                ][model.page as usize]
            };
            draw(
                "hana",
                glam::vec4(90.0, 198.0, 470.0, 638.0),
                glam::vec4(1.0, 1.0, 0.0, 0.0),
                [
                    colors[0] / 255.0,
                    colors[1] / 255.0,
                    colors[2] / 255.0,
                    128.0 / 255.0,
                ],
                false,
                false,
                None,
            );
            let slot =
                |page: u8| (model.page_mask & ((1u8 << page) - 1)).count_ones() as f32 * 400.0;
            for connector in &model.connectors {
                let x = 110.0 + slot(connector.page) - scroll + connector.x as f32 * 24.0;
                let y = 214.0 + connector.y as f32 * 26.0;
                let u = (4.0 + (connector.shape & 3) as f32 * 32.0) / 128.0;
                let v = (5.0 + (connector.shape >> 2) as f32 * 36.0) / 144.0;
                let c = [
                    [0.0; 3],
                    [192.0, 20.0, 60.0],
                    [60.0, 22.0, 224.0],
                    [240.0, 208.0, 20.0],
                ][connector.color as usize];
                draw(
                    "line",
                    glam::vec4(x, y, x + 24.0, y + 26.0),
                    glam::vec4(u, v, u + 24.0 / 128.0, v + 26.0 / 144.0),
                    [c[0] / 255.0, c[1] / 255.0, c[2] / 255.0, 1.0],
                    false,
                    true,
                    None,
                );
            }
            for (index, cell) in model.cells.iter().enumerate() {
                let x = 84.0 + slot(cell.page) - scroll + cell.x as f32 * 24.0;
                let y = 184.0 + cell.y as f32 * 26.0;
                let r = glam::vec4(x, y, x + 76.0, y + 86.0);
                let v = cell
                    .is_selectable()
                    .then(|| cell.definition().state().and_then(|s| s.variant()))
                    .flatten();
                let (name, mode) = v
                    .map(|v| (v.portrait.as_str(), v.display_mode))
                    .unwrap_or(("null", 0));
                draw(
                    name,
                    r,
                    glam::vec4(0.0, 0.0, 1.0, 1.0),
                    [1.0; 4],
                    false,
                    true,
                    Some(mode),
                );
                if cell.definitions.len() > 1 {
                    draw(
                        "numbase",
                        glam::vec4(x + 40.0, y + 66.0, x + 80.0, y + 86.0),
                        glam::vec4(0.0, 0.0, 1.0, 1.0),
                        [1.0; 4],
                        false,
                        true,
                        None,
                    );
                    let text =
                        format!("{}/{}", cell.definition_index() + 1, cell.definitions.len());
                    let mut pos = x + 72.0 - text.len() as f32 * 10.0 - 3.0;
                    for byte in text.bytes() {
                        let c = if byte == b'/' { 10 } else { byte - b'0' };
                        let u = c as f32 * 16.0 / 176.0;
                        draw(
                            "num",
                            glam::vec4(pos, y + 68.0, pos + 16.0, y + 84.0),
                            glam::vec4(u, 0.0, u + 16.0 / 176.0, 1.0),
                            [1.0; 4],
                            false,
                            true,
                            None,
                        );
                        pos += 10.0;
                    }
                }
                if model.selection_on_page(cell.page) == Some(index) {
                    draw(
                        "cur",
                        r,
                        glam::vec4(0.0, 0.0, 1.0, 1.0),
                        [
                            1.0,
                            1.0,
                            1.0,
                            if animated {
                                51.0 / 255.0
                            } else {
                                128.0 / 255.0
                            },
                        ],
                        false,
                        true,
                        None,
                    );
                }
                if hovered == Some(index) {
                    draw(
                        "cur",
                        r,
                        glam::vec4(0.0, 0.0, 1.0, 1.0),
                        [1.0, 1.0, 1.0, if animated { 89.0 / 255.0 } else { 1.0 }],
                        false,
                        true,
                        None,
                    );
                }
            }
            // Independently entered native button atlas rectangles; model flags are checked
            // by portable endpoint tests and executable fixtures.
            let flags = model.button_states();
            let controls = [
                (
                    "lr",
                    glam::vec4(90.0, 724.0, 274.0, 780.0),
                    glam::vec4(0.0, 0.0, 0.5, 1.0),
                ),
                (
                    "lr",
                    glam::vec4(306.0, 724.0, 490.0, 780.0),
                    glam::vec4(0.5, 0.0, 1.0, 1.0),
                ),
                (
                    "button",
                    glam::vec4(102.0, 780.0, 350.0, 844.0),
                    glam::vec4(0.0, 0.0, 2.0 / 3.0, 0.5),
                ),
                (
                    "button",
                    glam::vec4(102.0, 844.0, 350.0, 908.0),
                    glam::vec4(0.0, 0.5, 2.0 / 3.0, 1.0),
                ),
                (
                    "button",
                    glam::vec4(354.0, 780.0, 478.0, 908.0),
                    glam::vec4(2.0 / 3.0, 0.0, 1.0, 1.0),
                ),
            ];
            for (index, (name, r, uv)) in controls.into_iter().enumerate() {
                use crate::adv::characters::CharacterButtonState;
                if flags[index] == CharacterButtonState::Hidden {
                    continue;
                }
                let gray = if flags[index] == CharacterButtonState::Disabled {
                    128.0 / 255.0
                } else {
                    1.0
                };
                draw(name, r, uv, [gray, gray, gray, 1.0], false, false, None);
            }
            // Independent native footer table, layout, enable rules and icon UVs.
            let cell = model.selected_cell();
            let def = cell.map(|c| c.definition());
            let footer = [
                (1usize, true, true),
                (3, true, page == 0),
                (0, true, true),
                (6, model.page_mask.count_ones() >= 2, true),
                (2, true, cell.is_some_and(|c| c.definitions.len() > 1)),
                (5, id < 8, def.is_some_and(|d| d.states.len() > 1)),
                (
                    4,
                    id < 8,
                    def.and_then(|d| d.state())
                        .is_some_and(|s| s.variants.len() > 1),
                ),
            ];
            let widths = [206.0, 72.0, 272.0, 176.0, 140.0, 140.0, 238.0];
            let icons = [0, 4, 3, 9, 5, 6, 7];
            let mut right = 1872.0;
            let y = if animated { 1056.0 } else { 1012.0 };
            for (row, visible, enabled) in footer {
                if !visible {
                    continue;
                }
                let left = right - widths[row];
                let g = if enabled { 1.0 } else { 128.0 / 255.0 };
                let tint = [g, g, g, 1.0];
                draw(
                    "help_chars",
                    glam::vec4(left, y, right, y + 48.0),
                    glam::vec4(
                        0.0,
                        row as f32 / 7.0,
                        widths[row] / 320.0,
                        (row + 1) as f32 / 7.0,
                    ),
                    tint,
                    false,
                    false,
                    None,
                );
                let icon = icons[row];
                let x = left - 46.0;
                let u = (icon % 8) as f32 / 8.0;
                let v = (icon / 8) as f32 / 2.0;
                draw(
                    "key",
                    glam::vec4(x, y, x + 48.0, y + 48.0),
                    glam::vec4(u, v, u + 0.125, v + 0.5),
                    tint,
                    false,
                    false,
                    None,
                );
                right = x - 16.0;
            }
            let mut worst = 0.0f32;
            for (index, (actual, expected)) in pixels.chunks_exact(4).zip(expected).enumerate() {
                for channel in 0..4 {
                    let diff = (actual[channel] as f32 - expected[channel] * 255.0).abs();
                    worst = worst.max(diff);
                    assert!(
                        diff <= 2.1,
                        "Character grid{id} page{page} scroll{scroll} pixel({},{}) channel{channel}: {actual:?}, expected{expected:?}",
                        index % 1920,
                        index / 1920
                    );
                }
            }
            eprintln!(
                "Character grid{id} page{page} animated{animated}: worst channel error{worst}"
            );
            if id == 0 && page == 0 {
                let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
                for pixel in pixels.chunks_exact(4) {
                    ppm.extend_from_slice(&pixel[..3]);
                }
                std::fs::write("/tmp/shin-character-grid-reference.ppm", ppm).unwrap();
            }
        }
        {
            let tables = scenario.info_tables();
            let model =
                CharacterModel::new(&tables.chars_grid_info[0], &tables.chars_sprite_info, 0, 0);
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                Some(context.depth_stencil),
                "Character grid retained stencil",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(128),
                None,
            );
            atlas.render_grid(
                &mut pass,
                &model,
                0.0,
                None,
                crate::render::top_left_projection_matrix(),
            );
            shin_render::quad_vertices::QuadVertices::new()
                .with_box(0.0, 0.0, 1920.0, 1080.0)
                .with_color(shin_core::primitives::color::UnormColor::from_rgba(
                    11, 22, 33, 255,
                ))
                .fill(
                    &mut pass,
                    RenderRequestBuilder::new()
                        .color_blend_type(ColorBlendType::Opaque)
                        .depth_stencil(DepthStencilState {
                            stencil: StencilState {
                                stencil_reference: 128,
                                pipeline: StencilPipelineState {
                                    function: StencilFunction::Equal,
                                    stencil_read_mask: StencilMask::SignOnly,
                                    ..Default::default()
                                },
                            },
                            ..Default::default()
                        }),
                    crate::render::top_left_projection_matrix(),
                );
        }
        let pixels = super::movie_layer::readback_test_texture(context, &output);
        assert!(
            pixels.chunks_exact(4).all(|p| p == [11, 22, 33, 255]),
            "Grid clip destroyed the existing mask stencil bit"
        );
    }
}
