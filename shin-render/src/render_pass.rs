use glam::{vec3, vec4};
use shin_primitives::color::{FloatColor4, UnormColor};
use shin_render_shader_types::{
    buffer::VertexSource,
    texture::{DepthStencilTarget, TextureSamplerStore, TextureTarget, TextureTargetKind},
    uniforms::{
        BlendUniformParams, BlurUniformParams, ButtonUniformParams, CharacterPortraitUniformParams,
        ClearUniformParams, FillUniformParams, FontBorderUniformParams, FontSdfUniformParams,
        FontUniformParams, HigurashiBlurUniformParams, HigurashiRasterUniformParams,
        LayerUniformParams, MaskUniformParams, MosaicUniformParams, MovieUniformParams,
        RasterUniformParams, RippleUniformParams, SpriteUniformParams, TapRippleUniformParams,
        WiperDefaultUniformParams, WiperGlassUniformParams, WiperMaskUniformParams,
        WiperRippleUniformParams, WiperWaveUniformParams, WiperWhirlUniformParams,
        ZoomBlurUniformParams,
    },
    vertices::PosVertex,
};
use shin_render_shaders::{
    Blend, BlendBindings, Blur, BlurBindings, Button, ButtonBindings, CharacterPortrait,
    CharacterPortraitBindings, Clear, ClearBindings, Dissolve, DissolveBindings, Fill,
    FillBindings, Font, FontBindings, FontBorder, FontBorderBindings, FontSdf, FontSdfBindings,
    Gerokasu2Blur, Gerokasu2BlurBindings, HigurashiBlur, HigurashiBlurBindings, HigurashiOverlay,
    HigurashiOverlayBindings, HigurashiPostprocess, HigurashiPostprocessBindings, HigurashiRaster,
    HigurashiRasterBindings, Layer, LayerBindings, Mask, MaskBindings, Mosaic, MosaicBindings,
    Movie, MovieBindings, Raster, RasterBindings, Ripple, RippleBindings, ScreenCurtain,
    ScreenCurtainBindings, Shader, Sprite, SpriteBindings, TapRipple, TapRippleBindings, Window,
    WindowBindings, WiperDefault, WiperDefaultBindings, WiperGlass, WiperGlassBindings, WiperMask,
    WiperMaskBindings, WiperRipple, WiperRippleBindings, WiperWave, WiperWaveBindings, WiperWhirl,
    WiperWhirlBindings, ZoomBlur, ZoomBlurBindings,
};

use crate::{
    ColorBlendType, CullFace, DepthFunction, DepthState, DepthStencilState, DrawPrimitive,
    RenderProgramWithArguments, RenderRequest, RenderRequestBuilder, StencilFunction,
    StencilOperation, StencilPipelineState, StencilState,
    dynamic_buffer::DynamicBuffer,
    pipelines::{PipelineStorage, PipelineStorageKey},
};

pub struct RenderPass<'pipelines, 'dynbuffer, 'sampler, 'device, 'encoder> {
    pipeline_storage: &'pipelines mut PipelineStorage,
    dynamic_buffer: &'dynbuffer mut DynamicBuffer,
    sampler_store: &'sampler TextureSamplerStore,
    target_kind: TextureTargetKind,
    has_depth_stencil: bool,
    device: &'device wgpu::Device,
    pass: wgpu::RenderPass<'encoder>,
}

