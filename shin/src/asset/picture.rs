use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    sync::Arc,
};

use anyhow::Result;
use glam::{Vec2, vec2};
use shin_core::format::picture::{
    PicBlock, PicBlockRect, PictureBuilder, PictureFormat, read_picture_header_prefix_with_format,
};
use shin_render::{
    gpu_texture::GpuTexture,
    shaders::types::{
        buffer::{Buffer, OwnedIndexBuffer, OwnedVertexBuffer},
        vertices::PosTexVertex,
    },
};

use crate::asset::system::{Asset, AssetDataAccessor, AssetLoadContext};

#[cfg(test)]
use shin_core::format::picture::read_picture_header_prefix;

pub struct GpuPictureBlock {
    pub native_format: u16,
    /// Original block+8/+a/+c/+e: signed origins, unsigned texture extents.
    pub native_bounds: [u16; 4],
    pub vertex_buffer: OwnedVertexBuffer<PosTexVertex>,
    pub index_buffer: OwnedIndexBuffer,
    pub opaque_rect_count: usize,
    pub transparent_rect_count: usize,
    pub texture: GpuTexture,
}

impl GpuPictureBlock {
    pub const VERTICES_PER_RECT: usize = 4;
    pub const INDICES_PER_RECT: usize = 6;

    pub fn new(context: GpuTextureBuilderContext, block: PicBlock, label: &str) -> Self {
        let (vertex_buffer, index_buffer) = vertex_data(&block);

        let vertex_buffer = Buffer::allocate_vertex(
            context.wgpu_device,
            &vertex_buffer,
            Some(&format!("{}/vertex", label)),
        );
        let index_buffer = Buffer::allocate_index(
            context.wgpu_device,
            &index_buffer,
            Some(&format!("{}/index", label)),
        );

        let texture = GpuTexture::new_static_from_rgba_image(
            context.wgpu_device,
            context.wgpu_queue,
            Some(label),
            &block.data,
        );

        GpuPictureBlock {
            native_format: block.native_format,
            native_bounds: [
                block.offset_x as u16,
                block.offset_y as u16,
                block.data.width() as u16,
                block.data.height() as u16,
            ],
            vertex_buffer,
            index_buffer,
            opaque_rect_count: block.opaque_rects.len(),
            transparent_rect_count: block.transparent_rects.len(),
            texture,
        }
    }
}

pub(super) fn vertex_data(block: &PicBlock) -> (Vec<PosTexVertex>, Vec<u16>) {
    let rect_count = block.opaque_rects.len() + block.transparent_rects.len();

    let mut vertex_buffer = Vec::with_capacity(rect_count * GpuPictureBlock::VERTICES_PER_RECT);
    let mut index_buffer = Vec::with_capacity(rect_count * GpuPictureBlock::INDICES_PER_RECT);

    // Native block geometry sign-extends both 16-bit offsets and computes each
    // reciprocal once. Dividing every UV separately differs by an f32 ULP.
    let offset_x = block.offset_x as u16 as i16 as f32;
    let offset_y = block.offset_y as u16 as i16 as f32;

    let width = block.data.width() as f32;
    let height = block.data.height() as f32;
    let inverse_width = 1.0 / width;
    let inverse_height = 1.0 / height;

    let mut emit_rect = |PicBlockRect {
                             from_x,
                             from_y,
                             to_x,
                             to_y,
                         }: PicBlockRect| {
        let from_x = from_x as f32;
        let to_x = to_x as f32;
        let from_y = from_y as f32;
        let to_y = to_y as f32;

        let x_left = (offset_x + from_x, inverse_width * (from_x + 1.0));
        let x_right = (
            (to_x + 0.5) + offset_x,
            inverse_width * ((to_x + 0.5) + 1.0),
        );
        let y_top = (offset_y + from_y, inverse_height * (from_y + 1.0));
        let y_bottom = (
            (to_y + 0.5) + offset_y,
            inverse_height * ((to_y + 0.5) + 1.0),
        );

        let vertices: [_; GpuPictureBlock::VERTICES_PER_RECT] = [
            (x_left, y_top),
            (x_right, y_top),
            (x_left, y_bottom),
            (x_right, y_bottom),
        ];

        let index_base = vertex_buffer.len() as u16;
        let indices: [u16; GpuPictureBlock::INDICES_PER_RECT] =
            [0, 1, 2, 3, 2, 1].map(|i| index_base + i);

        vertex_buffer.extend(vertices.map(|((px, tx), (py, ty))| PosTexVertex {
            position: vec2(px, py),
            texture_position: vec2(tx, ty),
        }));
        index_buffer.extend(indices);
    };

    block.opaque_rects.iter().cloned().for_each(&mut emit_rect);
    block
        .transparent_rects
        .iter()
        .cloned()
        .for_each(&mut emit_rect);

    (vertex_buffer, index_buffer)
}

#[cfg(test)]
#[path = "picture_native_load_tests.rs"]
mod native_load_tests;

