use std::sync::Arc;

use glam::{Vec2, vec2, vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};

use crate::{
    asset::font::{GpuFontGlyphHandle, GpuFontLazy},
    layer::message_layer::MessageboxTextures,
    render::{PreRenderContext, top_left_projection_matrix},
};

#[path = "choice_model.rs"]
pub(crate) mod model;
pub use model::ChoiceModel;
pub(crate) use model::shuffle as shuffle_choice_indices;
use model::{GAP, Phase};
struct Label {
    text: super::bgm_title::TitleText,
    seen: bool,
}

pub struct ChoiceMenu {
    pub model: ChoiceModel,
    labels: Vec<Label>,
    textures: Arc<MessageboxTextures>,
    hidden: bool,
    opacity: f32,
}

impl ChoiceMenu {
    pub fn new(
        ctx: &PreRenderContext,
        font: Arc<GpuFontLazy>,
        textures: Arc<MessageboxTextures>,
        variants: &[String],
        mask: i32,
        seen: impl Fn(usize) -> bool,
    ) -> Self {
        let mut model = ChoiceModel::new(variants.len(), mask);
        let labels: Vec<_> = model
            .indices
            .iter()
            .map(|&index| Label {
                text: super::bgm_title::TitleText::load_gpu(
                    ctx.device,
                    ctx.queue,
                    &font,
                    &variants[index],
                    48.0,
                    1000.0,
                ),
                seen: seen(index),
            })
            .collect();
        model.heights = labels.iter().map(|l| l.text.size().y + 16.0).collect();
        Self {
            model,
            labels,
            textures,
            hidden: false,
            opacity: 1.0,
        }
    }

