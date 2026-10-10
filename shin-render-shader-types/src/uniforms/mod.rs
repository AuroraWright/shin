pub mod metadata;

use encase::ShaderType;
use glam::{Mat4, Vec4};
use shin_primitives::color::FloatColor4;

pub use crate::uniforms::metadata::UniformType;
use crate::uniforms::metadata::{
    ArraySchema, FieldSchema, PrimitiveType, StructSchema, TypeSchema,
};

macro_rules! impl_primitive {
    ($($ty:ty => $v:expr),*) => {
        $(
            impl UniformType for $ty {
                const SCHEMA: TypeSchema = TypeSchema::Primitive($v);
            }
        )*
    };
}

impl_primitive! {
    f32 => PrimitiveType::Float32,
    Vec4 => PrimitiveType::Float32x4,
    FloatColor4 => PrimitiveType::Float32x4,
    Mat4 => PrimitiveType::Float32x4x4,
    u32 => PrimitiveType::Uint32
}

impl<T, const S: usize> UniformType for [T; S]
where
    T: UniformType,
{
    const SCHEMA: TypeSchema = TypeSchema::Array(ArraySchema {
        ty: &T::SCHEMA,
        length: S as u32,
        stride: T::SCHEMA.size(),
    });
}

#[derive(ShaderType)]
pub struct ClearUniformParams {
    pub color: FloatColor4,
}

impl UniformType for ClearUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "ClearUniformParams",
        size: ClearUniformParams::METADATA.min_size.get() as u32,
        alignment: ClearUniformParams::METADATA.alignment.get() as u32,
        fields: &[FieldSchema {
            name: "color",
            ty: &<Vec4 as UniformType>::SCHEMA,
            offset: ClearUniformParams::METADATA.extra.offsets[0] as u32,
        }],
    });
}

#[derive(ShaderType)]
pub struct FillUniformParams {
    pub transform: Mat4,
}

impl UniformType for FillUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "FillUniformParams",
        size: FillUniformParams::METADATA.min_size.get() as u32,
        alignment: FillUniformParams::METADATA.alignment.get() as u32,
        fields: &[FieldSchema {
            name: "transform",
            ty: &<Mat4 as UniformType>::SCHEMA,
            offset: FillUniformParams::METADATA.extra.offsets[0] as u32,
        }],
    });
}

#[derive(ShaderType)]
pub struct SpriteUniformParams {
    pub transform: Mat4,
}

impl UniformType for SpriteUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "SpriteUniformParams",
        size: SpriteUniformParams::METADATA.min_size.get() as u32,
        alignment: SpriteUniformParams::METADATA.alignment.get() as u32,
        fields: &[FieldSchema {
            name: "transform",
            ty: &<Mat4 as UniformType>::SCHEMA,
            offset: SpriteUniformParams::METADATA.extra.offsets[0] as u32,
        }],
    });
}

#[derive(ShaderType)]
pub struct MosaicUniformParams {
    pub transform: Mat4,
    /// xy: normalized cell width/height; zw: reciprocal width/height.
    pub step: Vec4,
}

