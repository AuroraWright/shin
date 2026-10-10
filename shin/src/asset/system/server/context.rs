use shin_core::format::bustup::BustupBlockId;

use crate::asset::{
    font::FontGlyphCache,
    picture::{GpuPictureBlock, PictureBlocks},
    system::{cache::AssetCache, resource_cache::ResourceCache},
};

pub struct AssetLoadContext {
    pub wgpu_device: wgpu::Device,
    pub wgpu_queue: wgpu::Queue,
    pub bustup_cache: AssetCache<BustupBlockId, GpuPictureBlock>,
    pub picture_cache: ResourceCache<PictureBlocks>,
    pub font_cache: std::sync::Arc<FontGlyphCache>,
}
