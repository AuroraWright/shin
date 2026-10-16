//! Updated d3710 temporary clone/refresh: CharSelectLayer11 and TipsGetLayer12.
//! Resource preparation is side-effect free. Acquisition belongs to clone_refresh.
use std::sync::Arc;

use anyhow::{Result, ensure};
use glam::{Mat4, Vec4, vec2, vec3, vec4};
use shin_core::{
    format::scenario::higurashi::HigurashiScenario,
    primitives::color::{FloatColor4, UnormColor},
};
use shin_render::{
    ColorBlendType, DrawPrimitive, PassKind, RenderProgramWithArguments, RenderRequestBuilder,
    quad_vertices::build_quad_vertices,
    render_pass::RenderPass,
    shaders::types::{
        buffer::VertexSource,
        vertices::{PosColTexVertex, PosColVertex},
    },
};

use crate::{
    adv::{
        characters::view::CharacterTextures,
        higurashi::{
            character_selection::view::View as CharacterView, message_ui::HigurashiMessageTextures,
        },
    },
    asset::system::AssetServer,
    layer::{
        higurashi_properties::HigurashiProperties,
        render_params::{higurashi::HigurashiTransform, multiply_transforms as multiply},
    },
};

/// Native a9fe0 reads settings+b8 at2*ID; a9f2c sets that bit only if missing.
/// The caller owns the synchronized live settings bank and its native growth.
pub(crate) trait AcquiredBits {
    fn acquired(&self, source_index: usize) -> bool;
    fn acquire(&mut self, source_index: usize) -> Result<()>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Descriptor {
    pub source_index: usize,
    /// Original SNR first u16: upper bits select title_%03d,low5bits its UV row.
    pub atlas_word: u16,
}

#[derive(Clone, Debug)]
pub(crate) enum Prepared {
    Character {
        view: CharacterView,
    },
    Tips {
        atlas: Arc<CharacterTextures>,
        rows: Vec<Descriptor>,
        alternate: bool,
    },
}
impl Prepared {
    /// Original20df0/70ff0/71304 prepares only the requested atlas/metadata.
    /// No globals,seed source,opening task,input wrapper or GUI clock is used.
    pub async fn load(
        server: &AssetServer,
        scenario: &HigurashiScenario,
        kind: i32,
        params: &[i32; 8],
        route_navigation: bool,
    ) -> Result<Self> {
        match kind {
            11 => {
                let message = HigurashiMessageTextures::load(server).await?;
                let mut view = CharacterView::load(server, message, params[0] as u32).await?;
                view.route_navigation = route_navigation;
                Ok(Self::Character { view })
            }
            12 => {
                let count = usize::try_from(params[1])?;
                ensure!(
                    count <= 6,
                    "Saved TipsGetLayer exceeds six compact parameter IDs"
                );
                let rows = params[2..2 + count]
                    .iter()
                    .map(|&id| {
                        let source_index = usize::try_from(id)?;
                        let info = scenario.tips_info.get(source_index).ok_or_else(|| {
                            anyhow::anyhow!(
                                "Saved TipsGetLayer ID outside original scenario catalog"
                            )
                        })?;
                        Ok(Descriptor {
                            source_index,
                            atlas_word: info.id,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let atlas = server.load("/tipsget.txa").await?;
                Ok(Self::Tips {
                    atlas,
                    rows,
                    alternate: params[0] != 0,
                })
            }
            _ => anyhow::bail!("Saved temporary native layer type{kind} is outside11/12"),
        }
    }

    /// Called in ascending compact-slot order AFTER all factories are ready.
    /// The caller creates four fresh property seeds/restores65targets/ownerID
    /// around this clone. Index/visibility gates must not suppress this side effect.
    pub fn clone_refresh(&self, acquired: &mut impl AcquiredBits) -> Result<View> {
        match self {
            Self::Character { view } => {
                let view = view.clone();
                {
                    let mut model = view.model.lock();
                    for row in &mut model.rows {
                        row.alpha.fast_forward_to(1.0);
                    }
                    //43fec skipsclock5,which remains its constructor1.
                    for (i, value) in [
                        (0, 0.),
                        (1, 1.),
                        (2, 0.),
                        (3, 1.),
                        (4, 1.),
                        (6, 0.),
                        (7, 1.),
                    ] {
                        model.clocks[i].fast_forward_to(value);
                    }
                }
                Ok(View::Character(view))
            }
            Self::Tips {
                atlas,
                rows,
                alternate,
            } => {
                let rows = rows
                    .iter()
                    .map(|descriptor| {
                        let previously_acquired = acquired.acquired(descriptor.source_index);
                        if !previously_acquired {
                            acquired.acquire(descriptor.source_index)?;
                        }
                        Ok(TipRow {
                            descriptor: descriptor.clone(),
                            previously_acquired,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                //d7d50 leaves phase0 and sets blackopacity0/headeroffset160,
                //each rowalpha1. This retained preview never advances them.
                Ok(View::Tips(TipView {
                    atlas: atlas.clone(),
                    rows,
                    alternate: *alternate,
                }))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TipRow {
    pub descriptor: Descriptor,
    pub previously_acquired: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct TipView {
    atlas: Arc<CharacterTextures>,
    pub rows: Vec<TipRow>,
    #[allow(dead_code, reason = "Preserve the native temporary-tip atlas selector for inspection.")]
    pub alternate: bool,
}
#[derive(Clone, Debug)]
pub(crate) enum View {
    Character(CharacterView),
    Tips(TipView),
}
impl View {
    pub fn render(
        &self,
        pass: &mut RenderPass,
        properties: &HigurashiProperties,
        parent: &HigurashiTransform,
        stencil: u8,
        kind: PassKind,
    ) -> Result<()> {
        match self {
            Self::Character(view) => view.render(pass, properties, parent, stencil, kind),
            Self::Tips(view) => view.render(pass, properties, parent, stencil, kind),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Packet {
    Fill {
        rect: Vec4,
        color: UnormColor,
        opaque: bool,
    },
    Sprite {
        texture: String,
        rect: Vec4,
        uv: Vec4,
    },
}
impl Packet {
    pub fn fill_vertices(&self) -> [PosColVertex; 4] {
        let Self::Fill { rect, color, .. } = self else {
            panic!("TipsGetLayer fill packet")
        };
        build_quad_vertices(|c| PosColVertex {
            position: vec3(
                if c.x == 0. { rect.x } else { rect.z },
                if c.y == 0. { rect.y } else { rect.w },
                0.,
            ),
            color: *color,
        })
    }
    pub fn sprite_vertices(&self) -> [PosColTexVertex; 4] {
        let Self::Sprite { rect, uv, .. } = self else {
            panic!("TipsGetLayer sprite packet")
        };
        build_quad_vertices(|c| PosColTexVertex {
            position: vec3(
                if c.x == 0. { rect.x } else { rect.z },
                if c.y == 0. { rect.y } else { rect.w },
                0.,
            ),
            color: FloatColor4::WHITE.into_unorm(),
            texture_position: vec2(
                if c.x == 0. { uv.x } else { uv.z },
                if c.y == 0. { uv.y } else { uv.w },
            ),
        })
    }
}

impl TipView {
    pub fn reference_packets(&self, kind: PassKind) -> Vec<Packet> {
        let full = vec4(0., 0., 1920., 1080.);
        if kind == PassKind::Opaque {
            return vec![Packet::Fill {
                rect: full,
                color: UnormColor::BLACK,
                opaque: true,
            }];
        }
        let uv = vec4(0., 0., 1., 1.);
        let mut packets = vec![Packet::Sprite {
            texture: "tips_get".into(),
            rect: vec4(624., 160., 1296., 360.),
            uv,
        }];
        let first_y = 738.0 - 48.0 * self.rows.len() as f32;
        for (i, row) in self.rows.iter().enumerate() {
            //Native first_y is the row anchor;plate is anchor+3,title+18,new+30.
            let y = first_y + 96.0 * i as f32;
            packets.push(Packet::Sprite {
                texture: "plate".into(),
                rect: vec4(576., y + 3., 1344., y + 99.),
                uv,
            });
            let band = f32::from(row.descriptor.atlas_word & 31) * f32::from_bits(0x3d000000);
            packets.push(Packet::Sprite {
                texture: format!("title_{:03}", row.descriptor.atlas_word >> 5),
                rect: vec4(696., y + 18., 1224., y + 82.),
                uv: vec4(0., band, 1., band + f32::from_bits(0x3d000000)),
            });
            if !row.previously_acquired {
                packets.push(Packet::Sprite {
                    texture: "new".into(),
                    rect: vec4(576., y + 30., 684., y + 66.),
                    uv,
                });
            }
        }
        packets.push(Packet::Fill {
            rect: full,
            color: UnormColor::from_rgba(0, 0, 0, 0),
            opaque: false,
        });
        packets
    }

    pub(crate) fn matrix(properties: &HigurashiProperties, parent: &HigurashiTransform) -> Mat4 {
        let native = properties
            .composed_transform(parent)
            .compute_native_final_transform();
        multiply(
            multiply(native, Mat4::from_translation(vec3(-640., -720., 0.))),
            Mat4::from_scale(vec3(2.0 / 3.0, 2.0 / 3.0, 0.)),
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
        if !properties.is_visible() {
            return Ok(());
        }
        let transform = HigurashiTransform::convert_clip_depth(Self::matrix(properties, parent));
        for packet in self.reference_packets(kind) {
            let builder = RenderRequestBuilder::new().depth_stencil_shorthand(stencil, true, false);
            match &packet {
                Packet::Fill { opaque, .. } => {
                    let vertices = packet.fill_vertices();
                    pass.run(
                        builder
                            .color_blend_type(if *opaque {
                                ColorBlendType::Opaque
                            } else {
                                ColorBlendType::Layer1
                            })
                            .build(
                                RenderProgramWithArguments::Fill {
                                    vertices: VertexSource::VertexData {
                                        vertices: &vertices,
                                    },
                                    transform,
                                },
                                DrawPrimitive::TrianglesStrip,
                            ),
                    );
                }
                Packet::Sprite { texture, .. } => {
                    let vertices = packet.sprite_vertices();
                    pass.run(builder.color_blend_type(ColorBlendType::Layer1).build(
                        RenderProgramWithArguments::Sprite {
                            vertices: VertexSource::VertexData {
                                vertices: &vertices,
                            },
                            sprite: self.atlas.get(texture).as_source(),
                            transform,
                        },
                        DrawPrimitive::TrianglesStrip,
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests;
