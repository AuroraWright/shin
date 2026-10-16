//! SceneJump pane and right-anchored SimpleText labels (native a9770..aa56c).
use crate::adv::{
    backlog::{Backlog, BacklogItem},
    backlog::jump::{Kind, Marker, ROW_HEIGHT, SceneJump, VIEW_HEIGHT},
    backlog::view::BacklogTextures,
    dialog::view::DialogText,
};
use crate::{
    asset::{font::GpuFontLazy, system::AssetServer},
    layer::render_params::{DrawableClipMode, DrawableClipParams, TransformParams},
    render::top_left_projection_matrix,
};
use anyhow::{Result, bail};
use glam::{Mat4, Vec4, vec3, vec4};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DepthStencilState, DrawPrimitive, RenderProgramWithArguments,
    RenderRequestBuilder, StencilFunction, StencilMask, StencilPipelineState, StencilState,
    quad_vertices::QuadVertices,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, vertices::PosColVertex},
};
use std::{collections::BTreeMap, sync::Arc};

fn label(history: &Backlog, marker: &Marker) -> Result<(String, [f32; 3])> {
    if let Some(text) = marker.kind.label() {
        let color = if matches!(marker.kind, Kind::Beginning | Kind::End) {
            [0.875, 0.875, 1.0]
        } else {
            [0.75, 1.0, 0.75]
        };
        return Ok((text.into(), color));
    }
    let Some(row) = marker.authored_row.and_then(|i| history.entries.get(i)) else {
        bail!("Missing scene jump checkpoint")
    };
    Ok(match &row.item {
        BacklogItem::Choice { title, .. } => (title.clone(), [1.0, 0.75, 0.75]),
        BacklogItem::Checkpoint { text, .. } => (text.clone(), [0.75, 1.0, 1.0]),
        BacklogItem::Quiz { mode, .. } => (
            match mode {
                Some(0) => "クイズ",
                Some(1) => "ケーキ選択",
                Some(2) => "犯人当て",
                Some(3) => "最後の選択",
                _ => bail!("Unknown scene jump quiz subtype"),
            }
            .into(),
            [1.0, 0.7777778, 0.7777778],
        ),
        BacklogItem::Message { .. } => bail!("Message used as scene jump checkpoint"),
    })
}