    pub fn can_hide(&self) -> bool {
        self.model.phase == Phase::Waiting
    }
    pub fn set_hidden(&mut self, hidden: bool) {
        if hidden {
            self.model.cancel_pointer();
        }
        self.hidden = hidden;
    }
    pub fn update(&mut self, delta: shin_core::time::Ticks) {
        let step = delta.as_f32() * 0.1;
        self.opacity = (self.opacity + if self.hidden { -step } else { step }).clamp(0.0, 1.0);
        if self.opacity >= 1.0 {
            self.model.update(delta);
        }
    }
    pub fn navigate(&mut self, delta: i32) {
        if self.opacity >= 1.0 {
            self.model.move_selection(delta);
        }
    }
    pub fn confirm(&mut self) {
        if self.opacity >= 1.0 {
            self.model.confirm();
        }
    }
    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        self.model.take_sounds()
    }

    pub fn input(
        &mut self,
        state: &enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
    ) {
        use crate::app::AppAction as A;
        if self.opacity >= 1.0 {
            self.model.input(
                state[A::Up].is_clicked_or_repeated,
                state[A::AnyDown].is_clicked_or_repeated,
                state[A::Left].is_clicked_or_repeated,
                state[A::Right].is_clicked_or_repeated,
                state[A::Up].is_held || state[A::AnyDown].is_held,
                state[A::Enter].is_clicked,
            );
        }
    }

    pub fn pointer(&mut self, position: Vec2, clicked: bool) {
        // Complete gesture helper for callers that supply a click rather than
        // held state. The window path uses pointer_state for press/release.
        if clicked {
            self.pointer_state(position, true);
        }
        self.pointer_state(position, false);
    }

    pub fn pointer_state(&mut self, position: Vec2, held: bool) {
        if self.opacity >= 1.0 {
            self.model.pointer_state(position, held);
        } else {
            self.ignore_pointer_state(held);
        }
    }

    pub fn ignore_pointer_state(&mut self, held: bool) {
        self.model.ignore_pointer_state(held);
    }

    pub fn render(&self, pass: &mut RenderPass) {
        if self.opacity <= 0.0 {
            return;
        }
        let builder = RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1);
        let mut y = self.model.top();
        for ((label, row), &height) in self
            .labels
            .iter()
            .zip(&self.model.rows)
            .zip(&self.model.heights)
        {
            let transform = top_left_projection_matrix()
                * glam::Mat4::from_translation(glam::vec3(row.x.value(), y, 0.0));
            let alpha = self.opacity * row.alpha.value();
            let draw_background = |pass: &mut RenderPass,
                                   texture: &shin_render::gpu_texture::GpuTexture,
                                   opacity: f32| {
                if opacity <= 0.0 {
                    return;
                }
                for (top, bottom, uv_top, uv_bottom) in [
                    (-8.0, 0.0, 0.0, 0.1),
                    (0.0, height, 0.1, 0.9),
                    (height, height + 8.0, 0.9, 1.0),
                ] {
                    QuadVertices::new()
                        .with_box(360.0, top, 1560.0, bottom)
                        .with_tex_box(0.0, uv_top, 1.0, uv_bottom)
                        .with_color(FloatColor4::from_rgba(1.0, 1.0, 1.0, opacity).into_unorm())
                        .render_sprite(pass, builder, texture.as_source(), transform);
                }
            };
            draw_background(pass, &self.textures.select, alpha);
            label.text.render_colored(
                pass,
                vec2(960.0 + row.x.value(), y + 8.0),
                vec2(0.5, 0.0),
                Vec2::ONE,
                alpha,
                if label.seen {
                    vec4(0.75, 0.75, 1.0, 1.0)
                } else {
                    glam::Vec4::ONE
                },
            );
            draw_background(
                pass,
                &self.textures.select_cursor,
                alpha * row.focus.value() * 0.5,
            );
            draw_background(
                pass,
                &self.textures.select_cursor,
                alpha * row.flash.value(),
            );
            y += height + GAP;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_select_and_quiz_hide_interfaces() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        let mut relocations = std::collections::BTreeMap::new();
        for offset in (0x11e040..0x152eb8).step_by(24) {
            let address = u64::from_le_bytes(exe[offset + 256..offset + 264].try_into().unwrap());
            let target = u64::from_le_bytes(exe[offset + 272..offset + 280].try_into().unwrap());
            relocations.insert(address, target);
        }
        // Exact overlay-vtable +0x68 eligibility and +0x80 hide callbacks.
        for (base, eligibility, hide) in [
            (0x220dd8, 0xf0fd0, 0xf1000),
            (0x1dbe68, 0xc8990, 0x45e30),
            (0x1d4dc0, 0x45e00, 0x45e30),
            (0x1d7740, 0x6ffe0, 0x45e30),
            (0x1d9c98, 0x9ba60, 0x45e30),
        ] {
            assert_eq!(relocations.get(&(base + 0x68)), Some(&eligibility));
            assert_eq!(relocations.get(&(base + 0x80)), Some(&hide));
        }
        assert_eq!(word(0x159570), 0.1f32.to_bits());
        for (a, expected) in [
            (0xf0fd0, 0xb9408008),
            (0xf0fd4, 0x7100091f),
            (0xf0fd8, 0x1a9f17e0),
            (0xf1000, 0x12000028),
            (0xf1004, 0x39014008),
            (0xf07b4, 0xbd457120),
            (0xf07b8, 0x1e200900),
            (0xf07c0, 0x39414268),
            (0xf07c8, 0x1e203820),
            (0xf07e0, 0x1e212800),
            (0xf0814, 0x540018a4),
            (0xf0ddc, 0xbd405661),
            (0xc8990, 0x2a1f03e0),
            (0x45e00, 0x2a1f03e0),
            (0x6ffe0, 0x2a1f03e0),
            (0x9ba60, 0x2a1f03e0),
            (0x45e30, 0xd65f03c0),
        ] {
            assert_eq!(word(a), expected, "native SELECT hide {a:x}");
        }
    }

    #[test]
    fn hidden_rows_preserve_original_result_and_navigation_wraps() {
        use shin_core::time::Ticks;
        let mut choice = ChoiceModel::new(7, 0b1010010);
        assert_eq!(choice.indices, [1, 4, 6]);
        choice.update(Ticks::from_f32(100.0));
        choice.update(Ticks::ZERO);
        choice.move_selection(-1);
        choice.confirm();
        assert_eq!(choice.result, None);
        choice.update(Ticks::from_f32(7.0));
        choice.update(Ticks::ZERO);
        choice.update(Ticks::from_f32(100.0));
        choice.update(Ticks::ZERO);
        assert_eq!(choice.result, Some(6));
        choice.move_selection(1);
        choice.confirm();
        assert_eq!(choice.result, Some(6));
        let mut empty = ChoiceModel::new(7, 0);
        empty.move_selection(1);
        empty.confirm();
        assert_eq!(empty.result, None);
    }
    #[test]
    fn pointer_hit_testing_matches_native_row_spacing() {
        let choice = ChoiceModel::new(2, -1);
        assert_eq!(choice.row_at(vec2(960.0, 330.0)), Some(0));
        assert_eq!(choice.row_at(vec2(960.0, 430.0)), Some(1));
        assert_eq!(choice.row_at(vec2(0.0, 430.0)), None);
        assert_eq!(choice.row_at(vec2(960.0, 405.0)), None);
    }

    #[test]
    #[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
    fn reference_choice_menu_renders_with_supplied_assets() {
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
        let font = server
            .load_sync::<GpuFontLazy>("/newrodin-medium.fnt")
            .unwrap();
        let textures = server
            .load_sync::<MessageboxTextures>("/msgtex.txa")
            .unwrap();
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
        let context = PreRenderContext {
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
        let mut menu = ChoiceMenu::new(
            &context,
            font,
            textures,
            &["ルシファー".into(), "レヴィアタン".into(), "サタン".into()],
            0x1005,
            |i| i == 0,
        );
        assert_eq!(menu.model.indices.len(), 2);
        let mut original_indices = menu.model.indices.clone();
        original_indices.sort_unstable();
        assert_eq!(original_indices, [0, 2]);
        for (label, &index) in menu.labels.iter().zip(&menu.model.indices) {
            assert_eq!(label.seen, index == 0);
        }
        let expected_result = menu.model.indices[1] as i32;
        menu.update(shin_core::time::Ticks::from_f32(100.0));
        menu.update(shin_core::time::Ticks::ZERO);
        let position = vec2(960.0, menu.model.top() + menu.model.heights[0] + GAP + 20.0);
        menu.pointer_state(position, true);
        assert_eq!(menu.model.selected, Some(1));
        assert_eq!(menu.model.result, None);
        assert!(menu.can_hide(), "Press must leave menu waiting");
        menu.update(shin_core::time::Ticks::from_f32(3.5));
        assert_eq!(menu.model.rows[1].flash.value(), 0.7);
        menu.pointer_state(position, false);
        assert!(!menu.can_hide(), "Release starts confirmation animation");
        {
            let mut pass = RenderPass::new(
                &mut pipelines,
                &mut dynamic,
                &samplers,
                &device,
                &mut encoder,
                TextureTarget {
                    kind: TextureTargetKind::Screen,
                    view: &view,
                },
                Some(DepthStencilTarget { view: &depth_view }),
                None,
                "Choice menu",
            );
            pass.clear(
                Some(shin_core::primitives::color::UnormColor::BLACK),
                Some(0),
                Some(1.0),
            );
            menu.render(&mut pass);
        }
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 1920 * 1080 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1920 * 4),
                    rows_per_image: Some(1080),
                },
            },
            target.size(),
        );
        let mut uploads = device.create_command_encoder(&Default::default());
        dynamic.finish(&mut uploads);
        queue.submit([uploads.finish(), encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap()
            });
        device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
        receive.recv().unwrap().unwrap();
        let pixels = readback.slice(..).get_mapped_range()
            .expect("Readback mapping must be ready");
        for row in 0..2 {
            let y = (menu.model.top()
                + menu.model.heights.iter().take(row).sum::<f32>()
                + row as f32 * GAP
                + 40.0) as usize;
            let nonblack = pixels[y * 1920 * 4..(y + 1) * 1920 * 4]
                .chunks_exact(4)
                .filter(|p| p[0] > 0 || p[1] > 0 || p[2] > 0)
                .count();
            assert!(nonblack > 100, "Choice row {row} did not render");
        }
        let mut ppm = b"P6\n1920 1080\n255\n".to_vec();
        for pixel in pixels.chunks_exact(4) {
            ppm.extend_from_slice(&pixel[..3]);
        }
        std::fs::write("/tmp/shin-choice-menu.ppm", ppm).unwrap();
        menu.update(shin_core::time::Ticks::from_f32(7.0));
        menu.update(shin_core::time::Ticks::ZERO);
        menu.update(shin_core::time::Ticks::from_f32(100.0));
        menu.update(shin_core::time::Ticks::ZERO);
        assert_eq!(menu.model.result, Some(expected_result));
    }
}
