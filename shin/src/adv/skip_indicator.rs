//! Native ADV top-right autoplay and skip status (original/1.03 12700..1293c).
use super::{character_view::CharacterTextures, playback::PlaybackModes};
use shin_core::{primitives::color::FloatColor4, time::Ticks};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};
use std::sync::Arc;

pub(super) struct SkipIndicator {
    atlas: Arc<CharacterTextures>,
    phase: f32,
}
impl SkipIndicator {
    pub fn new(atlas: Arc<CharacterTextures>) -> Self {
        Self { atlas, phase: 0.0 }
    }
    pub fn update(&mut self, ticks: Ticks) {
        // Native11950..11964 accumulates f32 phase without wrapping.
        self.phase += ticks.as_f32() * 0.01666666753590107f32;
    }
    pub fn render(
        &self,
        pass: &mut RenderPass,
        modes: &PlaybackModes,
        skip_requested: bool,
        auto_active: bool,
        skip_active: bool,
        read_only_skip: bool,
    ) {
        let pulse = 0.75 + 0.25 * (self.phase * 6.283180236816406f32).sin();
        let dim = FloatColor4::from_rgba(0.25, 0.25, 0.25, 1.0);
        // Native bit 1 draws the lower atlas half first: double skip arrows.
        if skip_requested {
            let color = if !skip_active {
                dim
            } else if read_only_skip {
                FloatColor4::from_rgba(pulse, pulse, 1.0, 1.0)
            } else {
                FloatColor4::from_rgba(1.0, pulse, pulse, 1.0)
            };
            self.draw(pass, 1824.0, 0.5, color);
        }
        // Native bit 0 draws the upper atlas half: single autoplay arrow.
        if modes.autoplay {
            let left = if skip_requested { 1744.0 } else { 1824.0 };
            let color = if auto_active {
                FloatColor4::from_rgba(pulse, 1.0, pulse, 1.0)
            } else {
                dim
            };
            self.draw(pass, left, 0.0, color);
        }
    }

    fn draw(&self, pass: &mut RenderPass, left: f32, top_uv: f32, color: FloatColor4) {
        QuadVertices::new()
            .with_box(left, 24.0, left + 72.0, 96.0)
            .with_tex_box(0.0, top_uv, 1.0, top_uv + 0.5)
            .with_color(color.into_unorm())
            .render_sprite(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                self.atlas.get("adv").as_source(),
                crate::render::top_left_projection_matrix(),
            );
    }
}