impl UniformType for MosaicUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "MosaicUniformParams",
        size: MosaicUniformParams::METADATA.min_size.get() as u32,
        alignment: MosaicUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: MosaicUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "step",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: MosaicUniformParams::METADATA.extra.offsets[1] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct ButtonUniformParams {
    pub transform: Mat4,
    /// Native uniform color flashes; alpha is the RGB interpolation amount.
    pub flash1: Vec4,
    pub flash2: Vec4,
}
impl UniformType for ButtonUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "ButtonUniformParams",
        size: ButtonUniformParams::METADATA.min_size.get() as u32,
        alignment: ButtonUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: ButtonUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "flash1",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: ButtonUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "flash2",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: ButtonUniformParams::METADATA.extra.offsets[2] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct BlendUniformParams {
    pub transform: Mat4,
    /// Per-channel interpolation from texture1 to texture2.
    pub blend: Vec4,
    pub flash: Vec4,
}
impl UniformType for BlendUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "BlendUniformParams",
        size: BlendUniformParams::METADATA.min_size.get() as u32,
        alignment: BlendUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: BlendUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "blend",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: BlendUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "flash",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: BlendUniformParams::METADATA.extra.offsets[2] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct RasterUniformParams {
    pub transform: Mat4,
    /// xy: UV amplitude; zw: spatial frequencies of the crossed axes.
    pub amplitude_frequency: Vec4,
    pub phase: Vec4,
}
impl UniformType for RasterUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "RasterUniformParams",
        size: RasterUniformParams::METADATA.min_size.get() as u32,
        alignment: RasterUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: RasterUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "amplitude_frequency",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: RasterUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "phase",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: RasterUniformParams::METADATA.extra.offsets[2] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct RippleUniformParams {
    pub transform: Mat4,
    /// x: radial frequency; y: UV amplitude; z: retained phase.
    pub wave: Vec4,
}
impl UniformType for RippleUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "RippleUniformParams",
        size: RippleUniformParams::METADATA.min_size.get() as u32,
        alignment: RippleUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: RippleUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "wave",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: RippleUniformParams::METADATA.extra.offsets[1] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct TapRippleUniformParams {
    pub transform: Mat4,
    /// xy: radial center; zw: metric scale.
    pub center_scale: Vec4,
    /// x: inverse width; y: amplitude; z: wave phase; w: transition progress.
    pub wave_params: Vec4,
}
impl UniformType for TapRippleUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "TapRippleUniformParams",
        size: TapRippleUniformParams::METADATA.min_size.get() as u32,
        alignment: TapRippleUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: TapRippleUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "center_scale",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: TapRippleUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "wave_params",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: TapRippleUniformParams::METADATA.extra.offsets[2] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct WiperRippleUniformParams {
    pub transform: Mat4,
    /// xy: radial center; zw: metric scale.
    pub center_scale: Vec4,
    /// x: inverse width; y: amplitude; z: wave phase; w: transition progress.
    pub wave_params: Vec4,
}
impl UniformType for WiperRippleUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "WiperRippleUniformParams",
        size: WiperRippleUniformParams::METADATA.min_size.get() as u32,
        alignment: WiperRippleUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: WiperRippleUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "center_scale",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: WiperRippleUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "wave_params",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: WiperRippleUniformParams::METADATA.extra.offsets[2] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct WiperWaveUniformParams {
    pub transform: Mat4,
    /// xy: radial center; zw: metric scale.
    pub center_scale: Vec4,
    /// x: front radius; y: wave width; z: progress.
    pub front_width: Vec4,
}
impl UniformType for WiperWaveUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "WiperWaveUniformParams",
        size: WiperWaveUniformParams::METADATA.min_size.get() as u32,
        alignment: WiperWaveUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: WiperWaveUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "center_scale",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: WiperWaveUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "front_width",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: WiperWaveUniformParams::METADATA.extra.offsets[2] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct WiperWhirlUniformParams {
    pub transform: Mat4,
    /// x: progress; y: direction; zw: UV metric scale.
    pub effect: Vec4,
}
impl UniformType for WiperWhirlUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "WiperWhirlUniformParams",
        size: WiperWhirlUniformParams::METADATA.min_size.get() as u32,
        alignment: WiperWhirlUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: WiperWhirlUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "effect",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: WiperWhirlUniformParams::METADATA.extra.offsets[1] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct CharacterPortraitUniformParams {
    pub transform: Mat4,
    /// x: display mode (alive0, dead1, missing2, half-dead3).
    pub effect: Vec4,
}
impl UniformType for CharacterPortraitUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "CharacterPortraitUniformParams",
        size: CharacterPortraitUniformParams::METADATA.min_size.get() as u32,
        alignment: CharacterPortraitUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: CharacterPortraitUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "effect",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: CharacterPortraitUniformParams::METADATA.extra.offsets[1] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct BlurUniformParams {
    pub transform: Mat4,
    /// xy: one-radius UV displacement along the active filter axis.
    pub step: Vec4,
}
impl UniformType for BlurUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "BlurUniformParams",
        size: BlurUniformParams::METADATA.min_size.get() as u32,
        alignment: BlurUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: BlurUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "step",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: BlurUniformParams::METADATA.extra.offsets[1] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct ZoomBlurUniformParams {
    pub transform: Mat4,
    pub center: Vec4,
    pub scales_first: Vec4,
    pub scales_last: Vec4,
}
impl UniformType for ZoomBlurUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "ZoomBlurUniformParams",
        size: ZoomBlurUniformParams::METADATA.min_size.get() as u32,
        alignment: ZoomBlurUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: ZoomBlurUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "center",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: ZoomBlurUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "scales_first",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: ZoomBlurUniformParams::METADATA.extra.offsets[2] as u32,
            },
            FieldSchema {
                name: "scales_last",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: ZoomBlurUniformParams::METADATA.extra.offsets[3] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct FontUniformParams {
    pub transform: Mat4,
    pub color1: FloatColor4,
    pub color2: FloatColor4,
}

impl UniformType for FontUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "FontUniformParams",
        size: FontUniformParams::METADATA.min_size.get() as u32,
        alignment: FontUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: FontUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "color1",
                ty: &<FloatColor4 as UniformType>::SCHEMA,
                offset: FontUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "color2",
                ty: &<FloatColor4 as UniformType>::SCHEMA,
                offset: FontUniformParams::METADATA.extra.offsets[2] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct FontBorderUniformParams {
    pub transform: Mat4,
    // can't pass [Vec2; 8] due to alignment requirements,
    // so distances are packed pairwise
    pub dist: [Vec4; 4],
    pub color: FloatColor4,
}

impl UniformType for FontBorderUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "FontBorderUniformParams",
        size: FontBorderUniformParams::METADATA.min_size.get() as u32,
        alignment: FontBorderUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: FontBorderUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "dist",
                ty: &<[Vec4; 4] as UniformType>::SCHEMA,
                offset: FontBorderUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "color",
                ty: &<FloatColor4 as UniformType>::SCHEMA,
                offset: FontBorderUniformParams::METADATA.extra.offsets[2] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct LayerUniformParams {
    pub transform: Mat4,
    pub color: FloatColor4,
    pub fragment_param: Vec4,
    // in reality those are enums, but wgsl doesn't natively support them
    // we can probably be a bit smarter and generate constants for those but that's a paaaaaain
    pub output_type: u32,
    pub fragment_operation: u32,
}

impl UniformType for LayerUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "LayerUniformParams",
        size: LayerUniformParams::METADATA.min_size.get() as u32,
        alignment: LayerUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: LayerUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "color",
                ty: &<FloatColor4 as UniformType>::SCHEMA,
                offset: LayerUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "fragment_param",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: LayerUniformParams::METADATA.extra.offsets[2] as u32,
            },
            FieldSchema {
                name: "output_type",
                ty: &<u32 as UniformType>::SCHEMA,
                offset: LayerUniformParams::METADATA.extra.offsets[3] as u32,
            },
            FieldSchema {
                name: "fragment_operation",
                ty: &<u32 as UniformType>::SCHEMA,
                offset: LayerUniformParams::METADATA.extra.offsets[4] as u32,
            },
        ],
    });
}
#[derive(ShaderType)]
pub struct MaskUniformParams {
    pub transform: Mat4,
    pub color: FloatColor4,
    pub fragment_param: Vec4,
    pub minmax: Vec4,
    // in reality this is an enum, but wgsl doesn't natively support them
    // we can probably be a bit smarter and generate constants for those but that's a paaaaaain
    pub fragment_operation: u32,
}