impl<'pipelines, 'dynbuffer, 'sampler, 'device, 'encoder>
    RenderPass<'pipelines, 'dynbuffer, 'sampler, 'device, 'encoder>
{
    pub fn new(
        pipeline_storage: &'pipelines mut PipelineStorage,
        dynamic_buffer: &'dynbuffer mut DynamicBuffer,
        sampler_store: &'sampler TextureSamplerStore,
        device: &'device wgpu::Device,
        encoder: &'encoder mut wgpu::CommandEncoder,
        target_color: TextureTarget,
        target_depth_stencil: Option<DepthStencilTarget>,
        viewport: Option<(f32, f32, f32, f32)>,
        label: &str,
    ) -> Self {
        Self::new_with_depth_preservation(
            pipeline_storage,
            dynamic_buffer,
            sampler_store,
            device,
            encoder,
            target_color,
            target_depth_stencil,
            viewport,
            label,
            false,
        )
    }

    /// A native target restore preserves its existing depth/stencil contents.
    /// Ordinary callers retain the original constructor's clearing behavior.
    pub fn new_with_depth_preservation(
        pipeline_storage: &'pipelines mut PipelineStorage,
        dynamic_buffer: &'dynbuffer mut DynamicBuffer,
        sampler_store: &'sampler TextureSamplerStore,
        device: &'device wgpu::Device,
        encoder: &'encoder mut wgpu::CommandEncoder,
        target_color: TextureTarget,
        target_depth_stencil: Option<DepthStencilTarget>,
        viewport: Option<(f32, f32, f32, f32)>,
        label: &str,
        preserve_depth_stencil: bool,
    ) -> Self {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            multiview_mask: None,
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                depth_slice: None,
                view: target_color.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    // NOTE: using `Load` here can have some perf implications
                    // however, this is needed for ghosting effect to work (maybe there's something else)
                    // TODO: add a parameter determining whether to clear the texture or not
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: target_depth_stencil.map(|target_depth_stencil| {
                wgpu::RenderPassDepthStencilAttachment {
                    view: target_depth_stencil.view,
                    depth_ops: Some(wgpu::Operations {
                        // NOTE: potential incompatibility, shin _might_ not perform a clear when changing a render target
                        load: if preserve_depth_stencil {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(0.0)
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        // NOTE: potential incompatibility, shin _might_ not perform a clear when changing a render target
                        load: if preserve_depth_stencil {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(0)
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                }
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        if let Some((x, y, width, height)) = viewport {
            pass.set_viewport(x, y, width, height, 0.0, 1.0);
        }

        Self {
            pipeline_storage,
            dynamic_buffer,
            sampler_store,
            target_kind: target_color.kind,
            has_depth_stencil: target_depth_stencil.is_some(),
            device,
            pass,
        }
    }

    /// Set the native half-target viewport used by downsampling effects.
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.pass.set_viewport(0.0, 0.0, width, height, 0.0, 1.0);
    }

    pub fn push_debug(&mut self, label: &str) {
        self.pass.push_debug_group(label)
    }

    pub fn pop_debug(&mut self) {
        self.pass.pop_debug_group()
    }

    fn run_impl<S: Shader>(
        &mut self,
        key: PipelineStorageKey,
        bindings: S::Bindings<'_>,
        vertices: VertexSource<S::Vertex>,
    ) {
        self.pipeline_storage.get::<S>(key).render(
            self.device,
            self.dynamic_buffer,
            self.sampler_store,
            &mut self.pass,
            bindings,
            vertices,
        )
    }

    #[tracing::instrument(level = "trace", skip_all, fields(shader = ?request.program.get_shader_name()))]
    pub fn run(&mut self, request: RenderRequest) {
        let pass = &mut self.pass;

        let RenderRequest {
            depth_stencil,
            color_blend_type,
            cull_faces,
            primitive,
            program,
        } = request;

        let (depth_stencil, stencil_reference) = depth_stencil.into_pipeline_parts();

        let key = PipelineStorageKey {
            target_kind: self.target_kind,
            draw_primitive: primitive,
            cull_face: cull_faces,
            blend_type: color_blend_type,
            depth_stencil: self.has_depth_stencil.then_some(depth_stencil),
        };
        pass.set_stencil_reference(stencil_reference as u32);

        let higurashi_default = matches!(
            program,
            RenderProgramWithArguments::WiperHigurashiDefault { .. }
        );
        let higurashi_mask = matches!(
            program,
            RenderProgramWithArguments::WiperHigurashiMask { .. }
        );
        match program {
            RenderProgramWithArguments::Clear { vertices, color } => self.run_impl::<Clear>(
                key,
                ClearBindings {
                    params: &ClearUniformParams { color },
                },
                vertices,
            ),
            RenderProgramWithArguments::Fill {
                vertices,
                transform,
            } => self.run_impl::<Fill>(
                key,
                FillBindings {
                    params: &FillUniformParams { transform },
                },
                vertices,
            ),
            RenderProgramWithArguments::Sprite {
                vertices,
                sprite,
                transform,
            } => self.run_impl::<Sprite>(
                key,
                SpriteBindings {
                    params: &SpriteUniformParams { transform },
                    sprite,
                },
                vertices,
            ),
            RenderProgramWithArguments::Blend {
                vertices,
                texture1,
                texture2,
                transform,
                blend,
                flash,
            } => self.run_impl::<Blend>(
                key,
                BlendBindings {
                    params: &BlendUniformParams {
                        transform,
                        blend,
                        flash,
                    },
                    texture1,
                    texture2,
                },
                vertices,
            ),
            RenderProgramWithArguments::Window {
                vertices,
                border,
                mask,
                background,
                transform,
            } => self.run_impl::<Window>(
                key,
                WindowBindings {
                    params: &FillUniformParams { transform },
                    border,
                    mask,
                    background,
                },
                vertices,
            ),
            RenderProgramWithArguments::Font {
                vertices,
                glyph,
                transform,
                color1,
                color2,
            } => self.run_impl::<Font>(
                key,
                FontBindings {
                    params: &FontUniformParams {
                        transform,
                        color1,
                        color2,
                    },
                    glyph,
                },
                vertices,
            ),
            RenderProgramWithArguments::FontSdf {
                vertices,
                glyph,
                transform,
                color1,
                color2,
                thresholds,
            } => self.run_impl::<FontSdf>(
                key,
                FontSdfBindings {
                    params: &FontSdfUniformParams {
                        transform,
                        color1,
                        color2,
                        thresholds,
                    },
                    glyph,
                },
                vertices,
            ),
            RenderProgramWithArguments::FontBorder {
                vertices,
                glyph,
                transform,
                distances,
                color,
            } => self.run_impl::<FontBorder>(
                key,
                FontBorderBindings {
                    params: &FontBorderUniformParams {
                        transform,
                        // [Vec2; 8] -> [Vec4; 4]
                        dist: bytemuck::cast(distances),
                        color,
                    },
                    glyph,
                },
                vertices,
            ),

            RenderProgramWithArguments::Layer {
                output_kind,
                fragment_shader,
                vertices,
                texture,
                transform,
                color_multiplier,
                fragment_shader_param,
            } => self.run_impl::<Layer>(
                key,
                LayerBindings {
                    params: &LayerUniformParams {
                        transform,
                        color: color_multiplier,
                        fragment_param: fragment_shader_param,
                        output_type: output_kind as u32,
                        fragment_operation: fragment_shader as u32,
                    },
                    texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::HigurashiPostprocess {
                vertices,
                texture,
                transform,
                color,
                operation,
                param,
            } => self.run_impl::<HigurashiPostprocess>(
                key,
                HigurashiPostprocessBindings {
                    params: &LayerUniformParams {
                        transform,
                        color,
                        fragment_param: param,
                        output_type: 0,
                        fragment_operation: operation as u32,
                    },
                    source: texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::HigurashiOverlay {
                vertices,
                texture,
                mask,
                transform,
                minmax,
            } => self.run_impl::<HigurashiOverlay>(
                key,
                HigurashiOverlayBindings {
                    params: &WiperMaskUniformParams {
                        transform,
                        minmax: minmax.extend(0.0).extend(0.0),
                    },
                    texture,
                    mask,
                },
                vertices,
            ),
            RenderProgramWithArguments::Mask {
                fragment_shader,
                vertices,
                texture,
                mask,
                transform,
                color_multiplier,
                fragment_shader_param,
                minmax,
            } => self.run_impl::<Mask>(
                key,
                MaskBindings {
                    params: &MaskUniformParams {
                        transform,
                        color: color_multiplier,
                        fragment_param: fragment_shader_param,
                        minmax: minmax.extend(0.0).extend(0.0),
                        fragment_operation: fragment_shader as u32,
                    },
                    texture,
                    mask,
                },
                vertices,
            ),
            RenderProgramWithArguments::TapRipple {
                vertices,
                texture,
                transform,
                center_scale,
                wave_params,
            } => self.run_impl::<TapRipple>(
                key,
                TapRippleBindings {
                    params: &TapRippleUniformParams {
                        transform,
                        center_scale,
                        wave_params,
                    },
                    source: texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::Ripple {
                vertices,
                texture,
                transform,
                wave,
            } => self.run_impl::<Ripple>(
                key,
                RippleBindings {
                    params: &RippleUniformParams { transform, wave },
                    source: texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::HigurashiRaster {
                vertices,
                texture,
                transform,
                color,
                amplitude_frequency,
                phase,
            } => self.run_impl::<HigurashiRaster>(
                key,
                HigurashiRasterBindings {
                    params: &HigurashiRasterUniformParams {
                        transform,
                        color,
                        amplitude_frequency,
                        phase,
                    },
                    source: texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::Gerokasu2Blur {
                vertices,
                texture,
                transform,
                distances,
                weights,
            } => self.run_impl::<Gerokasu2Blur>(
                key,
                Gerokasu2BlurBindings {
                    params: &HigurashiBlurUniformParams {
                        transform,
                        distances_first: distances[0],
                        distances_last: distances[1],
                        color: glam::Vec4::ONE,
                        weights,
                    },
                    source: texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::HigurashiBlur {
                vertices,
                texture,
                transform,
                distances,
                color,
                weights,
            } => self.run_impl::<HigurashiBlur>(
                key,
                HigurashiBlurBindings {
                    params: &HigurashiBlurUniformParams {
                        transform,
                        distances_first: distances[0],
                        distances_last: distances[1],
                        color,
                        weights,
                    },
                    source: texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::Blur {
                vertices,
                texture,
                transform,
                step,
            } => self.run_impl::<Blur>(
                key,
                BlurBindings {
                    params: &BlurUniformParams { transform, step },
                    source: texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::ZoomBlur {
                vertices,
                texture,
                transform,
                center,
                scales,
            } => self.run_impl::<ZoomBlur>(
                key,
                ZoomBlurBindings {
                    params: &ZoomBlurUniformParams {
                        transform,
                        center,
                        scales_first: scales[0],
                        scales_last: scales[1],
                    },
                    source: texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::Raster {
                vertices,
                texture,
                transform,
                amplitude_frequency,
                phase,
            } => self.run_impl::<Raster>(
                key,
                RasterBindings {
                    params: &RasterUniformParams {
                        transform,
                        amplitude_frequency,
                        phase,
                    },
                    source: texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::Mosaic {
                vertices,
                texture,
                transform,
                step,
            } => self.run_impl::<Mosaic>(
                key,
                MosaicBindings {
                    params: &MosaicUniformParams { transform, step },
                    source: texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::Dissolve {
                vertices,
                texture,
                breakup,
                transform,
            } => self.run_impl::<Dissolve>(
                key,
                DissolveBindings {
                    params: &SpriteUniformParams { transform },
                    source: texture,
                    breakup,
                },
                vertices,
            ),
            RenderProgramWithArguments::Button {
                vertices,
                texture,
                transform,
                flash1,
                flash2,
            } => self.run_impl::<Button>(
                key,
                ButtonBindings {
                    params: &ButtonUniformParams {
                        transform,
                        flash1,
                        flash2,
                    },
                    button: texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::CharacterPortrait {
                vertices,
                portrait,
                frame,
                transform,
                mode,
            } => self.run_impl::<CharacterPortrait>(
                key,
                CharacterPortraitBindings {
                    params: &CharacterPortraitUniformParams {
                        transform,
                        effect: vec4(f32::from(mode), 0.0, 0.0, 0.0),
                    },
                    portrait: portrait,
                    frame: frame,
                },
                vertices,
            ),
            RenderProgramWithArguments::Movie {
                vertices,
                texture_luma,
                texture_chroma,
                transform,
                color_bias,
                color_transform,
                color_multiplier,
                alpha_mode,
                texture_size,
            } => self.run_impl::<Movie>(
                key,
                MovieBindings {
                    params: &MovieUniformParams {
                        transform,
                        color_bias,
                        color_transform,
                        color_multiplier,
                        alpha_mode: u32::from(alpha_mode),
                        texture_size,
                    },
                    luma: texture_luma,
                    chroma: texture_chroma,
                },
                vertices,
            ),

            RenderProgramWithArguments::WiperGlass {
                vertices,
                texture,
                transform,
                model,
                effect,
            } => self.run_impl::<WiperGlass>(
                key,
                WiperGlassBindings {
                    params: &WiperGlassUniformParams {
                        transform,
                        model,
                        effect,
                    },
                    source: texture,
                },
                vertices,
            ),
            RenderProgramWithArguments::WiperRipple {
                vertices,
                texture_source,
                texture_target,
                transform,
                center_scale,
                wave_params,
            } => self.run_impl::<WiperRipple>(
                key,
                WiperRippleBindings {
                    params: &WiperRippleUniformParams {
                        transform,
                        center_scale,
                        wave_params,
                    },
                    source: texture_source,
                    target: texture_target,
                },
                vertices,
            ),
            RenderProgramWithArguments::WiperWave {
                vertices,
                texture_source,
                texture_target,
                transform,
                center_scale,
                front_width,
            } => self.run_impl::<WiperWave>(
                key,
                WiperWaveBindings {
                    params: &WiperWaveUniformParams {
                        transform,
                        center_scale,
                        front_width,
                    },
                    source: texture_source,
                    target: texture_target,
                },
                vertices,
            ),
            RenderProgramWithArguments::WiperWhirl {
                vertices,
                texture_source,
                texture_target,
                transform,
                effect,
            } => self.run_impl::<WiperWhirl>(
                key,
                WiperWhirlBindings {
                    params: &WiperWhirlUniformParams { transform, effect },
                    source: texture_source,
                    target: texture_target,
                },
                vertices,
            ),
            RenderProgramWithArguments::WiperDefault {
                vertices,
                texture_source,
                texture_target,
                transform,
                alpha,
            }
            | RenderProgramWithArguments::WiperHigurashiDefault {
                vertices,
                texture_source,
                texture_target,
                transform,
                alpha,
            } => self.run_impl::<WiperDefault>(
                key,
                WiperDefaultBindings {
                    params: &WiperDefaultUniformParams {
                        transform,
                        alpha: vec4(alpha, if higurashi_default { 1.0 } else { 0.0 }, 0.0, 0.0),
                    },
                    source: texture_source,
                    target: texture_target,
                },
                vertices,
            ),

            RenderProgramWithArguments::WiperMask {
                vertices,
                texture_source,
                texture_target,
                texture_mask,
                transform,
                minmax,
            }
            | RenderProgramWithArguments::WiperHigurashiMask {
                vertices,
                texture_source,
                texture_target,
                texture_mask,
                transform,
                minmax,
            } => self.run_impl::<WiperMask>(
                key,
                WiperMaskBindings {
                    params: &WiperMaskUniformParams {
                        transform,
                        minmax: minmax
                            .extend(if higurashi_mask { 1.0 } else { 0.0 })
                            .extend(0.0),
                    },
                    source: texture_source,
                    target: texture_target,
                    mask: texture_mask,
                },
                vertices,
            ),
            RenderProgramWithArguments::ScreenCurtain {
                vertices,
                texture,
                mask,
                transform,
                minmax,
            } => self.run_impl::<ScreenCurtain>(
                key,
                ScreenCurtainBindings {
                    params: &WiperMaskUniformParams {
                        transform,
                        minmax: minmax.extend(0.0).extend(0.0),
                    },
                    texture,
                    mask,
                },
                vertices,
            ),
            _ => todo!(),
        }
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub fn clear(&mut self, color: Option<UnormColor>, stencil: Option<u8>, depth: Option<f32>) {
        // Clear's shader writes clip coordinates directly. wgpu accepts
        // depth in 0..1; converting zero to OpenGL's -1 clips the triangle
        // away and leaves depth masks unable to clip scrolling content.
        let z = depth.unwrap_or(1.0);
        // draw a single full-screen triangle
        // https://stackoverflow.com/questions/2588875/whats-the-best-way-to-draw-a-fullscreen-quad-in-opengl-3-2
        let vertices = &[
            PosVertex {
                position: vec3(-1.0, -1.0, z),
            },
            PosVertex {
                position: vec3(3.0, -1.0, z),
            },
            PosVertex {
                position: vec3(-1.0, 3.0, z),
            },
        ];
        let color_param = match color {
            Some(color) => FloatColor4::from_unorm(color),
            None => FloatColor4::BLACK,
        };
        let stencil_param = stencil.unwrap_or(1);
        let stencil_operation = if stencil.is_some() {
            StencilOperation::Replace
        } else {
            StencilOperation::Keep
        };

        self.push_debug(&format!("clear[{:?}, {:?}, {:?}]", color, stencil, depth));
        self.run(
            RenderRequestBuilder::new()
                .color_blend_type(if color.is_some() {
                    ColorBlendType::Opaque
                } else {
                    ColorBlendType::NoColor
                })
                .depth_stencil(DepthStencilState {
                    depth: DepthState {
                        function: DepthFunction::Always,
                        write_enable: depth.is_some(),
                    },
                    stencil: StencilState {
                        pipeline: StencilPipelineState {
                            function: StencilFunction::Always,
                            stencil_fail_operation: stencil_operation,
                            depth_fail_operation: stencil_operation,
                            pass_operation: stencil_operation,
                            ..Default::default()
                        },
                        stencil_reference: stencil_param,
                    },
                })
                .cull_faces(CullFace::None)
                .build(
                    RenderProgramWithArguments::Clear {
                        vertices: VertexSource::VertexData { vertices },
                        color: color_param,
                    },
                    DrawPrimitive::Triangles,
                ),
        );
        self.pop_debug();
    }
}