#[cfg(test)]
pub(super) fn assert_reference_render<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
) {
    use super::{Adv, assets::AdvAssets};
    use crate::update::{Updatable, UpdateContext};
    use shin_core::{
        format::rom::RomReader,
        primitives::{color::UnormColor, stateless_reader::StatelessFile},
    };
    use shin_render::shaders::types::texture::{TextureTarget, TextureTargetKind};
    use std::io::Read;
    let assets = shin_tasks::block_on(AdvAssets::load(server)).unwrap();
    let mut live = Adv::new(
        Arc::new(shin_audio::AudioManager::new_for_tests()),
        server.clone(),
        assets.clone(),
        shin_core::vm::Scripter::new(&assets.scenario, 0, 42),
    );
    live.vm_state.settings.v11_disallowskipunread = false;
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Native skip indicator"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let draw = |live: &Adv, context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>| {
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::Screen,
                },
                None,
                "Native ADV skip",
            );
            pass.clear(Some(UnormColor::BLACK), None, None);
            live.render(&mut pass);
        }
        crate::layer::user::readback_test_texture(context, &target)
    };
    let blank = draw(&live, context);
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(&path).unwrap()).unwrap()).unwrap();
    let mut b = Vec::new();
    rom.open_file(rom.find_file("/adv.txa").unwrap())
        .unwrap()
        .read_to_end(&mut b)
        .unwrap();
    let atlas = shin_core::format::texture_archive::read_texture_archive(&b).unwrap();
    let art = &atlas.textures[atlas.name_to_index["adv"]];
    eprintln!("Native skip atlas{:?}", art.dimensions());
    let sample = |u: f32, v: f32| -> [f32; 4] {
        let x = (u * art.width() as f32 - 0.5).clamp(0.0, art.width() as f32 - 1.0);
        let y = (v * art.height() as f32 - 0.5).clamp(0.0, art.height() as f32 - 1.0);
        let a = x.floor() as u32;
        let b = y.floor() as u32;
        let c = (a + 1).min(art.width() - 1);
        let d = (b + 1).min(art.height() - 1);
        std::array::from_fn(|channel| {
            let p = |x: u32, y: u32| {
                art.as_raw()[((y * art.width() + x) * 4) as usize + channel] as f32
            };
            (p(a, b) * (1.0 - x + a as f32) + p(c, b) * (x - a as f32)) * (1.0 - y + b as f32)
                + (p(a, d) * (1.0 - x + a as f32) + p(c, d) * (x - a as f32)) * (y - b as f32)
        })
    };
    // Decode both authored glyphs independently; test complete root output.
    let verify = |pixels: &[u8], icons: &[(usize, f32, [u8; 3])]| {
        let mut worst = 0.0f32;
        for y in 0..1080usize {
            for x in 0..1920usize {
                let o = (y * 1920 + x) * 4;
                if let Some(&(left, top_uv, color)) = icons
                    .iter()
                    .find(|&&(left, _, _)| (left..left + 72).contains(&x) && (24..96).contains(&y))
                {
                    let t = sample(
                        (x as f32 + 0.5 - left as f32) / 72.0,
                        top_uv + (y as f32 + 0.5 - 24.0) / 144.0,
                    );
                    for c in 0..3 {
                        let a = t[3] / 255.0;
                        let expected =
                            t[c] * color[c] as f32 / 255.0 * a + blank[o + c] as f32 * (1.0 - a);
                        worst = worst.max((pixels[o + c] as f32 - expected).abs());
                    }
                } else {
                    assert_eq!(
                        &pixels[o..o + 4],
                        &blank[o..o + 4],
                        "Outside native playback rects {x},{y}"
                    );
                }
            }
        }
        assert!(worst < 1.1, "Playback icons {icons:?} RGB error {worst}");
        eprintln!(
            "Native ADV playback {icons:?}: {} decoded pixels, outside unchanged, maxRGB {worst}",
            icons.len() * 5184
        );
    };
    // CTRL uses the same artwork as latched skip, without setting that bit.
    let mut held_keys =
        enum_map::EnumMap::<crate::app::AppAction, shin_input::ActionState>::default();
    held_keys[crate::app::AppAction::HoldSkip].is_held = true;
    live.handle_input(held_keys, true);
    assert!(live.skip_indicator_active());
    assert!(!live.adv_state.playback_modes.skip);
    assert!(live.pending_playback_sound.is_none());
    let held_pixels = draw(&live, context);
    verify(&held_pixels, &[(1824, 0.5, [255, 191, 191])]);
    live.adv_state.playback_modes.skip = true;
    live.handle_input(Default::default(), true);
    assert_eq!(
        draw(&live, context),
        held_pixels,
        "CTRL and S glyphs must match"
    );
    live.adv_state.playback_modes.skip = false;
    live.handle_input(held_keys, false);
    assert!(!live.held_skip, "Focus loss clears temporary skip");
    assert_eq!(draw(&live, context), blank);
    live.handle_input(held_keys, true);
    live.handle_input(Default::default(), true);
    assert_eq!(
        draw(&live, context),
        blank,
        "Release removes temporary arrow"
    );
    let mut keys = enum_map::EnumMap::<crate::app::AppAction, shin_input::ActionState>::default();
    keys[crate::app::AppAction::ToggleAuto].is_clicked = true;
    live.handle_input(keys, true);
    assert!(live.autoplay_indicator_active());
    assert!(!live.skip_indicator_active());
    verify(&draw(&live, context), &[(1824, 0.0, [191, 255, 191])]);
    live.handle_input(held_keys, true);
    verify(
        &draw(&live, context),
        &[(1824, 0.5, [255, 191, 191]), (1744, 0.0, [191, 255, 191])],
    );
    live.vm_state.settings.v11_disallowskipunread = true;
    live.vm_state.messagebox_state.messagebox_shown = true;
    live.vm_state.messagebox_state.was_read = false;
    verify(
        &draw(&live, context),
        &[(1824, 0.5, [63, 63, 63]), (1744, 0.0, [191, 255, 191])],
    );
    live.vm_state.messagebox_state.was_read = true;
    verify(
        &draw(&live, context),
        &[(1824, 0.5, [191, 191, 255]), (1744, 0.0, [191, 255, 191])],
    );
    live.vm_state.settings.v11_disallowskipunread = false;
    live.vm_state.current_event = Some(0);
    verify(
        &draw(&live, context),
        &[(1824, 0.5, [63, 63, 63]), (1744, 0.0, [63, 63, 63])],
    );
    live.vm_state.current_event = None;
    live.adv_state.hidden_ui_active = true;
    assert!(!live.skip_indicator_requested());
    verify(&draw(&live, context), &[(1824, 0.0, [63, 63, 63])]);
    live.adv_state.hidden_ui_active = false;
    live.handle_input(Default::default(), true);
    verify(&draw(&live, context), &[(1824, 0.0, [191, 255, 191])]);
    let mut keys = enum_map::EnumMap::<crate::app::AppAction, shin_input::ActionState>::default();
    keys[crate::app::AppAction::ToggleSkip].is_clicked = true;
    live.handle_input(keys, true);
    assert!(live.skip_indicator_active());
    verify(
        &draw(&live, context),
        &[(1824, 0.5, [255, 191, 191]), (1744, 0.0, [191, 255, 191])],
    );
    // Skip alone is the lower glyph, red, in the rightmost slot.
    live.adv_state.playback_modes.autoplay = false;
    verify(&draw(&live, context), &[(1824, 0.5, [255, 191, 191])]);
    for color in [[255, 255, 255], [255, 191, 191], [255, 127, 127]] {
        let mut update = UpdateContext {
            frame_id: Default::default(),
            frame_started_at: std::time::Instant::now(),
            delta_ticks: Ticks::from_f32(15.0),
            asset_server: server,
            pre_render: context,
        };
        live.adv_state.update(&mut update);
        verify(&draw(&live, context), &[(1824, 0.5, color)]);
    }
    live.adv_state.playback_modes.autoplay = true;
    live.vm_state.settings.v11_disallowskipunread = true;
    live.vm_state.messagebox_state.messagebox_shown = true;
    live.vm_state.messagebox_state.was_read = false;
    assert!(!live.vm_state.can_skip_current_message());
    assert!(!live.skip_indicator_active());
    assert!(live.autoplay_indicator_active());
    verify(
        &draw(&live, context),
        &[(1824, 0.5, [63, 63, 63]), (1744, 0.0, [127, 255, 127])],
    );
    live.vm_state.messagebox_state.was_read = true;
    assert!(live.skip_indicator_active());
    verify(
        &draw(&live, context),
        &[(1824, 0.5, [127, 127, 255]), (1744, 0.0, [127, 255, 127])],
    );
    live.vm_state.settings.v11_disallowskipunread = false;
    live.vm_state.current_event = Some(0);
    assert!(!live.skip_indicator_active());
    assert!(!live.autoplay_indicator_active());
    verify(
        &draw(&live, context),
        &[(1824, 0.5, [63, 63, 63]), (1744, 0.0, [63, 63, 63])],
    );
    live.vm_state.current_event = None;
    live.vm_state.system_controls_allowed = false;
    verify(
        &draw(&live, context),
        &[(1824, 0.5, [63, 63, 63]), (1744, 0.0, [63, 63, 63])],
    );
    live.vm_state.system_controls_allowed = true;
    live.adv_state.hidden_ui_active = true;
    verify(
        &draw(&live, context),
        &[(1824, 0.5, [63, 63, 63]), (1744, 0.0, [63, 63, 63])],
    );
    live.adv_state.hidden_ui_active = false;
    live.adv_state.playback_modes.skip = false;
    verify(&draw(&live, context), &[(1824, 0.0, [127, 255, 127])]);
    live.adv_state.playback_modes.reset();
    assert_eq!(draw(&live, context), blank);
    // Lower real controller shoulders: L/autoplay wins a simultaneous press.
    let mut raw = shin_input::RawInputState::new();
    raw.gamepads.unified.buttons[shin_input::inputs::GamepadButton::L] = true;
    raw.gamepads.unified.buttons[shin_input::inputs::GamepadButton::R] = true;
    let lowered = <crate::app::AppAction as shin_input::Action>::lower(&raw);
    let keys = enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
        is_clicked: lowered[action],
        is_held: lowered[action],
        ..Default::default()
    });
    live.handle_input(keys, true);
    assert!(live.adv_state.playback_modes.autoplay && !live.adv_state.playback_modes.skip);
    verify(&draw(&live, context), &[(1824, 0.0, [127, 255, 127])]);
    raw.gamepads.unified.buttons[shin_input::inputs::GamepadButton::L] = false;
    let lowered = <crate::app::AppAction as shin_input::Action>::lower(&raw);
    let keys = enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
        is_clicked: lowered[action],
        is_held: lowered[action],
        ..Default::default()
    });
    live.handle_input(keys, true);
    assert!(live.adv_state.playback_modes.autoplay && live.adv_state.playback_modes.skip);
    verify(
        &draw(&live, context),
        &[(1824, 0.5, [255, 127, 127]), (1744, 0.0, [127, 255, 127])],
    );
    live.adv_state.playback_modes.reset();
    let phase = live.adv_state.skip_indicator.phase;
    {
        let mut update = UpdateContext {
            frame_id: Default::default(),
            frame_started_at: std::time::Instant::now(),
            delta_ticks: Ticks::from_f32(150.0),
            asset_server: server,
            pre_render: context,
        };
        live.adv_state.update_presentation(&mut update);
    }
    assert_eq!(
        live.adv_state.skip_indicator.phase,
        (phase + 150.0 * 0.01666666753590107f32)
    );
    assert!(
        live.adv_state.skip_indicator.phase > 1.0,
        "Native phase never wraps"
    );
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "requires both supplied Switch executables"]
    fn reference_native_skip_icon_geometry_tint_phase_and_mode_bit() {
        let p = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let folder = std::path::Path::new(&p).parent().unwrap();
        for (name, offset) in [
            ("uncompressed_executable", 0usize),
            ("uncompressed_executable_update", 0xfe0),
        ] {
            let b = std::fs::read(folder.join(name)).unwrap();
            let word = |a: usize| u32::from_le_bytes(b[a + 256..a + 260].try_into().unwrap());
            for (a, v) in [
                (0x12770, 0x37000868),
                (0x12768, 0x37080088), // native skip is bit 1
                (0x127e4, 0x2d0203e0), // read-only skip: blue
                (0x127e8, 0xb9001bf5),
                (0x127f0, 0xb90013f5), // ordinary skip: red
                (0x12814, 0x2d0283e0),
                (0x12840, 0x1e2c1001), // skip lower atlas half
                (0x12848, 0x4ea21c43),
                (0x128d8, 0xbd001be0), // autoplay: green
                (0x128dc, 0xbd0013e0),
                (0x128e0, 0xb90017f4),
                (0x12914, 0x4ea01c01), // autoplay upper atlas half
                (0x12480, 0x360ffe88), // skip active mode bit
                (0x124d4, 0x3942ed08), // read-only policy
                (0x124e0, 0x38686a68), // read-message gate
                (0x12880, 0x1e2e1003),
                (0x128c4, 0x1e210800),
                (0x128cc, 0x1e2b0803),
                (0x128d4, 0x1e212860),
                (0x128f8, 0x1e271001),
                (0x12910, 0x1e2c1003),
                (0x11958, 0x1e200900),
                (0x11960, 0x1e212800),
                (0x11964, 0xbd049a60),
                (0x1236c, 0x3951f008),
            ] {
                assert_eq!(word(a), v, "{name}{a:x}");
            }
            for (a, v) in [
                (0x159574, 1824.0f32),
                (0x1593dc, 1896.0),
                (0x159818, 96.0),
                (0x159ab4, 72.0),
                (0x1595d4, 1744.0),
                (0x159598, 6.283180236816406),
                (0x159a5c, 0.01666666753590107),
            ] {
                assert_eq!(word(a + offset), v.to_bits(), "{name} float{a:x}");
            }
        }
    }
}