/// aa4b8..aa518 uses integer arithmetic, including a separately rounded bottom.
fn scrollbar(count: usize, scroll: f32) -> Option<Vec4> {
    let height = count as i64 * 60;
    if height <= 720 {
        return None;
    }
    let numerator = scroll as i64 * 720;
    Some(vec4(
        1808.0,
        240.0 + (numerator / height) as f32,
        1824.0,
        240.0 + ((numerator + 518400) / height) as f32,
    ))
}
fn fill(pass: &mut RenderPass, rect: Vec4, colors: [FloatColor4; 4], transform: Mat4) {
    let positions = [
        (rect.x, rect.y),
        (rect.z, rect.y),
        (rect.x, rect.w),
        (rect.z, rect.w),
    ];
    let vertices = std::array::from_fn::<_, 4, _>(|i| PosColVertex {
        position: vec3(positions[i].0, positions[i].1, 0.0),
        color: colors[i].into_unorm(),
    });
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
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
pub(crate) struct SceneJumpView {
    font: Arc<GpuFontLazy>,
    labels: Vec<(String, [f32; 3])>,
    cached: BTreeMap<usize, DialogText>,
}
impl SceneJumpView {
    pub fn load(server: &AssetServer, history: &Backlog, model: &SceneJump) -> Result<Self> {
        Ok(Self {
            font: server.load_sync::<GpuFontLazy>("/system.fnt")?,
            labels: model
                .markers
                .iter()
                .map(|m| label(history, m))
                .collect::<Result<_>>()?,
            cached: BTreeMap::new(),
        })
    }
    pub fn prepare(&mut self, server: &AssetServer, model: &SceneJump) {
        let visible = |i: usize| {
            let y = i as f32 * ROW_HEIGHT - model.scroll();
            y < VIEW_HEIGHT && y + ROW_HEIGHT > 0.0
        };
        self.cached.retain(|&i, _| visible(i));
        for (i, (text, _)) in self.labels.iter().enumerate().filter(|(i, _)| visible(*i)) {
            self.cached.entry(i).or_insert_with(|| {
                DialogText::load_with_width(server, &self.font, text, 48.0, 1024.0)
            });
        }
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        textures: &BacklogTextures,
        model: &SceneJump,
        progress: f32,
    ) {
        let transform = top_left_projection_matrix()
            * Mat4::from_translation(vec3(1920.0 * (1.0 - progress), 0.0, 0.0));
        let pane =
            FloatColor4::from_rgba(128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0);
        let transparent = FloatColor4::from_rgba(128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 0.0);
        fill(pass, vec4(420.0, 0.0, 1920.0, 1080.0), [pane; 4], transform);
        fill(
            pass,
            vec4(0.0, 0.0, 420.0, 1080.0),
            [transparent, pane, transparent, pane],
            transform,
        );
        let clip = DrawableClipParams {
            mode: DrawableClipMode::Clip,
            area: vec4(0.0, 240.0, 1920.0, 720.0),
        };
        clip.begin(
            pass,
            &TransformParams {
                transform: crate::render::centered_projection_matrix().inverse() * transform,
                ..Default::default()
            },
        );
        let builder = RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::Layer1)
            .depth_stencil(DepthStencilState {
                stencil: StencilState {
                    stencil_reference: 0,
                    pipeline: StencilPipelineState {
                        function: StencilFunction::Equal,
                        stencil_read_mask: StencilMask::ClipOnly,
                        ..Default::default()
                    },
                },
                ..Default::default()
            });
        for (&i, text) in &self.cached {
            let [r, g, b] = self.labels[i].1;
            let multiplier = if i == model.selected { 1.0 } else { 0.7 };
            text.render_with_builder(
                pass,
                transform
                    * Mat4::from_translation(vec3(
                        1800.0 - text.size.x,
                        243.0 + i as f32 * 60.0 - model.scroll(),
                        0.0,
                    )),
                FloatColor4::from_rgba(r * multiplier, g * multiplier, b * multiplier, 1.0),
                FloatColor4::from_rgba(0.0, 0.0, 0.0, 240.0 / 255.0),
                builder,
            );
        }
        clip.end(pass);
        QuadVertices::new()
            .with_box(1152.0, 144.0, 1800.0, 240.0)
            .render_sprite(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                textures.chapter.as_source(),
                transform,
            );
        let clearwhite = FloatColor4::from_rgba(1.0, 1.0, 1.0, 0.0);
        // Both rules fade horizontally from transparent at x420 to white at1920.
        for rect in [
            vec4(420.0, 234.0, 1920.0, 240.0),
            vec4(420.0, 960.0, 1920.0, 966.0),
        ] {
            fill(
                pass,
                rect,
                [
                    clearwhite,
                    FloatColor4::WHITE,
                    clearwhite,
                    FloatColor4::WHITE,
                ],
                transform,
            );
        }
        if let Some(thumb) = scrollbar(model.markers.len(), model.scroll()) {
            fill(
                pass,
                vec4(1808.0, 240.0, 1824.0, 960.0),
                [FloatColor4::from_rgba(16.0 / 255.0, 16.0 / 255.0, 64.0 / 255.0, 96.0 / 255.0); 4],
                transform,
            );
            fill(
                pass,
                thumb,
                [FloatColor4::from_rgba(192.0 / 255.0, 192.0 / 255.0, 192.0 / 255.0, 192.0 / 255.0);
                    4],
                transform,
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM; no GPU required"]
    fn reference_native_scene_jump_pane_simple_text_and_close_geometry() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        for (a, word) in [
            (0xa9dac, 0xb26003e8u32), // outlined SimpleText style1
            (0xa9db4, 0xd2a89008),
            (0xa9db8, 0xf2e84808), // width1024, size48
            (0xa9e5c, 0x94015cb9),
            (0xa9e84, 0x94015cab), // right anchor, row position
            (0xaa154, 0x52901001),
            (0xaa158, 0x72b81001), // pane ARGB c0808080
            (0xaa184, 0x52901008),
            (0xaa188, 0x72a01008), // gradient ARGB00808080
            (0xaa420, 0x32005fe8),
            (0xaa424, 0xb81443a8),
            (0xaa428, 0xb81143a8), // white-rule left colors
            (0xaa498, 0x710b427f), // scrollbar only above720
            (0xa8f74, 0xbd497500),
            (0xa8fcc, 0x94004a89),  // .2 opening, wait parent's close feedback
            (0x10065c, 0x32040fe0), // default SimpleText outline ARGB f0000000
            (0xaf210, 0xf9400900),
            (0xaf218, 0xb900e109), // accepted child closes marker menu
        ] {
            assert_eq!(
                u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
                word,
                "native jump view {a:x}"
            );
        }
        for (a, value) in [
            (0x159620, 420.0f32),
            (0x15974c, 1808.0),
            (0x159574, 1824.0),
            (0x159624, 960.0),
            (0x159788, 966.0),
            (0x159968, 234.0),
            (0x159974, 0.2),
            (0x15989c, -0.2),
        ] {
            assert_eq!(
                f32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap()),
                value,
                "native jump constant {a:x}"
            );
        }
        for (a, text) in [
            (0x15cd9a, "クイズ"),
            (0x162da9, "ケーキ選択"),
            (0x15fdc9, "犯人当て"),
            (0x15b2b9, "最後の選択"),
        ] {
            assert_eq!(&exe[a + 256..a + 256 + text.len()], text.as_bytes());
        }
    }
    #[test]
    fn thumb_uses_separately_truncated_native_endpoints() {
        assert_eq!(scrollbar(12, 0.0), None);
        assert_eq!(scrollbar(13, 0.0), Some(vec4(1808.0, 240.0, 1824.0, 904.0)));
        assert_eq!(
            scrollbar(13, 60.9),
            Some(vec4(1808.0, 295.0, 1824.0, 960.0))
        );
        assert_eq!(
            scrollbar(20, 31.9),
            Some(vec4(1808.0, 258.0, 1824.0, 690.0))
        );
    }
    #[test]
    fn authored_checkpoints_use_short_titles_and_native_palette() {
        use crate::adv::backlog::BacklogEntry;
        let history = Backlog {
            entries: vec![
                BacklogEntry {
                    save_position: 1,
                    item: BacklogItem::Choice {
                        title: "選択".into(),
                        text: "回答".into(),
                        response: 0,
                    },
                },
                BacklogEntry {
                    save_position: 2,
                    item: BacklogItem::Checkpoint {
                        title: "幕".into(),
                        text: "章".into(),
                    },
                },
                BacklogEntry {
                    save_position: 3,
                    item: BacklogItem::Quiz {
                        argument: 0,
                        mode: Some(1),
                        response: 0,
                    },
                },
            ],
        };
        let marker = |i| Marker {
            kind: Kind::Checkpoint,
            authored_row: Some(i),
            target_row: i,
        };
        assert_eq!(
            label(&history, &marker(0)).unwrap(),
            ("選択".into(), [1.0, 0.75, 0.75])
        );
        assert_eq!(
            label(&history, &marker(1)).unwrap(),
            ("章".into(), [0.75, 1.0, 1.0])
        );
        assert_eq!(label(&history, &marker(2)).unwrap().0, "ケーキ選択");
        assert!(label(&history, &marker(3)).is_err());
    }
}