impl UniformType for MaskUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "MaskUniformParams",
        size: MaskUniformParams::METADATA.min_size.get() as u32,
        alignment: MaskUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: MaskUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "color",
                ty: &<FloatColor4 as UniformType>::SCHEMA,
                offset: MaskUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "fragment_param",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: MaskUniformParams::METADATA.extra.offsets[2] as u32,
            },
            FieldSchema {
                name: "minmax",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: MaskUniformParams::METADATA.extra.offsets[3] as u32,
            },
            FieldSchema {
                name: "fragment_operation",
                ty: &<u32 as UniformType>::SCHEMA,
                offset: MaskUniformParams::METADATA.extra.offsets[4] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct MovieUniformParams {
    pub transform: Mat4,
    pub color_bias: Vec4,
    pub color_transform: [Vec4; 3],
    pub color_multiplier: Vec4,
    pub alpha_mode: u32,
    pub texture_size: Vec4,
}

impl UniformType for MovieUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "MovieUniformParams",
        size: MovieUniformParams::METADATA.min_size.get() as u32,
        alignment: MovieUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: MovieUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "color_bias",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: MovieUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "color_transform",
                ty: &<[Vec4; 3] as UniformType>::SCHEMA,
                offset: MovieUniformParams::METADATA.extra.offsets[2] as u32,
            },
            FieldSchema {
                name: "color_multiplier",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: MovieUniformParams::METADATA.extra.offsets[3] as u32,
            },
            FieldSchema {
                name: "alpha_mode",
                ty: &<u32 as UniformType>::SCHEMA,
                offset: MovieUniformParams::METADATA.extra.offsets[4] as u32,
            },
            FieldSchema {
                name: "texture_size",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: MovieUniformParams::METADATA.extra.offsets[5] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct WiperDefaultUniformParams {
    pub transform: Mat4,
    pub alpha: Vec4,
}

impl UniformType for WiperDefaultUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "WiperDefaultUniformParams",
        size: WiperDefaultUniformParams::METADATA.min_size.get() as u32,
        alignment: WiperDefaultUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: WiperDefaultUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "alpha",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: WiperDefaultUniformParams::METADATA.extra.offsets[1] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct WiperMaskUniformParams {
    pub transform: Mat4,
    pub minmax: Vec4,
}

impl UniformType for WiperMaskUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "WiperMaskUniformParams",
        size: WiperMaskUniformParams::METADATA.min_size.get() as u32,
        alignment: WiperMaskUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: WiperMaskUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "minmax",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: WiperMaskUniformParams::METADATA.extra.offsets[1] as u32,
            },
        ],
    });
}

#[derive(ShaderType)]
pub struct WiperGlassUniformParams {
    pub transform: Mat4,
    pub model: Mat4,
    /// x: prism opacity; y: early white flash.
    pub effect: Vec4,
}
impl UniformType for WiperGlassUniformParams {
    const SCHEMA: TypeSchema = TypeSchema::Struct(StructSchema {
        name: "WiperGlassUniformParams",
        size: WiperGlassUniformParams::METADATA.min_size.get() as u32,
        alignment: WiperGlassUniformParams::METADATA.alignment.get() as u32,
        fields: &[
            FieldSchema {
                name: "transform",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: WiperGlassUniformParams::METADATA.extra.offsets[0] as u32,
            },
            FieldSchema {
                name: "model",
                ty: &<Mat4 as UniformType>::SCHEMA,
                offset: WiperGlassUniformParams::METADATA.extra.offsets[1] as u32,
            },
            FieldSchema {
                name: "effect",
                ty: &<Vec4 as UniformType>::SCHEMA,
                offset: WiperGlassUniformParams::METADATA.extra.offsets[2] as u32,
            },
        ],
    });
}
