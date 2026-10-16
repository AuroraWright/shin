//! Actual93e28/94470 MovieMode parent packets. The owner subsequently draws its
//! private ModeStack snapshot (including First's program6/18 capture overlay).
use super::ViewState;
use crate::{
    adv::{
        characters::view::CharacterTextures,
        higurashi::{
            browser_decorations,
        },
    },
    app::higurashi::gallery_view::{
        CompletionText, Draw, FULL, HelpSheet, hint, quad, scrollbar_matrix, text_matrix,
    },
    asset::{font::GpuFontLazy, system::AssetServer},
    render::NATIVE_UI_PROJECTION,
};
use anyhow::{Result, ensure};
use glam::Vec2;
use shin_render::render_pass::RenderPass;
use std::sync::Arc;

// Actual150d24 has three 12-byte entries and exactly one key each.
const HELP: HelpSheet = HelpSheet {
    texture: "help_moviemode",
    height: 144.,
    hints: &[(0, -1, 0, 214), (2, -1, 48, 76), (1, -1, 96, 78)],
};
pub(super) fn packets(state: &ViewState, size: Vec2, swapped: bool) -> Vec<Draw> {
    if !state.ready {
        return vec![];
    }
    let p = NATIVE_UI_PROJECTION;
    let mut out = vec![];
    quad(&mut out, "bg", p, [0., 0., 1920., 1080.], FULL, u32::MAX);
    if let Some(Draw::Quad(bg)) = out.last_mut() {
        bg.opaque = true;
    }
    let origin = state.scroll + -210.;
    for row in &state.rows {
        let grow = (row.focus + -0.5) * 0.5;
        let width = grow * 75.;
        let height = grow * 42.;
        let left = (row.x + 138.) - width * 0.5;
        let top = (row.y + 0.) - origin - height * 0.5;
        let rect = [left, top, (width + 384.) + left, (height + 216.) + top];
        let gray = ((row.focus.max(0.) * 255.) as u32).min(255);
        let color = 0xff000000 | gray | (gray << 8) | (gray << 16);
        quad(
            &mut out,
            if row.locked {
                "logo".to_owned()
            } else {
                format!("mv{:02}", u32::from(row.movie_id) + 1)
            },
            p,
            rect,
            FULL,
            color,
        );
        // 93ec0's blend1 survives the entire thumbnail loop until94084.
        if let Some(Draw::Quad(q)) = out.last_mut() {
            q.opaque = true;
        }
    }
    // Authored12 rows produce720 exactly; greater heights are only explicit
    // component seams until an actual catalog can reach that branch.
    if state.document_height > 720 {
        for vertices in browser_decorations::scrollbar(720., state.scrollbar) {
            out.push(Draw::Scrollbar {
                matrix: scrollbar_matrix(p, 210.),
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
        close.flash = state.close_flash;
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
    for id in [1, 2, 0] {
        let disabled = id == 2 && state.rows.get(state.selected).is_some_and(|r| r.locked);
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
    /// published. These19 names are actually requested by native93e28/94470.
    pub fn validate_atlas(atlas: &CharacterTextures) -> Result<()> {
        for name in [
            "bg",
            "logo",
            "obi",
            "caption",
            "close",
            "help_moviemode",
            "key",
        ] {
            ensure!(atlas.contains(name), "Movie archive is missing {name}");
        }
        for id in 1..=12 {
            let name = format!("mv{id:02}");
            ensure!(atlas.contains(&name), "Movie archive is missing {name}");
        }
        Ok(())
    }
    pub fn synchronize(&mut self, server: &AssetServer, state: &ViewState) -> Result<()> {
        self.completion.synchronize(server, &state.completion)
    }
    /// Parent draws are atlas-gated; private First drawing belongs AFTER this
    /// method and is permitted while the worker is still pending/unpublished.
    pub fn render(
        &self,
        state: &ViewState,
        atlas: Option<&CharacterTextures>,
        system: &CharacterTextures,
        swapped: bool,
        pass: &mut RenderPass,
    ) -> Result<()> {
        let size = self.completion.size(atlas.is_some(), "Movie")?;
        ensure!(
            state.ready == atlas.is_some(),
            "Movie atlas publication and view state disagree"
        );
        ensure!(
            !state.ready || state.selected < state.rows.len(),
            "Movie selected row outside gallery"
        );
        self.completion
            .render(packets(state, size, swapped), atlas, system, pass);
        Ok(())
    }
}
#[cfg(test)]
mod tests;
