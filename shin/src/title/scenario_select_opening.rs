//! Native Start worker e2e60 and opening task e0ba0/curtain62df0.
use crate::{
    adv::character_view::CharacterTextures,
    asset::{mask::MaskTexture, system::AssetServer},
    render::top_left_projection_matrix,
};
use glam::vec2;
use shin_core::time::Ticks;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::MaskVertex},
};
use std::sync::{Arc, mpsc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OpeningEvent {
    StopMusic,
    StartMusic,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Loading,
    Curtain,
    Ready,
}
pub(super) struct OpeningClock {
    phase: Phase,
    elapsed: f32,
    events: Vec<OpeningEvent>,
}
impl OpeningClock {
    fn new() -> Self {
        Self {
            phase: Phase::Loading,
            elapsed: 0.0,
            events: Vec::new(),
        }
    }
    fn assets_ready(&mut self) {
        if self.phase == Phase::Loading {
            self.phase = Phase::Curtain;
            self.events.push(OpeningEvent::StopMusic);
        }
    }
    fn prepare_frame(&mut self) {
        if self.phase == Phase::Curtain && self.elapsed >= 60.0 {
            self.phase = Phase::Ready;
            self.events.push(OpeningEvent::StartMusic);
        }
    }
    fn advance(&mut self, delta: Ticks) {
        if self.phase != Phase::Loading {
            self.elapsed = (self.elapsed + delta.as_f32()).min(60.0);
        }
    }
    pub fn accepts_input(&self) -> bool {
        self.phase == Phase::Ready
    }
    pub fn take_events(&mut self) -> Vec<OpeningEvent> {
        std::mem::take(&mut self.events)
    }
    fn minmax(&self) -> glam::Vec2 {
        // Native softness.5: reciprocal2; min=1-progress*3,max=min+2.
        let min = 1.0 - self.elapsed / 60.0 * 3.0;
        vec2(min, min + 2.0)
    }
}
pub(super) struct ScenarioSelectOpening {
    pub clock: OpeningClock,
    atlas: Option<Arc<CharacterTextures>>,
    system: Option<Arc<CharacterTextures>>,
    received: mpsc::Receiver<(Arc<CharacterTextures>, Arc<CharacterTextures>)>,
    loading: Option<shin_tasks::AsyncTask<anyhow::Result<Arc<MaskTexture>>>>,
    mask: Option<Arc<MaskTexture>>,
}
impl ScenarioSelectOpening {
    #[cfg(test)]
    pub fn new(server: Arc<AssetServer>) -> Self {
        Self::new_for_setup(
            server,
            &super::scenario_select::ScenarioSelectSetup::from_save(&Default::default()),
        )
    }
    pub fn new_for_setup(
        server: Arc<AssetServer>,
        setup: &super::scenario_select::ScenarioSelectSetup,
    ) -> Self {
        let mut names = vec![
            "bg".to_owned(),
            "menu".to_owned(),
            "close".to_owned(),
            "help_snrsel".to_owned(),
            "key".to_owned(),
        ];
        if setup.groups.len() > 1 {
            names.extend(["tab_off".to_owned(), "tab_on".to_owned()]);
            if setup.groups.iter().any(|group| group.has_new) {
                names.push("new".to_owned());
            }
        }
        for entry in setup.groups.iter().flat_map(|group| &group.entries) {
            names.push(entry.details_name());
            if let Some(badge) = match entry.state {
                0 => Some("lock"),
                1 => Some("new"),
                3 => Some("star"),
                _ => None,
            } {
                names.push(badge.to_owned());
            }
        }
        names.sort();
        names.dedup();
        let needs_scroll = setup.groups.iter().any(|group| group.content_height > 670);
        let (send, received) = mpsc::channel();
        let loading = shin_tasks::async_io::spawn(async move {
            let atlas = server.load::<CharacterTextures, _>("/snrsel.txa").await?;
            let system = server.load::<CharacterTextures, _>("/systex.txa").await?;
            atlas.require_names(
                "/snrsel.txa",
                &names.iter().map(String::as_str).collect::<Vec<_>>(),
            )?;
            if needs_scroll {
                system.require_names("/systex.txa", &["scroll"])?;
            }
            let _ = send.send((atlas, system));
            server.load::<MaskTexture, _>("/curtain.msk").await
        });
        Self {
            clock: OpeningClock::new(),
            atlas: None,
            system: None,
            received,
            loading: Some(loading),
            mask: None,
        }
    }
    pub fn is_loaded(&self) -> bool {
        self.mask.is_some()
    }
    pub fn atlas(&self) -> Option<&Arc<CharacterTextures>> {
        self.atlas.as_ref()
    }
    pub fn system(&self) -> Option<&Arc<CharacterTextures>> {
        self.system.as_ref()
    }
    pub fn update_frame(&mut self, delta: Ticks) -> anyhow::Result<()> {
        if let Ok((atlas, system)) = self.received.try_recv() {
            self.atlas = Some(atlas);
            self.system = Some(system);
        }
        if let Some(result) = self.loading.as_mut().and_then(|task| task.poll_naive()) {
            self.loading = None;
            self.mask = Some(result?);
            if self.atlas.is_none() {
                if let Ok((atlas, system)) = self.received.try_recv() {
                    self.atlas = Some(atlas);
                    self.system = Some(system);
                }
            }
            self.clock.assets_ready();
        }
        self.clock.prepare_frame();
        self.clock.advance(delta);
        Ok(())
    }
    /// Draw over the newly composed picker using the retained previous screen.
    /// The scene host owns that capture and its lifetime across factory swaps.
    pub fn render_curtain(&self, previous: TextureSource<'_>, pass: &mut RenderPass) {
        if self.clock.elapsed >= 60.0 {
            return;
        }
        if let Some(mask) = &self.mask {
            render_curtain(
                previous,
                mask.texture.as_source(),
                self.clock.minmax(),
                pass,
            );
        } else {
            // Native62f70 selects63180 when the mask is absent. At loading
            // progress zero the retained source covers the whole new scene.
            shin_render::quad_vertices::QuadVertices::new()
                .with_box(0.0, 0.0, 1920.0, 1080.0)
                .with_tex_box(0.0, 0.0, 1.0, 1.0)
                .with_color(shin_core::primitives::color::UnormColor::WHITE)
                .render_sprite(
                    pass,
                    RenderRequestBuilder::new()
                        .color_blend_type(ColorBlendType::LayerPremultiplied1),
                    previous,
                    top_left_projection_matrix(),
                );
        }
    }
}
fn render_curtain(
    previous: TextureSource<'_>,
    mask: TextureSource<'_>,
    minmax: glam::Vec2,
    pass: &mut RenderPass,
) {
    let vertices = shin_render::quad_vertices::build_quad_vertices(|corner| MaskVertex {
        position: corner * vec2(1920.0, 1080.0),
        texture_position: corner,
        mask_position: corner,
    });
    pass.run(
        RenderRequestBuilder::new()
            .color_blend_type(ColorBlendType::LayerPremultiplied1)
            .build(
                RenderProgramWithArguments::ScreenCurtain {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    texture: previous,
                    mask,
                    transform: top_left_projection_matrix(),
                    minmax,
                },
                DrawPrimitive::TrianglesStrip,
            ),
    );
}
#[cfg(test)]
#[path = "scenario_select_opening_reference.rs"]
mod reference;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curtain_clock_waits_for_assets_and_polls_completion_before_advancing() {
        let mut clock = OpeningClock::new();
        clock.advance(Ticks::from_f32(100.0));
        assert_eq!(clock.elapsed, 0.0);
        assert!(!clock.accepts_input());
        clock.assets_ready();
        clock.assets_ready();
        assert_eq!(clock.take_events(), [OpeningEvent::StopMusic]);
        assert_eq!(clock.minmax(), vec2(1.0, 3.0));
        clock.prepare_frame();
        clock.advance(Ticks::from_f32(30.0));
        assert_eq!(clock.minmax(), vec2(-0.5, 1.5));
        assert!(!clock.accepts_input());
        clock.advance(Ticks::from_f32(40.0));
        assert_eq!(clock.elapsed, 60.0);
        assert!(!clock.accepts_input());
        assert_eq!(clock.minmax(), vec2(-2.0, 0.0));
        clock.prepare_frame();
        assert!(clock.accepts_input());
        assert_eq!(clock.take_events(), [OpeningEvent::StartMusic]);
        clock.prepare_frame();
        assert!(clock.take_events().is_empty());
    }
    #[test]
    #[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
    fn reference_native_start_opening_clock_and_premultiplied_curtain_program() {
        let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&rom)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |address: usize| {
            u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap())
        };
        for (address, expected) in [
            (0xe0bc4, 0x7100051f),
            (0xe0bdc, 0x7100081f),
            (0xe0be0, 0x540006e1),
            (0xe0c28, 0x1e2c1000),
            (0xe0c34, 0x321e0fe1),
            (0xe0c40, 0x97fe086c),
            (0xe0c6c, 0x321e0fe1),
            (0xe0c70, 0x97fd4bf0),
            (0xe0c84, 0x97fe08a3),
            (0xe0c88, 0x370001a0),
            (0xe0c9c, 0x528000a1),
            (0xe0ca8, 0x97fd4ad6),
            (0xe0cb0, 0x97fff9a0),
            (0x62f1c, 0x1e212000),
            (0x62f54, 0xbd000400),
            (0x63068, 0x1e282801),
            (0x6306c, 0x1e231842),
            (0x63088, 0x1e210c61),
            (0x6308c, 0x1e200c80),
            (0x63148, 0x321b03e0),
        ] {
            assert_eq!(word(address), expected, "native{address:x}");
        }
        let relocations: std::collections::HashMap<_, _> = (0x11e040..0x152eb8)
            .step_by(24)
            .map(|offset| {
                (
                    u64::from_le_bytes(exe[offset + 256..offset + 264].try_into().unwrap()),
                    u64::from_le_bytes(exe[offset + 272..offset + 280].try_into().unwrap()),
                )
            })
            .collect();
        assert_eq!(relocations[&0x21f690], 0xe0ba0);
        assert_eq!(relocations[&0x1dc7c0], 0x1e9cb0);
        assert_eq!(relocations[&0x1dc7c8], 0x1e8ff0);
        let base = 0x1e8ff0 + 0xb40 + 256;
        for (offset, expected) in [
            (0x08, 0x4c98078c00070008u64),
            (0x18, 0x4c59100c00170808),
            (0x78, 0xd82200aff0570405),
            (0x90, 0xd832008020770600),
            (0x98, 0x51a4028c00070808),
            (0xa8, 0x5c68100000870000),
            (0xb0, 0x5c68100000870101),
            (0xb8, 0x5c68100000870202),
            (0xc8, 0x5c68100000870303),
        ] {
            assert_eq!(
                u64::from_le_bytes(exe[base + offset..base + offset + 8].try_into().unwrap()),
                expected,
                "Maxwell program32+{offset:x}"
            );
        }
    }
}
