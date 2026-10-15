//! Saved fresh OtsuGet13/KakeraGet14 clones. Their virtual40 refresh is a return;
//! clocks remain at constructor values, including the final black overlay.
use std::sync::Arc;

use crate::{
    adv::character_view::CharacterTextures,
    asset::system::AssetServer,
    layer::{
        higurashi_properties::HigurashiProperties,
        render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
    },
};
use anyhow::{Result, ensure};
use glam::{Mat4, Vec4, vec2, vec3, vec4};
use shin_core::{format::scenario::higurashi::HigurashiScenario, primitives::color::UnormColor};
use shin_render::{
    ColorBlendType, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, PosColVertex},
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Content {
    Otsu { argument: i32, append: bool },
    Kakera { ids: Vec<i32> },
}
impl Content {
    fn from_params(kind: i32, params: &[i32; 8], append: bool) -> Result<Self> {
        match kind {
            13 => Ok(Self::Otsu {
                argument: params[0],
                append,
            }),
            14 => {
                //20eac..20eb4 skips the copy loop for any signed count<1.
                let count = params[0].max(0) as usize;
                ensure!(
                    count <= 7,
                    "Saved KakeraGet exceeds seven compact parameter IDs"
                );
                Ok(Self::Kakera {
                    ids: params[1..1 + count].to_vec(),
                })
            }
            _ => anyhow::bail!("Saved notification type{kind} is outside13/14"),
        }
    }
    fn archive(&self) -> &'static str {
        match self {
            Self::Otsu { .. } => "/otsuget.txa",
            Self::Kakera { .. } => "/kakeraget.txa",
        }
    }
    fn packets(&self, kind: PassKind) -> Vec<Packet> {
        let full = vec4(0., 0., 1920., 1080.);
        if kind == PassKind::Opaque {
            return vec![Packet {
                texture: None,
                rect: full,
                uv: Vec4::ZERO,
                color: UnormColor::BLACK,
                opaque: true,
            }];
        }
        let uv = vec4(0., 0., 1., 1.);
        let mut out = Vec::new();
        match self {
            Self::Otsu { argument, append } => {
                let mut name = if *argument <= 6 {
                    format!("otsuget_{argument:03}")
                } else if *argument <= 31 {
                    format!("snrget_{:03}", argument - 7)
                } else {
                    format!("snrget_{:03}", argument.wrapping_add(18))
                };
                if (27..=31).contains(argument) && *append {
                    name.push('_');
                }
                //97f14 ctor: slide1498=-120, unused14e8=0, black1538=1.
                out.push(Packet::sprite(
                    name,
                    vec4(450., 264., 1470., 504.),
                    uv,
                    UnormColor::WHITE,
                ));
            }
            Self::Kakera { ids } => {
                //5f3ec ctor: black14a0=1,header14f0=100,each rowalpha=0.
                out.push(Packet::sprite(
                    "kakera_get",
                    vec4(624., 100., 1296., 300.),
                    uv,
                    UnormColor::WHITE,
                ));
                //60664..606c8 computes -96*count in wrapping u64,adds576,
                //logically shifts right,then UCVTF +450. Preserve count7's
                //native unsigned wrap instead of inferring a UI row clamp.
                let half = 576u64.wrapping_sub((ids.len() as u64).wrapping_mul(96)) >> 1;
                let mut y = half as f32 + 450.;
                let transparent = UnormColor::from_rgba(255, 255, 255, 0);
                for id in ids {
                    out.push(Packet::sprite(
                        "plate",
                        vec4(576., y + 3., 1344., (y + 3.) + 96.),
                        uv,
                        transparent,
                    ));
                    let top = y + 18.;
                    //The native signed division/remainder truncate toward0.
                    let band = (id % 32) as f32 * f32::from_bits(0x3d000000);
                    out.push(Packet::sprite(
                        format!("title_{:03}", id / 32),
                        vec4(696., top, 1224., top + 64.),
                        vec4(0., band, 1., band + f32::from_bits(0x3d000000)),
                        transparent,
                    ));
                    y += 96.;
                }
            }
        }
        out.push(Packet {
            texture: None,
            rect: full,
            uv: Vec4::ZERO,
            color: UnormColor::BLACK,
            opaque: false,
        });
        out
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Prepared {
    atlas: Arc<CharacterTextures>,
    content: Content,
}
impl Prepared {
    pub async fn load(
        server: &AssetServer,
        scenario: &HigurashiScenario,
        kind: i32,
        params: &[i32; 8],
    ) -> Result<Self> {
        Self::load_with_append(
            server,
            scenario,
            kind,
            params,
            server.contains("/append.txt"),
        )
        .await
    }
    /// The original57e40 append-presence read is an explicit test/service input.
    pub(crate) async fn load_with_append(
        server: &AssetServer,
        _scenario: &HigurashiScenario,
        kind: i32,
        params: &[i32; 8],
        append: bool,
    ) -> Result<Self> {
        let content = Content::from_params(kind, params, append)?;
        let atlas: Arc<CharacterTextures> = server.load(content.archive()).await?;
        for packet in content.packets(PassKind::Transparent) {
            if let Some(name) = packet.texture {
                ensure!(
                    atlas.contains(&name),
                    "Saved notification native {} has no texture{name}",
                    content.archive()
                );
            }
        }
        Ok(Self { atlas, content })
    }
    /// All factories have completed before the enclosing scene calls this in
    /// physical-slot order. Base four OS seeds/65targets/ID belong to that owner.
    /// Native989d0/603fc return without clock/global/entropy changes.
    pub fn clone_refresh(&self) -> Result<View> {
        Ok(View {
            atlas: self.atlas.clone(),
            content: self.content.clone(),
        })
    }
}
#[derive(Clone, Debug)]
pub(crate) struct View {
    atlas: Arc<CharacterTextures>,
    content: Content,
}
#[derive(Clone, Debug)]
struct Packet {
    texture: Option<String>,
    rect: Vec4,
    uv: Vec4,
    color: UnormColor,
    opaque: bool,
}
impl Packet {
    fn sprite(name: impl Into<String>, rect: Vec4, uv: Vec4, color: UnormColor) -> Self {
        Self {
            texture: Some(name.into()),
            rect,
            uv,
            color,
            opaque: false,
        }
    }
    fn fills(&self) -> [PosColVertex; 4] {
        build_quad_vertices(|c| PosColVertex {
            position: vec3(
                if c.x == 0. { self.rect.x } else { self.rect.z },
                if c.y == 0. { self.rect.y } else { self.rect.w },
                0.,
            ),
            color: self.color,
        })
    }
    fn sprites(&self) -> [PosColTexVertex; 4] {
        build_quad_vertices(|c| PosColTexVertex {
            position: vec3(
                if c.x == 0. { self.rect.x } else { self.rect.z },
                if c.y == 0. { self.rect.y } else { self.rect.w },
                0.,
            ),
            color: self.color,
            texture_position: vec2(
                if c.x == 0. { self.uv.x } else { self.uv.z },
                if c.y == 0. { self.uv.y } else { self.uv.w },
            ),
        })
    }
}
impl View {
    fn matrix(properties: &HigurashiProperties, parent: &HigurashiTransform) -> Mat4 {
        let native = properties
            .composed_transform(parent)
            .compute_native_final_transform();
        multiply(
            multiply(native, Mat4::from_translation(vec3(-640., -720., 0.))),
            Mat4::from_scale(vec3(2. / 3., 2. / 3., 0.)),
        )
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        properties: &HigurashiProperties,
        parent: &HigurashiTransform,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        //98a14/60474 directly emit fixed notification colors,including their
        //black overlay with propertyalpha0. Generic leaf opacity/tint/shader
        //multiplication or a visibility predicate would change this widget.
        let transform = HigurashiTransform::convert_clip_depth(Self::matrix(properties, parent));
        for packet in self.content.packets(kind) {
            let builder = RenderRequestBuilder::new()
                .depth_stencil_shorthand(stencil, true, false)
                .color_blend_type(if packet.opaque {
                    ColorBlendType::Opaque
                } else {
                    ColorBlendType::Layer1
                });
            if let Some(name) = &packet.texture {
                let vertices = packet.sprites();
                pass.run(builder.build(
                    RenderProgramWithArguments::Sprite {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        sprite: self.atlas.get(name).as_source(),
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            } else {
                let vertices = packet.fills();
                pass.run(builder.build(
                    RenderProgramWithArguments::Fill {
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        transform,
                    },
                    DrawPrimitive::TrianglesStrip,
                ));
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests;
