use super::texture_archive::TextureArchive;
use shin_render::gpu_texture::GpuTexture;

#[derive(TextureArchive)]
pub struct EffectTextures {
    #[txa(name = "rain")]
    pub rain: GpuTexture,
    #[txa(name = "hanabira")]
    pub petals: GpuTexture,
    #[txa(name = "snow")]
    pub snow: GpuTexture,
    #[txa(name = "goldsnow")]
    pub gold: GpuTexture,
    #[txa(name = "focusline")]
    pub _focusline: GpuTexture,
    #[txa(name = "breakup")]
    pub breakup: GpuTexture,
}
