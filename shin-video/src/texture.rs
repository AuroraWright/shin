use dpi::PhysicalSize;
use glam::{Mat4, Vec2, Vec4, vec2};
use shin_render::{
    DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    gpu_texture::{GpuTexture, TextureKind},
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::texture::TextureSource,
    shaders::types::{buffer::VertexSource, vertices::PosTexVertex},
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
        Self::new_checked(device, size).expect("Invalid movie texture extent")
    }
    pub(crate) fn new_checked(
        device: &wgpu::Device,
        size: PhysicalSize<u32>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            size.width >= 2 && size.height >= 2 && size.width % 2 == 0 && size.height % 2 == 0,
            "Invalid NV12 movie extent {}x{}",
            size.width,
            size.height
        );
        let limit = device.limits().max_texture_dimension_2d;
        anyhow::ensure!(
            size.width <= limit && size.height <= limit,
            "Movie extent {}x{} exceeds device texture limit {limit}",
            size.width,
            size.height
        );

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

        Ok(Self {
            tex_y,
            tex_uv,
            size,
        })
    }

    pub(crate) fn validate_frame(&self, frame: &Nv12Frame) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.size == frame.size,
            "Movie frame extent changed from {}x{} to {}x{}",
            self.size.width,
            self.size.height,
            frame.size.width,
            frame.size.height
        );
        let pixels = (self.size.width as usize)
            .checked_mul(self.size.height as usize)
            .ok_or_else(|| anyhow::anyhow!("Movie NV12 plane size overflow"))?;
        anyhow::ensure!(
            frame.y_plane.len() == pixels && frame.uv_plane.len() == pixels / 2,
            "Invalid movie NV12 plane lengths: Y={},UV={},expected Y={pixels},UV={}",
            frame.y_plane.len(),
            frame.uv_plane.len(),
            pixels / 2
        );
        Ok(())
    }
    pub fn write_data_nv12(&self, queue: &wgpu::Queue, nv12: &Nv12Frame) {
        self.write_data_nv12_checked(queue, nv12)
            .expect("Invalid movie frame upload");
    }
    pub(crate) fn write_data_nv12_checked(
        &self,
        queue: &wgpu::Queue,
        nv12: &Nv12Frame,
    ) -> anyhow::Result<()> {
        self.validate_frame(nv12)?;
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
        Ok(())
    }

    pub fn get_size(&self) -> Vec2 {
        let PhysicalSize { width, height } = self.size;
        vec2(width as f32, height as f32)
    }

    pub fn get_y_source(&self) -> TextureSource<'_> {
        self.tex_y.as_source()
    }

    pub fn get_uv_source(&self) -> TextureSource<'_> {
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
