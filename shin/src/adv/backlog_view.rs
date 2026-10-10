//! MessageBrowser shell. Text, scrollbar, help and replay controls are separate.
use glam::{Vec2, Vec4, vec4};
use shin_core::{
    primitives::color::FloatColor4,
    time::{Ticks, Tween, Tweener},
};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, gpu_texture::GpuTexture, quad_vertices::QuadVertices,
    render_pass::RenderPass,
};

use crate::{asset::texture_archive::TextureArchive, render::top_left_projection_matrix};

#[derive(TextureArchive)]
pub(super) struct BacklogTextures {
    #[txa(name = "bg")]
    pub bg: GpuTexture,
    #[txa(name = "obi")]
    pub obi: GpuTexture,
    #[txa(name = "caption")]
    pub caption: GpuTexture,
    #[txa(name = "close")]
    pub close: GpuTexture,
    #[txa(name = "window")]
    pub window: GpuTexture,
    #[txa(name = "chapter")]
    pub chapter: GpuTexture,
    #[txa(name = "focus")]
    pub focus: GpuTexture,
    #[txa(name = "help_logview")]
    pub help_logview: GpuTexture,
    #[txa(name = "key")]
    pub key: GpuTexture,
    #[txa(name = "line")]
    pub line: GpuTexture,
    #[txa(name = "voice")]
    pub voice: GpuTexture,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Phase {
    #[default]
    Opening,
    Open,
    CloseFlash,
    Closing,
    Closed,
}

#[derive(Debug)]
pub(super) struct BacklogShell {
    progress: f32,
    phase: Phase,
    close_flash: Tweener,
}

impl Default for BacklogShell {
    fn default() -> Self {
        Self {
            progress: 0.0,
            phase: Phase::Opening,
            close_flash: Tweener::new(0.0),
        }
    }
}

impl BacklogShell {
    pub fn progress(&self) -> f32 {
        self.progress
    }
    // Native acc40/ad0a0: begin after route collection is ready; do not carry
    // unused opening time into the next task or signal close before fade-out.
    pub fn update(&mut self, delta: Ticks) {
        match self.phase {
            Phase::Opening => {
                self.progress = (self.progress + delta.as_f32() * 0.1).min(1.0);
                if self.progress == 1.0 {
                    self.phase = Phase::Open;
                }
            }
            Phase::CloseFlash => {
                if self.close_flash.is_idle() {
                    self.phase = Phase::Closing;
                }
            }
            Phase::Closing => {
                self.progress = (self.progress - delta.as_f32() * 0.1).max(0.0);
                if self.progress == 0.0 {
                    self.phase = Phase::Closed;
                }
            }
            _ => {}
        }
        self.close_flash.update(delta);
    }

    pub fn hold_close(&mut self) {
        let current = self.close_flash.value();
        self.close_flash.fast_forward_to(current);
        self.close_flash.enqueue(
            0.7,
            Tween::linear(Ticks::from_f32((0.7 - current).abs() / 0.2)),
        );
    }
    pub fn cancel_close(&mut self) {
        let current = self.close_flash.value();
        self.close_flash.fast_forward_to(current);
        self.close_flash
            .enqueue(0.0, Tween::linear(Ticks::from_f32(current.abs() / 0.2)));
    }
    pub fn flash_close(&mut self) {
        self.hold_close();
        self.close_flash
            .enqueue(0.0, Tween::linear(Ticks::from_f32(3.5)));
    }
    pub fn close_feedback_finished(&self) -> bool {
        self.close_flash.is_idle()
    }
    pub fn request_close(&mut self) {
        if self.is_open() {
            self.flash_close();
            self.phase = Phase::CloseFlash;
        }
    }

