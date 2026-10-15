use shin_core::format::bustup::BustupBlockId;

use crate::asset::{
    bustup::higurashi::HigurashiBustupCache,
    font::FontGlyphCache,
    picture::{GpuPictureBlock, PictureBlocks},
    system::{cache::AssetCache, resource_cache::ResourceCache},
};

pub struct AssetLoadContext {
    /// Immutable for this server: game-specific cache IDs cannot cross games.
    pub game: crate::cli::Game,
    pub wgpu_device: wgpu::Device,
    pub wgpu_queue: wgpu::Queue,
    pub bustup_cache: AssetCache<BustupBlockId, GpuPictureBlock>,
    pub higurashi_bustup_cache: HigurashiBustupCache,
    pub picture_cache: ResourceCache<PictureBlocks>,
    pub font_cache: std::sync::Arc<FontGlyphCache>,
}
