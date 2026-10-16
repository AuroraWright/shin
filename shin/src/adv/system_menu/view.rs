//! Native SystemMenu row artwork and right-hand title/play-time panels.
use glam::{Mat4, Vec2, Vec4, vec2, vec3, vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::GpuTexture,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColTexVertex},
};

use super::{SystemMenuModel, SystemMenuPhase};

/// Draw the supplied `/adv.txa` `sysmenu` atlas in native row order.
/// The caller supplies the canvas projection and owns the story/menu render pass.
pub fn render_rows(
    model: &SystemMenuModel,
    atlas: &GpuTexture,
    pass: &mut RenderPass,
    transform: Mat4,
) {
    if model.phase() == SystemMenuPhase::Closed {
        return;
    }
    for row in 0..model.items().len() {
        let rect = model.render_rect(row).unwrap();
        let uv = model.atlas_rect(row).unwrap();
        let (slide, flash) = model.feedback(row).unwrap();
        let color = FloatColor4::from_rgba(1.0, 1.0, 1.0, slide).into_unorm();
        let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
            position: vec3(rect.x + rect.z * corner.x, rect.y + rect.w * corner.y, 0.0),
            texture_position: vec2(uv.x + uv.z * corner.x, uv.y + uv.w * corner.y),
            color,
        });
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::Layer1)
                .build(
                    RenderProgramWithArguments::Button {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        texture: atlas.as_source(),
                        transform,
                        flash1: Vec4::ZERO,
                        flash2: vec4(1.0, 1.0, 1.0, flash),
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}

/// Native1d0e0 measures elapsed play time in native ticks,3600 per minute.
/// FCVTZU saturates negative/NaN values to zero and overflow to UINT32_MAX.
pub fn play_time_text(ticks: f64) -> String {
    let minutes = (ticks / 3600.0) as u32;
    let minutes = minutes.min(59_999);
    if minutes < 60 {
        format!("{minutes}分")
    } else {
        format!("{}時間 {}分", minutes / 60, minutes % 60)
    }
}

/// Text content is supplied by the owner: native VM+9110 is snapshotted at open,
/// while the native global play clock is read each frame. ADV ownership is pending.
pub(crate) struct SystemMenuDetail {
    title: Option<crate::adv::bgm_title::TitleText>,
    time: crate::adv::bgm_title::TitleText,
    time_string: String,
    font: std::sync::Arc<crate::asset::font::GpuFontLazy>,
}
impl SystemMenuDetail {
    pub fn load(server: &crate::asset::system::AssetServer, title: &str, ticks: f64) -> Self {
        let font = server
            .load_sync::<crate::asset::font::GpuFontLazy>("/system.fnt")
            .unwrap();
        let make = |text| {
            crate::adv::bgm_title::TitleText::load_gpu(
                &server.context().wgpu_device,
                &server.context().wgpu_queue,
                &font,
                text,
                38.0,
                750.0,
            )
        };
        let time_string = play_time_text(ticks);
        Self {
            title: (!title.is_empty()).then(|| make(title)),
            time: make(&time_string),
            time_string,
            font,
        }
    }
    pub fn update_time(&mut self, server: &crate::asset::system::AssetServer, ticks: f64) {
        let text = play_time_text(ticks);
        if self.time_string != text {
            self.time = crate::adv::bgm_title::TitleText::load_gpu(
                &server.context().wgpu_device,
                &server.context().wgpu_queue,
                &self.font,
                &text,
                38.0,
                750.0,
            );
            self.time_string = text;
        }
    }
    pub fn render(&self, model: &SystemMenuModel, info: &GpuTexture, pass: &mut RenderPass) {
        if model.phase() == SystemMenuPhase::Closed {
            return;
        }
        let (slide, fade) = model.detail_feedback();
        for (index, text) in [self.title.as_ref(), Some(&self.time)]
            .into_iter()
            .enumerate()
        {
            let Some(text) = text else {
                continue;
            };
            if text.size().x <= 0.0 {
                continue;
            }
            let y = if index == 0 { 795.0 } else { 930.0 };
            let vertices =
                shin_render::quad_vertices::build_quad_vertices(|corner| PosColTexVertex {
                    position: vec3(
                        1128.0 + slide * 120.0 + 756.0 * corner.x,
                        y + 64.0 * corner.y,
                        0.0,
                    ),
                    texture_position: vec2(corner.x, index as f32 * 0.5 + corner.y * 0.5),
                    color: FloatColor4::from_rgba(1.0, 1.0, 1.0, fade).into_unorm(),
                });
            let transform = crate::render::top_left_projection_matrix();
            pass.run(
                RenderRequestBuilder::new()
                    .color_blend_type(ColorBlendType::Layer1)
                    .build(
                        RenderProgramWithArguments::Sprite {
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
                            sprite: info.as_source(),
                            transform,
                        },
                        DrawPrimitive::TrianglesStrip,
                    ),
            );
            text.render_colored(
                pass,
                vec2(1872.0 + slide * 120.0 - text.size().x, y + 54.0),
                Vec2::ZERO,
                Vec2::ONE,
                fade,
                Vec4::ONE,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{PreRenderContext, top_left_projection_matrix};
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_system_menu_detail_style_time_and_geometry() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (address, expected) in [
            (0x1d0e0, 0xf0001028),
            (0x1d0e4, 0xf943b908),
            (0x1d0e8, 0xfd400100),
            (0x1d0ec, 0xf0000a28),
            (0x1d0f0, 0xfd46a101),
            (0x1d0f4, 0x1e611800),
            (0x1d0f8, 0x1e790008),
            (0x1d0fc, 0x52911129),
            (0x1d100, 0x72b11109),
            (0x1d104, 0x9ba97d09),
            (0x1d108, 0xd365fd29),
            (0x1d10c, 0x321e0fea),
            (0x1d110, 0x1b0aa12a),
            (0x1d114, 0x53057d08),
            (0x1d118, 0x711d491f),
            (0x1d11c, 0x52807ce8),
            (0x1d120, 0x1a898102),
            (0x1d124, 0x52800768),
            (0x1d128, 0x1a8a8103),
            (0x1d12c, 0x340000c2),
            (0x1d130, 0xb00009e1),
            (0x1d134, 0x91121c21),
            (0x1d138, 0x910083e0),
            (0x1d13c, 0x9403feb1),
            (0x1d140, 0x14000006),
            (0x1d144, 0xb0000a21),
            (0x1d148, 0x913d8421),
            (0x1d14c, 0x910083e0),
            (0x1d150, 0x2a0303e2),
            (0x1d154, 0x9403feab),
            (0x1cb90, 0xf9400a68),
            (0x1cb94, 0xf9423508),
            (0x1cb98, 0x52922209),
            (0x1cb9c, 0xf8696919),
            (0x1cba0, 0x39400328),
            (0x1cba4, 0x34000928),
            (0x1cc24, 0x320003e9),
            (0x1cc28, 0x9103a3e8),
            (0x1cc2c, 0xb900f7e9),
            (0x1cc30, 0x52a84309),
            (0x1cc34, 0xf801411f),
            (0x1cc38, 0xf8004109),
            (0x1cc3c, 0x52a887a8),
            (0x1cc40, 0xb900ebe8),
            (0x1cc44, 0x7901f3ff),
            (0x100ae8, 0xb9400e88),
            (0x100aec, 0x1e2703e9),
            (0x100af0, 0x1e211000),
            (0x100af4, 0x1e3e100a),
            (0x100af8, 0x12800009),
            (0x100afc, 0x1e2f100c),
            (0x100b00, 0x7100051f),
            (0x100b04, 0x1e201d22),
            (0x100b08, 0x320003ea),
            (0x100b0c, 0x1a9f1129),
            (0x100b10, 0x1e201d41),
            (0x100b14, 0x7100091f),
            (0x100b18, 0x1a890148),
            (0x100b1c, 0xb9006a68),
            (0x100b20, 0x32091be8),
            (0x100b24, 0xf800c2ff),
            (0x100b28, 0xf80042ff),
            (0x100b2c, 0xb815c3a8),
            (0x100b30, 0x1e220c00),
            (0x100b34, 0x1e220d2d),
            (0x100b38, 0x381603aa),
            (0x100b3c, 0x1e210d8b),
            (0x100b40, 0xbd400281),
            (0x100b44, 0x1e2d2808),
            (0x100b48, 0x1e283820),
            (0x100b4c, 0xbc1403a0),
            (0x100b50, 0xb9400688),
            (0x100b54, 0x292aa3bf),
            (0x100b58, 0xb9400a88),
            (0x100b5c, 0xb81503a8),
            (0x100b60, 0x39404288),
            (0x100b64, 0x381623a8),
            (0x100b68, 0x39404688),
            (0x100b6c, 0x381613a8),
            (0x100b70, 0xb9401688),
            (0x100b74, 0xb81483a8),
            (0x100b78, 0xb9401a88),
            (0x100b7c, 0xb814c3a8),
            (0x1d930, 0x4ea01c0a),
            (0x1d934, 0xbd49c500),
            (0x1d938, 0x900009e8),
            (0x1d93c, 0x1e2703e9),
            (0x1d940, 0xbd4a6501),
            (0x1d944, 0x910243e8),
            (0x1d948, 0x4ea91d22),
            (0x1d94c, 0x940211b9),
            (0x1d950, 0x91036260),
            (0x1d954, 0x940276d3),
            (0x1d958, 0x900009e8),
            (0x1d95c, 0xbd4a1901),
            (0x1d960, 0x910043e8),
            (0x1d964, 0x1e210800),
            (0x1d968, 0x4ea91d21),
            (0x1d96c, 0x4ea91d22),
            (0x1d970, 0x940211b0),
            (0x1d974, 0x910143e8),
            (0x1d978, 0x910243e0),
            (0x1d97c, 0x910043e1),
            (0x1d980, 0x94021004),
            (0x1da0c, 0x900009e8),
            (0x1da10, 0xbd479d00),
            (0x1da14, 0x900009e8),
            (0x1da18, 0xbd471902),
            (0x1da1c, 0x900009e8),
            (0x1da20, 0xbd490103),
            (0x1da24, 0x4ea91d21),
            (0x1da28, 0x910343e0),
            (0x1da2c, 0x9402a4a1),
            (0x1da30, 0x1e2e100b),
            (0x1da34, 0x1e2c1003),
            (0x1da38, 0x4ea91d20),
            (0x1da3c, 0x4ea91d21),
            (0x1da40, 0x4eab1d62),
            (0x1da44, 0x910343e0),
            (0x1da48, 0x9402a4ae),
            (0x1da4c, 0x9104c275),
            (0x1da50, 0xaa1503e0),
            (0x1da54, 0x94027693),
            (0x1da58, 0x4ea01c03),
            (0x1da5c, 0x4eab1d60),
            (0x1da60, 0x4eab1d61),
            (0x1da64, 0x4eab1d62),
            (0x1da68, 0x9400e33a),
            (0x1da90, 0xbd400be0),
            (0x1da94, 0x900009e8),
            (0x1da98, 0xbd497901),
            (0x1da9c, 0x910043e8),
            (0x1daa0, 0x1e2a3800),
            (0x1daa4, 0x4ea91d22),
            (0x1dae0, 0x94027670),
            (0x1dae4, 0x4ea01c03),
            (0x1dae8, 0x4eab1d60),
            (0x1daec, 0x4eab1d61),
            (0x1daf0, 0x4eab1d62),
            (0x1daf4, 0x9400e317),
            (0x1daf8, 0x910143e1),
            (0x1dafc, 0xaa1403e0),
            (0x1db00, 0x2d0a07e0),
            (0x1db04, 0x2d0b0fe2),
            (0x1db08, 0x94038d9e),
            (0x1db0c, 0xaa1503e0),
            (0x1db10, 0x94027664),
            (0x1db14, 0x4ea01c03),
            (0x1db18, 0x4ea91d20),
            (0x1db1c, 0x4ea91d21),
            (0x1db20, 0x4ea91d22),
            (0x1db24, 0x9400e30b),
            (0x1db28, 0x910143e1),
            (0x1db2c, 0xaa1403e0),
            (0x1db30, 0x2d0a07e0),
            (0x1db34, 0x2d0b0fe2),
            (0x1db38, 0x94038d9a),
            (0x1db3c, 0x910243e1),
            (0x1db40, 0xaa1403e0),
            (0x1db44, 0x94038da3),
            (0x1db64, 0xbd49c500),
            (0x1db68, 0x900009e8),
            (0x1db6c, 0x1e2703e9),
            (0x1db70, 0xbd464901),
            (0x1db74, 0x910243e8),
            (0x1db78, 0x4ea91d22),
            (0x1db7c, 0x9402112d),
            (0x1db80, 0x91036260),
            (0x1db84, 0x94027647),
            (0x1db88, 0x900009e8),
            (0x1db8c, 0xbd4a1901),
            (0x1db90, 0x910043e8),
            (0x1db94, 0x1e210800),
            (0x1db98, 0x4ea91d21),
            (0x1db9c, 0x4ea91d22),
            (0x1dba0, 0x94021124),
            (0x1dba4, 0x910143e8),
            (0x1dc3c, 0x900009e8),
            (0x1dc40, 0xbd479d00),
            (0x1dc44, 0x900009e8),
            (0x1dc48, 0xbd471902),
            (0x1dc4c, 0x900009e8),
            (0x1dc50, 0xbd490103),
            (0x1dc54, 0x4ea91d21),
            (0x1dc58, 0x910343e0),
            (0x1dc5c, 0x9402a415),
            (0x1dc60, 0x1e2e100b),
            (0x1dc64, 0x1e2c1001),
            (0x1dc68, 0x4ea91d20),
            (0x1dc6c, 0x910343e0),
            (0x1dc70, 0x4eab1d62),
            (0x1dc74, 0x4eab1d63),
            (0x1dc78, 0x9402a422),
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected,
                "native{address:x}"
            );
        }
        for (address, expected) in [
            (0x1599c4, -48.0_f32),
            (0x159a64, 795.0),
            (0x159648, 930.0),
            (0x159a18, 120.0),
            (0x15979c, 1176.0),
            (0x159718, 1932.0),
            (0x159900, 64.0),
            (0x159758, 1920.0),
            (0x159978, 54.0),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap()),
                expected
            );
        }
        assert_eq!(
            f64::from_le_bytes(exe[0x164d40 + 256..0x164d48 + 256].try_into().unwrap()),
            3600.0
        );
        assert_eq!(
            &exe[0x15a487 + 256..0x15a487 + 256 + 15],
            "%d時間 %d分\0".as_bytes()
        );
        assert_eq!(
            &exe[0x162f61 + 256..0x162f61 + 256 + 6],
            "%d分\0".as_bytes()
        );
    }
    #[test]
    fn native_play_time_minutes_hours_and_saturation() {
        for (ticks, expected) in [
            (0.0, "0分"),
            (3599.999, "0分"),
            (3600.0, "1分"),
            (59.0 * 3600.0, "59分"),
            (60.0 * 3600.0, "1時間 0分"),
            (61.0 * 3600.0, "1時間 1分"),
            (59999.0 * 3600.0, "999時間 59分"),
            (60000.0 * 3600.0, "999時間 59分"),
            (f64::INFINITY, "999時間 59分"),
            (-3600.0, "0分"),
            (f64::NAN, "0分"),
        ] {
            assert_eq!(play_time_text(ticks), expected);
        }
    }
    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_system_menu_rows_match_supplied_atlas() {
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
        let mut io = LayeredAssetIo::new();
        io.try_with_rom(std::env::var("SHIN_REFERENCE_ROM").unwrap())
            .unwrap();
        let server = AssetServer::new(
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
        );
        let textures = server
            .load_sync::<crate::adv::characters::view::CharacterTextures>("/adv.txa")
            .unwrap();
        let resize = SurfaceResizeSource::new(ViewportParams::both(winit::dpi::PhysicalSize::new(
            1920, 1080,
        )));
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("System-menu test depth"),
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
            label: Some("System-menu test target"),
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
        rom.open_file(rom.find_file("/adv.txa").unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let atlas = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
        assert_eq!(
            atlas.textures[atlas.name_to_index["sysmenu"]].dimensions(),
            (792, 840)
        );
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

        let mut checked = 0;
        let mut max_error = 0.0_f32;
        // Independent native geometry reconstruction for each authored ID;
        // include missing conditional rows, opening, held flash, release and closing.
        for case in 0..13 {
            let (characters, information) = match case {
                1 => (-1, false),
                2 => (0, false),
                3 => (-1, true),
                _ => (0, true),
            };
            let mut model = SystemMenuModel::new(characters, information);
            let mut slides = vec![1.0_f32; model.items().len()];
            let mut selected = 0;
            let mut flash = 0.0;
            if case == 0 {
                model.update(4.0);
                for (row, slide) in slides.iter_mut().enumerate() {
                    let t = ((4.0 - row as f32 * 2.0) / 10.0).clamp(0.0, 1.0);
                    *slide = (t * std::f32::consts::FRAC_PI_2).sin();
                }
            } else {
                model.update(22.0);
                model.update(0.0);
                if (4..=7).contains(&case) {
                    selected = 3;
                    model.pointer_probe(vec2(30.0, 72.0 + selected as f32 * 128.0));
                    model.pointer_press();
                    model.update(1.0);
                    flash = 0.2;
                    if case == 5 {
                        model.pointer_leave();
                        model.update(0.5);
                        flash = 0.1;
                    } else if case == 6 {
                        model.pointer_release();
                        model.update(2.5);
                        flash = 0.7;
                    } else if case == 7 {
                        model.pointer_release();
                        model.update(5.0);
                        flash = 0.2;
                    }
                } else if case >= 9 {
                    selected = 5; // Original manual item in the complete seven-row menu.
                    model.pointer_probe(vec2(30.0, 72.0 + selected as f32 * 128.0));
                    model.pointer_press();
                    model.pointer_release();
                    model.update(7.0);
                    model.update(0.0);
                    assert_eq!(model.take_item(), Some(5));
                    assert_eq!(model.phase(), SystemMenuPhase::Child);
                    let ticks = if case == 9 { 4.0 } else { 22.0 };
                    model.update(ticks);
                    for (row, slide) in slides.iter_mut().enumerate() {
                        let t = ((ticks - row as f32 * 2.0) / 10.0).clamp(0.0, 1.0);
                        *slide = (t * std::f32::consts::FRAC_PI_2).cos();
                    }
                    if case >= 11 {
                        model.child_response(1);
                        let ticks = if case == 11 { 4.0 } else { 22.0 };
                        model.update(ticks);
                        for (row, slide) in slides.iter_mut().enumerate() {
                            let t = ((ticks - row as f32 * 2.0) / 10.0).clamp(0.0, 1.0);
                            *slide = (t * std::f32::consts::FRAC_PI_2).sin();
                        }
                    }
                } else if case == 8 {
                    model.close();
                    model.update(4.0);
                    for (row, slide) in slides.iter_mut().enumerate() {
                        let t = ((4.0 - row as f32 * 2.0) / 10.0).clamp(0.0, 1.0);
                        *slide = (t * std::f32::consts::FRAC_PI_2).cos();
                    }
                }
            }
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
                    "Native system-menu rows",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                render_rows(
                    &model,
                    textures.get("sysmenu"),
                    &mut pass,
                    top_left_projection_matrix(),
                );
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            let original_ids: Vec<usize> = (0..7)
                .filter(|&id| (id != 3 || characters >= 0) && (id != 4 || information))
                .collect();
            for (row, &item) in original_ids.iter().enumerate() {
                let left = 24.0 + (slides[row] - 1.0) * 504.0;
                let top = 72.0 + row as f32 * 128.0;
                for y in ((top as usize + 2)..(top as usize + 118)).step_by(3) {
                    for x in ((left.max(0.0).ceil() as usize + 2)
                        ..((left + 396.0).max(0.0).floor() as usize).saturating_sub(2))
                        .step_by(3)
                    {
                        let u = if row == selected { 0.0 } else { 0.5 }
                            + ((x as f32 + 0.5 - left) / 396.0) * 0.5;
                        let v = (item as f32 * 120.0 + (y as f32 + 0.5 - top)) / 840.0;
                        let source = sample("sysmenu", u, v);
                        // The atlas is antialiased; opaque interior samples isolate
                        // flash, atlas-column choice, row packing and slide opacity.
                        if source[3] < 0.999999 {
                            continue;
                        }
                        let f = if row == selected { flash } else { 0.0 };
                        let alpha = (slides[row] * 255.0).round() / 255.0;
                        let offset = (y * 1920 + x) * 4;
                        for channel in 0..3 {
                            let expected =
                                ((source[channel] * (1.0 - f) + f) * alpha * 255.0).round();
                            let error = (pixels[offset + channel] as f32 - expected).abs();
                            max_error = max_error.max(error);
                            assert!(
                                error <= 1.1,
                                "case{case},row{row},item{item},({x},{y}),channel{channel}:{} versus{expected}",
                                pixels[offset + channel]
                            );
                        }
                        assert_eq!(pixels[offset + 3], 255);
                        checked += 1;
                    }
                }
            }
            for (x, y) in [(500, 80), (1200, 500), (20, 1000)] {
                assert_eq!(
                    &pixels[(y * 1920 + x) * 4..(y * 1920 + x) * 4 + 4],
                    &[0, 0, 0, 255]
                );
            }
        }
        let mut detail =
            SystemMenuDetail::load(&server, "10/4（土）8:00 新島空港到着", 61.0 * 3600.0);
        assert_eq!(detail.time_string, "1時間 1分");
        let mut detail_samples = 0;
        let mut shown_pixels = Vec::new();
        let mut blank_pixels = Vec::new();
        for case in 0..5 {
            let mut model = SystemMenuModel::new(0, true);
            if case != 0 {
                model.update(22.0);
                model.update(0.0);
            }
            if case == 2 {
                model.close();
                model.update(5.0);
            }
            if case == 3 {
                detail.update_time(&server, 3600.0);
                assert_eq!(detail.time_string, "1分");
            }
            if case == 4 {
                detail = SystemMenuDetail::load(&server, "", 3600.0);
            }
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
                    "Native SystemMenu detail panels",
                );
                pass.clear(
                    Some(shin_core::primitives::color::UnormColor::BLACK),
                    Some(0),
                    Some(1.0),
                );
                detail.render(&model, textures.get("info"), &mut pass);
            }
            let pixels = crate::layer::user::readback_test_texture(&mut context, &target);
            if case == 0 {
                blank_pixels = pixels.clone();
                assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
            }
            if case == 1 {
                shown_pixels = pixels.clone();
            }
            let (slide, fade) = match case {
                0 => (1.0, 0.0),
                2 => (1.0 - (0.5 * std::f32::consts::FRAC_PI_2).cos(), 0.5),
                _ => (0.0, 1.0),
            };
            let left = 1128.0 + slide * 120.0;
            for index in 0..2 {
                let y = if index == 0 { 795 } else { 930 };
                if case == 4 && index == 0 {
                    assert_eq!(
                        &pixels[y * 1920 * 4..(y + 120) * 1920 * 4],
                        &blank_pixels[y * 1920 * 4..(y + 120) * 1920 * 4]
                    );
                    continue;
                }
                // Top40px precede text origin54 and isolate authored panel artwork.
                for py in (y + 2..y + 40).step_by(3) {
                    for x in ((left.ceil() as usize + 2)..1880).step_by(7) {
                        let source = sample(
                            "info",
                            (x as f32 + 0.5 - left) / 756.0,
                            index as f32 * 0.5 + (py as f32 + 0.5 - y as f32) / 128.0,
                        );
                        let alpha = (fade * 255.0_f32).round() / 255.0;
                        for channel in 0..3 {
                            let expected = (source[channel] * source[3] * alpha * 255.0).round();
                            assert!(
                                (pixels[(py * 1920 + x) * 4 + channel] as f32 - expected).abs()
                                    <= 1.1,
                                "detail case{case},({x},{py}),channel{channel}"
                            );
                        }
                        detail_samples += 1;
                    }
                }
            }
            if case == 1 || case == 3 {
                for (y, min_white) in [(849, 100), (984, 50)] {
                    let white = (y..y + 55)
                        .flat_map(|py| (1128..1872).map(move |x| (py * 1920 + x) * 4))
                        .filter(|&i| pixels[i] > 245 && pixels[i + 1] > 245 && pixels[i + 2] > 245)
                        .count();
                    assert!(
                        white > min_white,
                        "detail case{case},text y{y},white glyph pixels{white}"
                    );
                }
            }
            if case == 2 || case == 3 {
                assert_ne!(pixels, shown_pixels);
            }
        }
        assert!(detail_samples > 1000);
        // Bind an actual VM snapshot and live clock through the screen object;
        // compare complete frames with independently supplied native components.
        {
            use crate::adv::system_menu::screen::{SystemMenuRequest, SystemMenuScreen};
            let title = "10/4（土）8:00 新島空港到着";
            let mut vm = crate::adv::VmState::for_scenario(23, 42);
            vm.scene_title = title.into();
            vm.information.select_character_grid(0, 3);
            vm.information.acquire_tips(&[7, 2]);
            let snapshot = crate::adv::system_menu::SystemMenuSnapshot::from_vm(&vm);
            let mut screen = SystemMenuScreen::new(&server, snapshot, textures.clone(), 3599.5);
            vm.scene_title = "Changed after menu construction".into();
            vm.information.select_character_grid(-1, 0);
            vm.information.acquired_tips.clear();
            assert_eq!(screen.snapshot().scene_title, title);
            assert_eq!(screen.snapshot().episode(), 7);
            let mut expected_model = SystemMenuModel::new(0, true);
            let mut expected_detail = SystemMenuDetail::load(&server, title, 3599.5);
            for (case, delta, ticks) in [
                (0, 0.0, 3599.5),
                (1, 22.0, 3599.5),
                (2, 0.0, 3600.0),
                (3, 4.0, 3600.0),
                (4, 22.0, 3600.0),
                (5, 22.0, 3600.0),
            ] {
                if case == 3 {
                    use crate::adv::input_area::{PointerEvent, PointerFrame, PointerKind};
                    let mut simultaneous = enum_map::EnumMap::<
                        crate::app::AppAction,
                        shin_input::ActionState,
                    >::default();
                    simultaneous[crate::app::AppAction::Cancel].is_clicked = true;
                    for (number, time_seconds, events) in [
                        (
                            0,
                            0.0,
                            vec![PointerEvent {
                                kind: PointerKind::Down,
                                id: 1,
                                position: glam::ivec2(30, 72 + 5 * 128),
                            }],
                        ),
                        (1, 0.15, vec![]),
                        (
                            2,
                            0.16,
                            vec![PointerEvent {
                                kind: PointerKind::Up,
                                id: 1,
                                position: glam::ivec2(1200, 500),
                            }],
                        ),
                    ] {
                        screen.input_frame(
                            &PointerFrame {
                                number,
                                time_seconds,
                                events: &events,
                            },
                            &simultaneous,
                        );
                    }
                    // Independent component receives native target callbacks directly.
                    expected_model.pointer_probe(vec2(30.0, 72.0 + 5.0 * 128.0));
                    expected_model.pointer_press();
                    expected_model.pointer_release();
                    screen.update(shin_core::time::Ticks::from_f32(7.0), &server, ticks);
                    screen.update(shin_core::time::Ticks::ZERO, &server, ticks);
                    expected_model.update(7.0);
                    expected_model.update(0.0);
                    assert_eq!(screen.take_request(), Some(SystemMenuRequest::Manual));
                    assert_eq!(expected_model.take_item(), Some(5));
                    assert_eq!(screen.take_request(), None);
                    let mut input = enum_map::EnumMap::<
                        crate::app::AppAction,
                        shin_input::ActionState,
                    >::default();
                    input[crate::app::AppAction::Cancel].is_clicked = true;
                    screen.input(&input);
                    screen.child_response(0);
                    assert_eq!(screen.model.phase(), SystemMenuPhase::Child);
                }
                if case == 5 {
                    screen.child_response(1);
                    expected_model.child_response(1);
                }
                screen.update(shin_core::time::Ticks::from_f32(delta), &server, ticks);
                expected_model.update(delta);
                expected_detail.update_time(&server, ticks);
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            kind: TextureTargetKind::Screen,
                            view: &view,
                        },
                        Some(context.depth_stencil),
                        "Bound system-menu screen",
                    );
                    pass.clear(
                        Some(shin_core::primitives::color::UnormColor::BLACK),
                        Some(0),
                        Some(1.0),
                    );
                    screen.render(&mut pass);
                }
                let actual = crate::layer::user::readback_test_texture(&mut context, &target);
                {
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            kind: TextureTargetKind::Screen,
                            view: &view,
                        },
                        Some(context.depth_stencil),
                        "Independent menu components",
                    );
                    pass.clear(
                        Some(shin_core::primitives::color::UnormColor::BLACK),
                        Some(0),
                        Some(1.0),
                    );
                    render_rows(
                        &expected_model,
                        textures.get("sysmenu"),
                        &mut pass,
                        top_left_projection_matrix(),
                    );
                    expected_detail.render(&expected_model, textures.get("info"), &mut pass);
                }
                let expected = crate::layer::user::readback_test_texture(&mut context, &target);
                assert!(
                    actual == expected,
                    "Bound menu screen case{case} differs from independent components"
                );
            }
            assert_eq!(screen.snapshot().information.acquired_tips, [7, 2]);
            assert_eq!(screen.snapshot().information.character_grid, 0);
            assert_eq!(screen.take_sounds(), ["decide"]);
            eprintln!(
                "Bound SystemMenu screen:6 full-frame component comparisons; retained VM title/characters/tips, live minute refresh, manual request, child gate and reopening pass"
            );
        }
        eprintln!(
            "SystemMenu detail: {detail_samples} decoded plate samples,5 cases; real system-font title/time glyphs, fade, minute refresh and absent-title pass"
        );
        assert!(checked > 20000);
        eprintln!(
            "SystemMenu rows: {checked} decoded opaque samples,13 cases,max RGB error {max_error}"
        );
    }
}