    pub fn is_open(&self) -> bool {
        self.phase == Phase::Open
    }
    pub fn is_closed(&self) -> bool {
        self.phase == Phase::Closed
    }
    /// Restore failure leaves the current story intact and returns to its browser.
    pub fn reopen(&mut self) {
        self.progress = 1.0;
        self.phase = Phase::Open;
        self.close_flash.fast_forward_to(0.0);
    }
    pub fn begin_close(&mut self) {
        if self.phase != Phase::Closed {
            self.phase = Phase::Closing;
        }
    }

    /// Native a74f0 close target uses half-open canvas coordinates, independent
    /// of the caption's slide. Its task enables input only after opening.
    pub fn close_at(&self, position: Vec2) -> bool {
        self.is_open()
            && (1764.0..1848.0).contains(&position.x)
            && (32.0..116.0).contains(&position.y)
    }

    pub fn render_background(&self, textures: &BacklogTextures, pass: &mut RenderPass) {
        let p = self.progress;
        sprite(
            pass,
            &textures.bg,
            vec4(0.0, 0.0, 1920.0, 1080.0),
            Vec4::new(0.0, 0.0, 1.0, 1.0),
            p,
            ColorBlendType::Layer1,
        );
        let top = p * 160.0 - 160.0;
        let bottom = 1080.0 - p * 160.0;
        sprite(
            pass,
            &textures.obi,
            vec4(0.0, top, 1920.0, top + 160.0),
            vec4(0.0, 0.0, 1.0, 0.5),
            0.25,
            ColorBlendType::Layer2,
        );
        sprite(
            pass,
            &textures.obi,
            vec4(0.0, bottom, 1920.0, bottom + 160.0),
            vec4(0.0, 0.5, 1.0, 1.0),
            0.25,
            ColorBlendType::Layer2,
        );

        // a433c..a43c4: the content window slides by canvas WIDTH, not height.
        // a57e0: 80/1664/80 columns and 80/730/88 rows; center is opaque.
        let y = 112.0 + 1920.0 * (1.0 - p);
        let xs = [0.0, 80.0, 1744.0, 1824.0];
        let ys = [0.0, 80.0, 810.0, 898.0];
        for row in 0..3 {
            for column in 0..3 {
                sprite(
                    pass,
                    &textures.window,
                    vec4(
                        48.0 + xs[column],
                        y + ys[row],
                        48.0 + xs[column + 1],
                        y + ys[row + 1],
                    ),
                    vec4(
                        xs[column] / 1824.0,
                        ys[row] / 898.0,
                        xs[column + 1] / 1824.0,
                        ys[row + 1] / 898.0,
                    ),
                    1.0,
                    if row == 1 && column == 1 {
                        ColorBlendType::Opaque
                    } else {
                        ColorBlendType::Layer1
                    },
                );
            }
        }
    }

