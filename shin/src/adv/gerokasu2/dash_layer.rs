//! Gerokasu `DashAnimationLayer` (layer type 4, mode 0). Native ctor 249f0,
//! update 247d0 (base properties, then time += delta), render 24810:
//! `dash{trunc(time / 4.5) % 3}` from /effects.txa on one layer-program quad
//! (-1069,-640)-(1069,640), UV 0..1, under the layer transform.
use glam::vec2;
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerShaderOutputKind, PassKind, RenderProgramWithArguments,
    RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{RenderClone, buffer::VertexSource, vertices::PosTexVertex},
};

use crate::{
    asset::{effects::EffectsAtlasBorrow, system::AssetServer},
    layer::{
        NewDrawableLayer, NewDrawableLayerFastForward, NewDrawableLayerNeedsSeparatePass,
        NewDrawableLayerWrapper,
        render_params::{DrawableClipMode, DrawableClipParams, DrawableParams, TransformParams},
    },
    update::{AdvUpdatable, AdvUpdateContext},
};

pub const FRAMES: [&str; 3] = ["dash0", "dash1", "dash2"];

#[derive(Clone, RenderClone)]
pub struct DashRenderer {
    textures: EffectsAtlasBorrow,
    time: f32,
}
pub type DashLayer = NewDrawableLayerWrapper<DashRenderer>;

pub async fn load(assets: &AssetServer) -> anyhow::Result<EffectsAtlasBorrow> {
    let textures = assets.effects_atlas_for(FRAMES[0]).await?;
    for name in &FRAMES[1..] {
        textures.read().require_texture(name)?;
    }
    Ok(textures)
}
pub fn construct(textures: EffectsAtlasBorrow) -> DashLayer {
    let mut layer = NewDrawableLayerWrapper::from_inner_constructor(|| DashRenderer {
        textures,
        time: 0.0,
    });
    crate::layer::DrawableLayer::set_properties(
        &mut layer,
        crate::layer::LayerProperties::with_policy(crate::layer::PropertyPolicy::Gerokasu2),
    );
    layer
}
/// Native 248c8..24900: modff integer part, then a signed `% 3`.
pub fn frame(time: f32) -> usize {
    ((time / 4.5).trunc() as i32).rem_euclid(3) as usize
}
impl AdvUpdatable for DashRenderer {
    fn update(&mut self, context: &AdvUpdateContext) {
        self.time += context.delta_ticks.as_f32();
    }
}
impl NewDrawableLayerNeedsSeparatePass for DashRenderer {}
impl NewDrawableLayerFastForward for DashRenderer {
    fn fast_forward(&mut self) {}
}
impl NewDrawableLayer for DashRenderer {
    fn render_drawable_direct(
        &self,
        pass: &mut RenderPass,
        transform: &TransformParams,
        drawable: &DrawableParams,
        clip: &DrawableClipParams,
        stencil_ref: u8,
        pass_kind: PassKind,
    ) {
        if pass_kind == PassKind::Opaque {
            return;
        }
        assert_eq!(
            clip.mode,
            DrawableClipMode::None,
            "Clipping dash layer is not implemented"
        );
        let vertices = [
            (-1069.0, -640.0, 0.0, 0.0),
            (1069.0, -640.0, 1.0, 0.0),
            (-1069.0, 640.0, 0.0, 1.0),
            (1069.0, 640.0, 1.0, 1.0),
        ]
        .map(|(x, y, u, v)| PosTexVertex {
            position: vec2(x, y),
            texture_position: vec2(u, v),
        });
        let textures = self.textures.read();
        pass.run(
            RenderRequestBuilder::new()
                .depth_stencil_shorthand(stencil_ref, false, false)
                .color_blend_type(ColorBlendType::from_regular_layer(drawable.blend_type))
                .build(
                    RenderProgramWithArguments::Layer {
                        output_kind: LayerShaderOutputKind::Layer,
                        fragment_shader: drawable.fragment_shader,
                        vertices: VertexSource::VertexData {
                            vertices: &vertices,
                        },
                        texture: textures.required(FRAMES[frame(self.time)]).as_source(),
                        transform: transform.compute_final_transform(),
                        color_multiplier: drawable.color_multiplier,
                        fragment_shader_param: drawable.shader_param,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}

#[cfg(test)]
mod gpu_tests {
    use super::*;
    use crate::adv::gerokasu2::card_select::gerokasu_draw::gpu_tests::{Target, assets, save};
    use crate::asset::font::gerokasu2::draw::gpu_tests::Offscreen;

    /// Renders the three dash frames from the ROM's effects.txa; set
    /// `SHIN_GEROKASU_DASH_FRAMES=<dir>` to keep `.rgba` review dumps.
    #[test]
    #[ignore = "requires game-data/gerokasu/data.rom and Metal; renders the dash frames offscreen"]
    fn gerokasu_dash_frames_cover_screen_and_cycle() {
        let gpu = Offscreen::new();
        let assets = assets(&gpu);
        let textures = shin_tasks::block_on(load(&assets)).unwrap();
        let mut target = Target::new(&gpu);
        let dir = std::env::var_os("SHIN_GEROKASU_DASH_FRAMES").map(std::path::PathBuf::from);
        let frames: Vec<Vec<u8>> = [0.0, 4.5, 9.0, 13.5]
            .into_iter()
            .map(|time| {
                let renderer = DashRenderer {
                    textures: textures.clone(),
                    time,
                };
                let pixels = target.draw(&gpu, &renderer);
                if let Some(dir) = &dir {
                    std::fs::create_dir_all(dir).unwrap();
                    save(dir, &format!("dash-{time}"), &pixels);
                }
                pixels
            })
            .collect();
        let backdrop = [90, 70, 70, 255];
        let drawn: Vec<usize> = frames
            .iter()
            .map(|f| f.chunks(4).filter(|p| *p != backdrop).count())
            .collect();
        eprintln!("dash pixels changed per frame: {drawn:?}");
        assert!(drawn.iter().all(|&n| n > 0), "dash frames draw nothing");
        // Frame order 0,1,2 then repeat; the ROM's dash1 and dash2 are
        // pixel-identical, so only frame 0 differs.
        let hashes: Vec<u64> = frames
            .iter()
            .map(|f| {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                f.hash(&mut h);
                h.finish()
            })
            .collect();
        assert_ne!(hashes[0], hashes[1]);
        assert_eq!(hashes[1], hashes[2]);
        assert_eq!(hashes[0], hashes[3]);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn frame_cycles_every_four_and_a_half_ticks() {
        assert_eq!(
            [0.0, 4.4, 4.5, 9.0, 13.4, 13.5].map(super::frame),
            [0, 0, 1, 2, 2, 0]
        );
    }
}
