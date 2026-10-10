use dpi::PhysicalSize;
use glam::{vec2, Mat4, Vec2, Vec4};
use shin_render::{
    gpu_texture::{GpuTexture, TextureKind},
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::texture::TextureSource,
    shaders::types::{buffer::VertexSource, vertices::PosTexVertex},
    DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
};

use crate::h264_decoder::Nv12Frame;

// we could utilize `wgpu::Features::TEXTURE_FORMAT_NV12` to have some native YUV textures, but it's not supported on all platforms
// so we are going to store Y and UV planes separately
pub struct VideoFrameTexture {
    tex_y: GpuTexture,
    tex_uv: GpuTexture,
    size: PhysicalSize<u32>,
}

impl VideoFrameTexture {
    pub fn new(device: &wgpu::Device, size: PhysicalSize<u32>) -> Self {
        let tex_y = GpuTexture::new_empty(
            device,
            Some("VideoFrameTexture Y"),
            (size.width, size.height).into(),
            wgpu::TextureFormat::R8Unorm,
            TextureKind::Updatable,
        );
        let tex_uv = GpuTexture::new_empty(
            device,
            Some("VideoFrameTexture UV"),
            (size.width / 2, size.height / 2).into(),
            wgpu::TextureFormat::Rg8Unorm,
            TextureKind::Updatable,
        );

        Self {
            tex_y,
            tex_uv,
            size,
        }
    }

    pub fn write_data_nv12(&self, queue: &wgpu::Queue, nv12: &Nv12Frame) {
        // TODO: maybe support resizing the texture if the frame size changes?
        assert_eq!(self.size, nv12.size);

        let size = self.size;

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: self.tex_y.wgpu_texture(),
                mip_level: 0,
                origin: Default::default(),
                aspect: wgpu::TextureAspect::All,
            },
            &nv12.y_plane,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width),
                rows_per_image: Some(size.height),
            },
            wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: self.tex_uv.wgpu_texture(),
                mip_level: 0,
                origin: Default::default(),
                aspect: Default::default(),
            },
            &nv12.uv_plane,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width),
                rows_per_image: Some(size.height / 2),
            },
            wgpu::Extent3d {
                width: size.width / 2,
                height: size.height / 2,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn get_size(&self) -> Vec2 {
        let PhysicalSize { width, height } = self.size;
        vec2(width as f32, height as f32)
    }

    pub fn get_y_source(&self) -> TextureSource {
        self.tex_y.as_source()
    }

    pub fn get_uv_source(&self) -> TextureSource {
        self.tex_uv.as_source()
    }
    pub fn render_with_options(
        &self,
        pass: &mut RenderPass,
        builder: RenderRequestBuilder,
        transform: Mat4,
        alpha_mode: bool,
        color_multiplier: Vec4,
    ) {
        let tex = self;

        let source_size = tex.get_size();
        let mut size = source_size;
        // Native transparent movie frames contain eight rows between the
        // color and mask regions. Each visible half is doubled for display.
        if alpha_mode {
            size.y = (size.y - 8.0).max(0.0);
        }

        pass.run(builder.build(
            RenderProgramWithArguments::Movie {
                vertices: VertexSource::VertexData {
                    vertices: &build_quad_vertices(|t| PosTexVertex {
                        position: t * size,
                        texture_position: t,
                    }),
                },
                texture_luma: tex.get_y_source(),
                texture_chroma: tex.get_uv_source(),
                transform,
                alpha_mode,
                color_multiplier,
                texture_size: Vec4::new(
                    source_size.x,
                    source_size.y,
                    source_size.x * 0.5,
                    source_size.y * 0.5,
                ),
                color_bias: Vec4::new(0.0625, 0.5, 0.5, 1.1643835),
                color_transform: [
                    Vec4::new(1.1643835, 0.0, 1.7927411, 0.0),
                    Vec4::new(1.1643835, -0.21322097, -0.5328817, 0.0),
                    Vec4::new(1.1643835, 2.1124017, 0.0, 0.0),
                ],
            },
            DrawPrimitive::TrianglesStrip,
        ))
    }
}