#[cfg(test)]
#[path = "picture_cache_tests.rs"]
mod cache_tests;

#[cfg(test)]
#[path = "picture_header_tests.rs"]
mod header_tests;

#[derive(Copy, Clone)]
pub struct GpuTextureBuilderContext<'a> {
    pub wgpu_device: &'a wgpu::Device,
    pub wgpu_queue: &'a wgpu::Queue,
}

struct GpuPictureBuilder<'a> {
    context: GpuTextureBuilderContext<'a>,
    label: String,
    blocks: PictureBlocks,
}

impl<'a> PictureBuilder for GpuPictureBuilder<'a> {
    type Args = (GpuTextureBuilderContext<'a>, String);
    type Output = PictureBlocks;

    fn new(
        (context, label): Self::Args,
        _effective_width: u32,
        _effective_height: u32,
        _origin_x: i32,
        _origin_y: i32,
        _picture_id: u32,
    ) -> Self {
        GpuPictureBuilder {
            context,
            label,
            blocks: PictureBlocks {
                blocks: BTreeMap::new(),
                placements: Vec::new(),
            },
        }
    }

    fn add_block(
        &mut self,
        data_offset: u32,
        _positions: Vec<(u32, u32)>,
        block: PicBlock,
    ) -> Result<()> {
        // The native block constructor returns null before decoding/uploading
        // when neither rectangle group has geometry.
        if block.opaque_rects.is_empty() && block.transparent_rects.is_empty() {
            return Ok(());
        }
        let block = GpuPictureBlock::new(
            self.context,
            block,
            &format!("{}/{}", self.label, data_offset),
        );
        assert!(self.blocks.blocks.insert(data_offset, block).is_none());

        Ok(())
    }

    fn set_placements(&mut self, placements: Vec<(u32, (u32, u32))>) {
        self.blocks.placements = placements
            .into_iter()
            .map(|(offset, (x, y))| (offset, vec2(x as f32, y as f32)))
            .collect();
    }

    fn build(self) -> Result<Self::Output> {
        Ok(self.blocks)
    }
}

pub struct PictureBlocks {
    blocks: BTreeMap<u32, GpuPictureBlock>,
    placements: Vec<(u32, Vec2)>,
}

impl PictureBlocks {
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn iter_placements(&self) -> impl Iterator<Item = (u32, Vec2, &GpuPictureBlock)> {
        self.placements.iter().filter_map(|&(offset, position)| {
            self.blocks
                .get(&offset)
                .map(|block| (offset, position, block))
        })
    }
}

/// Per-file metadata and native-ID shared placement/GPU block ownership.
pub struct Picture {
    pub label: String,
    pub effective_width: u32,
    pub effective_height: u32,
    pub origin_x: i32,
    pub origin_y: i32,
    // Preserve native file metadata for header validation and future flag handling.
    #[allow(dead_code)]
    pub flags: u32,
    pub scale: f32,
    pub blocks: Arc<PictureBlocks>,
}

impl Asset for Picture {
    type Args = ();

    async fn load(
        context: &Arc<AssetLoadContext>,
        _args: (),
        name: &str,
        data: AssetDataAccessor,
    ) -> Result<Self> {
        let label = format!("Pic[{}]", name);
        let format = match context.game {
            crate::cli::Game::Umineko => PictureFormat::Umineko,
            crate::cli::Game::Higurashi => PictureFormat::HigurashiV2,
        };
        let mut cursor = data.cursor()?;
        let size = cursor.seek(SeekFrom::End(0))?;
        cursor.seek(SeekFrom::Start(0))?;
        let mut prefix = vec![
            0;
            if format == PictureFormat::HigurashiV2 {
                32
            } else {
                36
            }
        ];
        cursor.read_exact(&mut prefix)?;
        let header = read_picture_header_prefix_with_format(&prefix, size, format)?;
        let load_label = label.clone();
        let load_context = context.clone();
        let blocks = context
            .picture_cache
            .get_or_load(header.picture_id, || async move {
                // Cache hits need only the fixed header, as in the native loader.
                // Decode/upload runs on compute workers without blocking awaiters.
                shin_tasks::compute::spawn(move || {
                    crate::asset::system::catch_asset_preparation_panic("Picture", || {
                        let mut data = prefix.to_vec();
                        cursor.read_to_end(&mut data)?;
                        shin_core::format::picture::read_picture_with_format::<GpuPictureBuilder>(
                            &data,
                            (
                                GpuTextureBuilderContext {
                                    wgpu_device: &load_context.wgpu_device,
                                    wgpu_queue: &load_context.wgpu_queue,
                                },
                                load_label,
                            ),
                            format,
                        )
                    })
                })
                .await
            })
            .await?;
        Ok(Self {
            label,
            effective_width: header.effective_width.into(),
            effective_height: header.effective_height.into(),
            origin_x: header.origin_x.into(),
            origin_y: header.origin_y.into(),
            flags: header.flags,
            scale: header.scale as f32 * (1.0 / 4096.0),
            blocks,
        })
    }
}
