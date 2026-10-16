//! Actual3001c/308cc BGMMode parent packets. The owner subsequently draws its
//! private ModeStack snapshot (including First's program6/18 capture overlay).
use super::Model;
use crate::{
    adv::{
        characters::view::CharacterTextures,
        higurashi::{
            browser_controls::Scrollbar,
            browser_decorations,
        },
    },
    app::higurashi::gallery_view::{
        CompletionText, Draw, FULL, HelpSheet, alpha, hint, quad, scrollbar_matrix, text_matrix,
    },
    asset::{font::GpuFontLazy, system::AssetServer},
    render::NATIVE_UI_PROJECTION,
};
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

// Actual14bcfc; the fourth record has key4, not the later engine's key5.
const HELP: HelpSheet = HelpSheet {
    texture: "help_bgmmode",
    height: 192.,
    hints: &[
            (5, 0, 0, 214),
            (2, -1, 48, 76),
            (1, -1, 96, 78),
            (4, -1, 144, 78),
        ],
};
pub(super) fn packets(
    m: &Model,
    atlas: bool,
    bar: Scrollbar,
    size: Vec2,
    swapped: bool,
) -> Vec<Draw> {
    if !atlas {
        return vec![];
    }
    let p = NATIVE_UI_PROJECTION;
    let mut out = vec![];
    quad(&mut out, "bg", p, [0., 0., 1920., 1080.], FULL, u32::MAX);
    if let Some(Draw::Quad(bg)) = out.last_mut() {
        bg.opaque = true;
    }
    let origin = m.scroll.value() + -162.;
    let t = m.elapsed * f32::from_bits(0x40490fd0);
    let pulse = (t + t).sin() * f32::from_bits(0x3e4ccccd) + f32::from_bits(0x3ecccccd);
    for (index, row) in m.rows.iter().enumerate() {
        let y = ((index / 2) * 92) as f32 - origin;
        if y < -92. || y >= 1080. {
            continue;
        }
        let x = 150. + ((index & 1) * 820) as f32;
        let top = y + 2.;
        let rect = [x, top, x + 800., top + 88.];
        let uv = if m.playing == index as i32 {
            [0., 0.5, 1., 1.]
        } else {
            [0., 0., 1., 0.5]
        };
        quad(&mut out, "plate", p, rect, uv, u32::MAX);
        if m.selected == index as i32 {
            quad(&mut out, "cur", p, rect, FULL, 0x00ffffff | alpha(pulse));
        }
        let v = ((u32::from(row.name) % 32) * 64) as f32 * f32::from_bits(0x3a000000);
        quad(
            &mut out,
            format!("title{}", row.name / 32),
            p,
            [x + 80., top + 12., x + 720., top + 76.],
            [0., v, 1., v + f32::from_bits(0x3d000000)],
            u32::MAX,
        );
        let f = row.flash.value();
        if f > 0. {
            quad(&mut out, "flash", p, rect, FULL, 0x00ffffff | alpha(f));
        }
    }
    if m.document_height > 817 {
        for vertices in browser_decorations::scrollbar(818., bar) {
            out.push(Draw::Scrollbar {
                matrix: scrollbar_matrix(p, 162.),
                vertices,
            });
        }
    }
    quad(
        &mut out,
        "obi",
        p,
        [0., 0., 1920., 150.],
        [0., 0., 1., 0.625],
        0xd8ffffff,
    );
    quad(
        &mut out,
        "caption",
        p,
        [72., 0., 672., 144.],
        FULL,
        u32::MAX,
    );
    quad(
        &mut out,
        "close",
        p,
        [1764., 30., 1848., 114.],
        FULL,
        u32::MAX,
    );
    if let Some(Draw::Quad(close)) = out.last_mut() {
        close.flash = m.close_flash.value();
    }
    quad(
        &mut out,
        "obi",
        p,
        [0., 992., 1920., 1080.],
        [0., f32::from_bits(0x3f222222), 1., 1.],
        0xd8ffffff,
    );
    let mut right = 1872.;
    for id in [1, 3, 2, 0] {
        let disabled =
            (id == 3 && m.playing < 0) || (id == 2 && !m.rows[m.selected as usize].unlocked);
        right = hint(
            &mut out,
            p,
            &HELP,
            right,
            id,
            if disabled { 0xff808080 } else { u32::MAX },
            swapped,
        );
    }
    out.push(Draw::Completion(text_matrix(p, size)));
    out
}
pub(super) struct View {
    completion: CompletionText,
}
impl View {
    pub fn new(font: Arc<GpuFontLazy>) -> Self {
        Self {
            completion: CompletionText::new(font),
        }
    }
    /// Explicit desktop malformed-archive guard; no replacement artwork is
    /// published. These14 names are actually requested by native3001c/308cc.
    pub fn validate_atlas(atlas: &CharacterTextures) -> Result<()> {
        for name in [
            "bg",
            "plate",
            "cur",
            "flash",
            "obi",
            "caption",
            "close",
            "help_bgmmode",
            "key",
            "title0",
            "title1",
            "title2",
            "title3",
            "title4",
        ] {
            ensure!(atlas.contains(name), "Music archive is missing {name}");
        }
        Ok(())
    }
    pub fn synchronize(&mut self, server: &AssetServer, model: &Model) -> Result<()> {
        self.completion.synchronize(server, &model.completion_text)
    }
    /// Parent draws are atlas-gated; private First drawing belongs AFTER this
    /// method and is permitted while the worker is still pending/unpublished.
    pub fn render(
        &self,
        model: &Model,
        atlas: Option<&CharacterTextures>,
        system: &CharacterTextures,
        swapped: bool,
        pass: &mut RenderPass,
    ) -> Result<()> {
        let size = self.completion.size(atlas.is_some(), "Music")?;
        self.completion
            .render(packets(model, atlas.is_some(), model.scrollbar, size, swapped), atlas, system, pass);
        Ok(())
    }
}
#[cfg(test)]
mod tests;
