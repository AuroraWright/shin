use enum_iterator::Sequence;

#[derive(Debug)]
pub struct TextureSamplerStore {
    pub linear: wgpu::Sampler,
    pub linear_mirror_u: wgpu::Sampler,
    pub higurashi_mosaic: wgpu::Sampler,
}

impl TextureSamplerStore {
    pub fn new(device: &wgpu::Device) -> Self {
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Linear Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let linear_mirror_u = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Linear Mirror U Sampler"),
            address_mode_u: wgpu::AddressMode::MirrorRepeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        // Updated8b518 clears both min/mag words; actuala7d40 selects NVN
        // min4(NearestMipmapLinear),mag0(Nearest),wrap7(ClampToEdge).
        let higurashi_mosaic = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Higurashi Mosaic Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        Self {
            linear,
            linear_mirror_u,
            higurashi_mosaic,
        }
    }

    pub fn get(&self, sampler: TextureSampler) -> &wgpu::Sampler {
        match sampler {
            TextureSampler::Linear => &self.linear,
            TextureSampler::LinearMirrorU => &self.linear_mirror_u,
            TextureSampler::HigurashiMosaic => &self.higurashi_mosaic,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum TextureSampler {
    Linear,
    LinearMirrorU,
    HigurashiMosaic,
}

#[derive(Debug, Copy, Clone)]
pub struct TextureSource<'a> {
    pub view: &'a wgpu::TextureView,
    pub sampler: TextureSampler,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Sequence)]
pub enum TextureTargetKind {
    Screen,
    RenderTexture,
}

#[derive(Debug, Copy, Clone)]
pub struct TextureTarget<'a> {
    pub kind: TextureTargetKind,
    pub view: &'a wgpu::TextureView,
}

#[derive(Debug, Copy, Clone)]
pub struct DepthStencilTarget<'a> {
    pub view: &'a wgpu::TextureView,
}