    /// Draw caption and animated close feedback after the content. The footer
    /// is drawn separately by BacklogScreen (a54b8..a56c0).
    pub fn render_foreground(&self, textures: &BacklogTextures, pass: &mut RenderPass) {
        let y = self.progress * 160.0 - 160.0;
        sprite(
            pass,
            &textures.caption,
            vec4(480.0, y, 1440.0, y + 128.0),
            vec4(0.0, 0.0, 1.0, 1.0),
            1.0,
            ColorBlendType::Layer1,
        );
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| {
            shin_render::shaders::types::vertices::PosColTexVertex {
                position: glam::vec3(1764.0 + 84.0 * corner.x, y + 32.0 + 84.0 * corner.y, 0.0),
                texture_position: corner,
                color: shin_core::primitives::color::UnormColor::WHITE,
            }
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(
                    shin_render::RenderProgramWithArguments::Button {
                        vertices: shin_render::shaders::types::buffer::VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        texture: textures.close.as_source(),
                        transform: top_left_projection_matrix(),
                        flash1: glam::Vec4::ZERO,
                        flash2: vec4(1.0, 1.0, 1.0, self.close_flash.value()),
                    },
                    shin_render::DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}

fn sprite(
    pass: &mut RenderPass,
    texture: &GpuTexture,
    rect: Vec4,
    uv: Vec4,
    alpha: f32,
    blend: ColorBlendType,
) {
    QuadVertices::new()
        .with_box(rect.x, rect.y, rect.z, rect.w)
        .with_tex_box(uv.x, uv.y, uv.z, uv.w)
        .with_color(FloatColor4::from_rgba(1.0, 1.0, 1.0, alpha).into_unorm())
        .render_sprite(
            pass,
            RenderRequestBuilder::new().color_blend_type(blend),
            texture.as_source(),
            top_left_projection_matrix(),
        );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::PreRenderContext;

    #[test]
    fn backlog_requested_close_waits_for_native_flash_and_slide() {
        let mut shell = BacklogShell::default();
        shell.request_close();
        shell.update(Ticks::from_f32(10.0));
        assert!(shell.is_open());
        shell.request_close();
        assert!(!shell.is_open());
        shell.update(Ticks::from_f32(3.5));
        assert_eq!(shell.close_flash.value(), 0.7);
        assert_eq!(shell.progress(), 1.0);
        shell.update(Ticks::from_f32(3.5));
        assert_eq!(shell.close_flash.value(), 0.0);
        assert_eq!(shell.progress(), 1.0);
        shell.update(Ticks::ZERO);
        assert!(!shell.is_closed());
        shell.update(Ticks::from_f32(5.0));
        assert_eq!(shell.progress(), 0.5);
        shell.update(Ticks::from_f32(5.0));
        assert!(shell.is_closed());
    }

    #[test]
    fn backlog_shell_input_waits_for_open_and_close_waits_for_fade() {
        let mut shell = BacklogShell::default();
        assert!(!shell.is_closed());
        assert!(!shell.close_at(glam::vec2(1800.0, 70.0)));
        shell.update(Ticks::from_f32(5.0));
        assert_eq!(shell.progress, 0.5);
        assert!(!shell.is_open());
        shell.update(Ticks::from_f32(20.0));
        assert!(shell.is_open());
        for p in [glam::vec2(1764.0, 32.0), glam::vec2(1847.0, 115.0)] {
            assert!(shell.close_at(p));
        }
        for p in [
            glam::vec2(1848.0, 32.0),
            glam::vec2(1764.0, 116.0),
            glam::vec2(1763.0, 32.0),
        ] {
            assert!(!shell.close_at(p));
        }
        shell.begin_close();
        shell.update(Ticks::from_f32(7.5));
        assert_eq!(shell.progress, 0.25);
        assert!(!shell.is_closed());
        assert!(!shell.close_at(glam::vec2(1800.0, 70.0)));
        shell.update(Ticks::from_f32(20.0));
        assert!(shell.is_closed());
        shell.update(Ticks::from_f32(20.0));
        assert_eq!(shell.progress, 0.0);
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_backlog_shell_matches_supplied_atlas() {
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
                required_features: adapter.features() & wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
                ..Default::default()
            },
            None,
        ))
        .unwrap();
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        let server = AssetServer::new(
            io.into(),
            AssetLoadContext {
                wgpu_device: device.clone(),
                wgpu_queue: queue.clone(),
                bustup_cache: AssetCache::new(),
                picture_cache: Default::default(),
                font_cache: Default::default(),
            },
        );
        let textures = server.load_sync::<BacklogTextures>("/logview.txa").unwrap();
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            1920, 1080,
        )));
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Choice test depth"),
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
            label: Some("Choice test target"),
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

        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::{fs::File, io::Read};
        let mut rom = RomReader::new(
            StatelessFile::new(File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap())
                .unwrap(),
        )
        .unwrap();
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/logview.txa").unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let atlas = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        assert_eq!(atlas.name_to_index.len(), 11);
        let sample = |name: &str, u: f32, v: f32| -> [f32; 4] {
            let im = &atlas.textures[atlas.name_to_index[name]];
            let x = u * im.width() as f32 - 0.5;
            let y = v * im.height() as f32 - 0.5;
            let ix = x.floor();
            let iy = y.floor();
            let fx = x - ix;
            let fy = y - iy;
            std::array::from_fn(|channel| {
                let pixel = |dx: f32, dy: f32| {
                    im.get_pixel(
                        (ix + dx).clamp(0.0, im.width() as f32 - 1.0) as u32,
                        (iy + dy).clamp(0.0, im.height() as f32 - 1.0) as u32,
                    )[channel] as f32
                        / 255.0
                };
                (pixel(0.0, 0.0) * (1.0 - fx) + pixel(1.0, 0.0) * fx) * (1.0 - fy)
                    + (pixel(0.0, 1.0) * (1.0 - fx) + pixel(1.0, 1.0) * fx) * fy
            })
        };
        // Independent scalar reconstruction of the disassembled drawing calls.
        // Sample across seams, the opaque center, and both additive strips.
        let mut checked = 0;
        for progress in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let shell = BacklogShell {
                progress,
                phase: Phase::Open,
                close_flash: Tweener::new(0.0),
            };
            {
                let mut pass = RenderPass::new(
                    context.pipeline_storage,
                    context.dynamic_buffer,
                    context.sampler_store,
                    context.device,
                    context.encoder,
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    None,
                    "Backlog shell",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                shell.render_background(&textures, &mut pass);
                shell.render_foreground(&textures, &mut pass);
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            if progress == 1.0 {
                let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
                for pixel in pixels.chunks_exact(4) {
                    ppm.extend_from_slice(&pixel[..3]);
                }
                std::fs::write("/tmp/shin-backlog-shell.ppm", ppm).unwrap();
            }
            for y in (3..1080).step_by(13) {
                for x in (3..1920).step_by(17) {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let mut color = [0.0f32; 3];
                    let mut draw =
                        |name: &str, rect: [f32; 4], uv: [f32; 4], alpha: f32, blend: u8| {
                            if px < rect[0] || px >= rect[2] || py < rect[1] || py >= rect[3] {
                                return;
                            }
                            let u = uv[0] + (uv[2] - uv[0]) * (px - rect[0]) / (rect[2] - rect[0]);
                            let v = uv[1] + (uv[3] - uv[1]) * (py - rect[1]) / (rect[3] - rect[1]);
                            let c = sample(name, u, v);
                            // Vertex tint is quantized to UNORM8 before interpolation.
                            let a = c[3] * (alpha * 255.0).round() / 255.0;
                            for i in 0..3 {
                                color[i] = match blend {
                                    0 => c[i],
                                    1 => c[i] * a + color[i] * (1.0 - a),
                                    _ => c[i] * a + color[i],
                                };
                                color[i] = (color[i].clamp(0.0, 1.0) * 255.0).round() / 255.0;
                            }
                        };
                    draw(
                        "bg",
                        [0.0, 0.0, 1920.0, 1080.0],
                        [0.0, 0.0, 1.0, 1.0],
                        progress,
                        1,
                    );
                    let top = progress * 160.0 - 160.0;
                    let bottom = 1080.0 - progress * 160.0;
                    draw(
                        "obi",
                        [0.0, top, 1920.0, top + 160.0],
                        [0.0, 0.0, 1.0, 0.5],
                        0.25,
                        2,
                    );
                    draw(
                        "obi",
                        [0.0, bottom, 1920.0, bottom + 160.0],
                        [0.0, 0.5, 1.0, 1.0],
                        0.25,
                        2,
                    );
                    let wy = 112.0 + 1920.0 * (1.0 - progress);
                    // Native patch UVs reduce to these continuous coordinates since
                    // all nine patches preserve their source dimensions.
                    let center = px >= 128.0 && px < 1792.0 && py >= wy + 80.0 && py < wy + 810.0;
                    draw(
                        "window",
                        [48.0, wy, 1872.0, wy + 898.0],
                        [0.0, 0.0, 1.0, 1.0],
                        1.0,
                        if center { 0 } else { 1 },
                    );
                    draw(
                        "caption",
                        [480.0, top, 1440.0, top + 128.0],
                        [0.0, 0.0, 1.0, 1.0],
                        1.0,
                        1,
                    );
                    draw(
                        "close",
                        [1764.0, top + 32.0, 1848.0, top + 116.0],
                        [0.0, 0.0, 1.0, 1.0],
                        1.0,
                        1,
                    );
                    let offset = (y * 1920 + x) * 4;
                    for i in 0..3 {
                        assert!(
                            (pixels[offset + i] as f32 - color[i] * 255.0).abs() <= 2.0,
                            "backlog p={progress} x={x} y={y} channel={i}: actual={} expected={}",
                            pixels[offset + i],
                            color[i] * 255.0
                        );
                    }
                    checked += 1;
                }
            }
        }
        // Independent native footer calls: positions are reconstructed from
        // a5600/a63c0 and the171498 metadata, without using renderer HINTS.
        let mut footer_samples = 0;
        let menu_history = super::super::backlog::Backlog {
            entries: (0..3)
                .map(|i| super::super::backlog::BacklogEntry {
                    save_position: i + 1,
                    item: super::super::backlog::BacklogItem::Checkpoint {
                        title: "幕".into(),
                        text: "章".into(),
                    },
                })
                .collect(),
        };
        let menu_model = super::super::backlog_jump::SceneJump::new(&menu_history, 0, 1).unwrap();
        let mut menu_view = super::super::backlog_jump_view::SceneJumpView::load(
            &server,
            &menu_history,
            &menu_model,
        )
        .unwrap();
        menu_view.prepare(&server, &menu_model);
        for (progress, replay, menu, child_progress) in [
            (0.0, false, false, None),
            (0.5, true, false, None),
            (1.0, true, false, None),
            (1.0, false, false, None),
            (1.0, true, true, None),
            (1.0, false, true, None),
            (1.0, true, true, Some(0.5)),
            (1.0, false, true, Some(1.0)),
        ] {
            {
                let mut pass = RenderPass::new(
                    context.pipeline_storage,
                    context.dynamic_buffer,
                    context.sampler_store,
                    context.device,
                    context.encoder,
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    None,
                    "Backlog native footer",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                super::super::backlog_help::render(&mut pass, &textures, progress, replay, menu);
                if let Some(child_progress) = child_progress {
                    menu_view.render(&mut pass, &textures, &menu_model, child_progress);
                }
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            // label-left/right, row number, width, key-left, key index, enabled.
            let mut groups = vec![
                (1800.0, 1872.0, 1, 72.0, 1754.0, 4, true),
                (1598.0, 1738.0, 2, 140.0, 1552.0, 3, replay),
            ];
            if menu {
                groups.extend([
                    (1296.0, 1536.0, 3, 240.0, 1250.0, 6, true),
                    (1028.0, 1234.0, 0, 206.0, 982.0, 7, true),
                ]);
            } else {
                groups.extend([
                    (1270.0, 1536.0, 4, 266.0, 1224.0, 5, true),
                    (968.0, 1208.0, 3, 240.0, 922.0, 6, true),
                    (700.0, 906.0, 0, 206.0, 654.0, 7, true),
                ]);
            }
            let top = 1100.0 - 88.0 * progress;
            let mut visible_pixels = 0;
            for y in (1005..1080).step_by(2) {
                for x in (0..1920).step_by(5) {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let mut color = [0.0f32; 3];
                    let mut draw = |name: &str, rect: [f32; 4], uv: [f32; 4], tint: f32| {
                        if px < rect[0] || px >= rect[2] || py < rect[1] || py >= rect[3] {
                            return;
                        }
                        let u = uv[0] + (uv[2] - uv[0]) * (px - rect[0]) / (rect[2] - rect[0]);
                        let v = uv[1] + (uv[3] - uv[1]) * (py - rect[1]) / (rect[3] - rect[1]);
                        let c = sample(name, u, v);
                        for k in 0..3 {
                            color[k] = (255.0 * (c[k] * tint * c[3] + color[k] * (1.0 - c[3])))
                                .round()
                                / 255.0;
                        }
                    };
                    for &(left, right, row, width, key_left, key, enabled) in &groups {
                        let tint = if enabled { 1.0 } else { 128.0 / 255.0 };
                        draw(
                            "help_logview",
                            [left, top, right, top + 48.0],
                            [0.0, row as f32 / 6.0, width / 320.0, (row + 1) as f32 / 6.0],
                            tint,
                        );
                        draw(
                            "key",
                            [key_left, top, key_left + 48.0, top + 48.0],
                            [key as f32 / 8.0, 0.0, (key + 1) as f32 / 8.0, 0.5],
                            tint,
                        );
                        if row == 0 {
                            let k = key_left - 48.0;
                            draw(
                                "key",
                                [k, top, k + 48.0, top + 48.0],
                                [0.0, 0.0, 0.125, 0.5],
                                tint,
                            );
                        }
                    }
                    if let Some(child_progress) = child_progress {
                        let slide = 1920.0 * (1.0 - child_progress);
                        if px >= slide && px < slide + 1920.0 {
                            let alpha = (192.0 / 255.0) * ((px - slide) / 420.0).min(1.0);
                            for c in &mut color {
                                *c = (255.0 * ((128.0 / 255.0) * alpha + *c * (1.0 - alpha)))
                                    .round()
                                    / 255.0;
                            }
                        }
                    }
                    let offset = (y * 1920 + x) * 4;
                    for k in 0..3 {
                        assert!(
                            (pixels[offset + k] as f32 - color[k] * 255.0).abs() <= 2.0,
                            "Footer p{progress} replay{replay} menu{menu} at{x},{y} channel{k}: {} != {}",
                            pixels[offset + k],
                            color[k] * 255.0
                        );
                    }
                    if pixels[offset..offset + 3].iter().any(|&v| v > 16) {
                        visible_pixels += 1;
                    }
                    footer_samples += 1;
                }
            }
            if progress == 1.0 {
                assert!(
                    visible_pixels > 1000,
                    "Native footer atlas labels and keys must draw"
                );
            }
        }
        println!(
            "Native backlog footer: {footer_samples} scalar pixel samples across slide, replay tint, menu visibility and child-pane composition"
        );
        // Independently sample row decoration calls, including partial rows,
        // separator atlas seams and the content slide/viewport clipping.
        let mut decoration_samples = 0;
        for (top, height, scroll, progress, selected) in [
            (0.0, 96.0, 0.0, 1.0, true),
            (0.0, 96.0, 0.0, 1.0, false),
            (0.0, 96.0, 50.0, 1.0, true),
            (700.0, 140.0, 0.0, 1.0, true),
            (0.0, 96.0, 0.0, 0.75, true),
        ] {
            {
                let mut pass = RenderPass::new(
                    context.pipeline_storage,
                    context.dynamic_buffer,
                    context.sampler_store,
                    context.device,
                    context.encoder,
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    None,
                    "Backlog row decorations",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                super::super::backlog_decoration::render_row(
                    &mut pass,
                    &textures,
                    super::super::backlog_model::Row { top, height },
                    scroll,
                    progress,
                    selected,
                );
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let slide = 1920.0 * (1.0 - progress);
            for y in (1..1080).step_by(7) {
                for x in (1..1920).step_by(11) {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5 - slide;
                    let ry = py - (188.0 + top - scroll);
                    let mut expected = [0.0f32; 3];
                    if (136.0..1784.0).contains(&px) && (188.0..918.0).contains(&py) {
                        let (image, u, v, alpha) = if (height..height + 14.0).contains(&ry) {
                            let u = if px < 144.0 {
                                (px - 136.0) / 8.0 * 0.25
                            } else if px < 1776.0 {
                                0.25 + (px - 144.0) / 1632.0 * 0.5
                            } else {
                                0.75 + (px - 1776.0) / 8.0 * 0.25
                            };
                            ("line", u, (ry - height) / 14.0, 1.0)
                        } else if selected && (0.0..height).contains(&ry) {
                            let u = if px < 648.0 {
                                (px - 136.0) / 512.0 * 0.4
                            } else if px < 1272.0 {
                                0.4 + (px - 648.0) / 624.0 * 0.2
                            } else {
                                0.6 + (px - 1272.0) / 512.0 * 0.4
                            };
                            let v = if ry < 12.0 {
                                ry / 12.0 * 0.375
                            } else if ry < height - 12.0 {
                                0.375 + (ry - 12.0) / (height - 24.0) * 0.25
                            } else {
                                0.625 + (ry - height + 12.0) / 12.0 * 0.375
                            };
                            ("focus", u, v, 191.0 / 255.0)
                        } else {
                            ("focus", 0.0, 0.0, 0.0)
                        };
                        let color = sample(image, u, v);
                        for i in 0..3 {
                            expected[i] = color[i] * color[3] * alpha * 255.0;
                        }
                    }
                    let offset = (y * 1920 + x) * 4;
                    for i in 0..3 {
                        assert!(
                            (pixels[offset + i] as f32 - expected[i]).abs() <= 2.0,
                            "row focus p={progress} selected={selected} x={x} y={y} channel={i}: actual={} expected={}",
                            pixels[offset + i],
                            expected[i]
                        );
                    }
                    decoration_samples += 1;
                }
            }
        }
        let mut voice_samples = 0;
        for (top, height, scroll, progress, flash) in [
            (0.0, 96.0, 0.0, 1.0, 0.0),
            (0.0, 96.0, 0.0, 1.0, 0.2),
            (0.0, 96.0, 0.0, 1.0, 0.7),
            (0.0, 240.0, 0.0, 1.0, 0.0),
            (0.0, 96.0, 60.0, 1.0, 0.7),
            (650.0, 96.0, 0.0, 1.0, 0.2),
            (0.0, 96.0, 0.0, 0.75, 0.0),
        ] {
            {
                let mut pass = RenderPass::new(
                    context.pipeline_storage,
                    context.dynamic_buffer,
                    context.sampler_store,
                    context.device,
                    context.encoder,
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    None,
                    "Backlog voice icon",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                super::super::backlog_decoration::render_voice(
                    &mut pass,
                    &textures,
                    super::super::backlog_model::Row { top, height },
                    scroll,
                    progress,
                    flash,
                );
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let slide = 1920.0 * (1.0 - progress);
            let icon_top = 188.0 + top - scroll + (height - 76.0) * 0.5;
            for y in (1..1080).step_by(7) {
                for x in (1..1920).step_by(11) {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5 - slide;
                    let mut expected = [0.0f32; 3];
                    if (168.0..276.0).contains(&px)
                        && (188.0..918.0).contains(&py)
                        && (icon_top..icon_top + 76.0).contains(&py)
                    {
                        let color = sample("voice", (px - 168.0) / 108.0, (py - icon_top) / 76.0);
                        for i in 0..3 {
                            expected[i] = (color[i] * (1.0 - flash) + flash) * color[3] * 255.0;
                        }
                    }
                    let offset = (y * 1920 + x) * 4;
                    for i in 0..3 {
                        assert!(
                            (pixels[offset + i] as f32 - expected[i]).abs() <= 2.0,
                            "voice p={progress} flash={flash} x={x} y={y} channel={i}: actual={} expected={}",
                            pixels[offset + i],
                            expected[i]
                        );
                    }
                    voice_samples += 1;
                }
            }
        }
        let mut row_flash_samples = 0;
        for (top, height, scroll, progress, flash) in [
            (0.0, 96.0, 0.0, 1.0, 0.7),
            (0.0, 96.0, 50.0, 1.0, 0.35),
            (700.0, 140.0, 0.0, 1.0, 0.7),
            (0.0, 96.0, 0.0, 0.75, 0.7),
            (0.0, 96.0, 0.0, 1.0, 0.0),
        ] {
            {
                let mut pass = RenderPass::new(
                    context.pipeline_storage,
                    context.dynamic_buffer,
                    context.sampler_store,
                    context.device,
                    context.encoder,
                    TextureTarget {
                        kind: TextureTargetKind::Screen,
                        view: &view,
                    },
                    Some(context.depth_stencil),
                    None,
                    "Backlog confirmation row flash",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::from_rgba(
                        37, 81, 149, 255,
                    )),
                    Some(0),
                    Some(1.0),
                );
                super::super::backlog_decoration::render_confirmation_flash(
                    &mut pass,
                    super::super::backlog_model::Row { top, height },
                    scroll,
                    progress,
                    flash,
                );
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let slide = 1920.0 * (1.0 - progress);
            for y in (1..1080).step_by(7) {
                for x in (1..1920).step_by(11) {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5 - slide;
                    let ry = py - (188.0 + top - scroll);
                    let active = (136.0..1784.0).contains(&px)
                        && (188.0..918.0).contains(&py)
                        && (0.0..height).contains(&ry);
                    let alpha = if active {
                        (flash * 255.0) as u8 as f32 / 255.0
                    } else {
                        0.0
                    };
                    for (channel, base) in [37.0, 81.0, 149.0].into_iter().enumerate() {
                        let expected = base * (1.0 - alpha) + 255.0 * alpha;
                        assert!(
                            (pixels[(y * 1920 + x) * 4 + channel] as f32 - expected).abs() <= 2.0,
                            "Row confirmation flash {x},{y} p={progress} a={flash}"
                        );
                    }
                    row_flash_samples += 1;
                }
            }
        }
        println!("Backlog confirmation row flash: {row_flash_samples} scalar samples");
        println!("Backlog voice atlas/flash comparison: {voice_samples} samples");
        println!("Backlog row focus/separator comparison: {decoration_samples} samples");
        println!("Backlog shell scalar comparison: {checked} pixels at five transition positions");
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_backlog_shell_geometry_and_transitions() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0xacc60, 0x1e210800u32),
            (0xacc78, 0xbd019680u32),
            (0xad0c0, 0x1e210800u32),
            (0xad0d0, 0xbd019660u32),
            (0xa4164, 0x91125821u32),
            (0xa421c, 0x913fdc21u32),
            (0xa4204, 0x1e2a0800u32),
            (0xa42b8, 0x1e2339e1u32),
            (0xa4348, 0x1e2d0800u32),
            (0xa43c4, 0x94000507u32),
            (0xa595c, 0x940084d5u32),
            (0xa597c, 0x94008525u32),
            (0xa54c4, 0x9125c421u32),
            (0xa555c, 0x9102e421u32),
            (0xa55a0, 0x940085c4u32),
            (0xa55cc, 0x910a4260u32),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native backlog shell {address:x}"
            );
        }
        for (address, expected) in [
            (0x159570, 0.1f32),
            (0x15999c, -0.1f32),
            (0x159758, 1920.0f32),
            (0x159614, 1080.0f32),
            (0x15937c, 160.0f32),
            (0x159af8, -160.0f32),
            (0x159584, 48.0f32),
            (0x159484, 112.0f32),
            (0x159550, 898.0f32),
            (0x159480, 1764.0f32),
            (0x15971c, 1848.0f32),
            (0x159694, 32.0f32),
            (0x159658, 116.0f32),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native backlog constant {address:x}"
            );
        }
        for (address, name) in [
            (0x15a496, "bg"),
            (0x160ff7, "obi"),
            (0x15a971, "caption"),
            (0x15c0b9, "close"),
        ] {
            assert_eq!(
                &exe[address + 256..address + 256 + name.len()],
                name.as_bytes()
            );
        }
    }
}
