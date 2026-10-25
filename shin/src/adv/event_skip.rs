//! Native ADV event touch callbacks157d0/15c00 and skip hint12b70..12c48.
use super::{
    characters::view::CharacterTextures,
    input_area::{InputArea, InputAreaTarget, PointerFrame},
};
use glam::Vec2;
use shin_core::{primitives::color::FloatColor4, time::Ticks};
use shin_render::{
    ColorBlendType, RenderRequestBuilder, quad_vertices::QuadVertices, render_pass::RenderPass,
};
use std::sync::Arc;

#[derive(Default)]
struct Contact {
    tapped: bool,
    skipped: bool,
}

#[cfg(test)]
mod tests {
    use super::super::input_area::PointerSampler;
    use super::*;

    #[test]
    fn event_contact_requires_hold_and_cancels_on_drag_resize_and_input_loss() {
        let mut skip = EventSkip::new();
        let mut sampler = PointerSampler::default();
        let point = Vec2::new(300.0, 700.0);
        sampler.sample(point, true, 0.0);
        assert!(skip.pointer(&sampler.frame(), (1920, 1080), true));
        sampler.sample(point, false, 0.1);
        assert!(skip.pointer(&sampler.frame(), (1920, 1080), true));
        assert!(skip.touch_hint);
        assert!(!skip.take_request());
        sampler.sample(point, true, 0.1);
        skip.pointer(&sampler.frame(), (1920, 1080), true);
        sampler.sample(point, true, 1.149);
        skip.pointer(&sampler.frame(), (1920, 1080), true);
        assert!(!skip.take_request());
        sampler.sample(point, true, 0.002);
        skip.pointer(&sampler.frame(), (1920, 1080), true);
        assert!(skip.take_request());
        assert!(!skip.take_request());
        sampler.sample(point, false, 0.0);
        skip.pointer(&sampler.frame(), (1920, 1080), true);
        for cancel in 0..3 {
            sampler.sample(point, true, 0.0);
            skip.pointer(&sampler.frame(), (1920, 1080), true);
            let moved = if cancel == 0 {
                point + Vec2::new(100.0, 0.0)
            } else {
                point
            };
            sampler.sample(moved, true, 0.2);
            skip.pointer(
                &sampler.frame(),
                if cancel == 1 {
                    (1280, 720)
                } else {
                    (1920, 1080)
                },
                cancel != 2,
            );
            sampler.sample(moved, true, 2.0);
            skip.pointer(&sampler.frame(), (1920, 1080), true);
            assert!(!skip.take_request(), "cancel cause {cancel}");
            sampler.sample(moved, false, 0.0);
            skip.pointer(&sampler.frame(), (1920, 1080), true);
        }
        sampler.sample(point, true, 0.0);
        skip.pointer(&sampler.frame(), (1920, 1080), true);
        sampler.sample(point, true, 0.1);
        assert!(!skip.pointer(&sampler.frame(), (0, 0), true));
        sampler.sample(point, true, 2.0);
        skip.pointer(&sampler.frame(), (1920, 1080), true);
        assert!(!skip.take_request(), "Minimized viewport cancels capture");
    }
}
impl InputAreaTarget for Contact {
    fn probe(&mut self, _: Vec2) -> bool {
        true
    }
    fn reset(&mut self) {}
    fn press(&mut self, _: Vec2) {}
    fn release(&mut self, _: Vec2) {
        self.tapped = true;
    }
    fn long_press(&mut self, _: Vec2) {
        self.skipped = true;
    }
    fn drag(&mut self, _: Vec2, _: Vec2) {}
    fn gesture(&mut self, _: Vec2) {}
    fn cancel(&mut self) {}
}

pub(super) struct EventSkip {
    area: InputArea,
    contact: Contact,
    extent: (u16, u16),
    hint_time: Option<f32>,
    touch_hint: bool,
}
impl EventSkip {
    pub fn new() -> Self {
        let mut area = InputArea::canvas();
        area.set_long_press_enabled(true);
        Self {
            area,
            contact: Contact::default(),
            extent: (1920, 1080),
            hint_time: None,
            touch_hint: false,
        }
    }
    pub fn pointer(&mut self, frame: &PointerFrame<'_>, extent: (u16, u16), enabled: bool) -> bool {
        if extent.0 == 0 || extent.1 == 0 {
            self.area.disable(&mut self.contact);
            self.contact = Contact::default();
            return false;
        }
        if extent != self.extent {
            self.area.disable(&mut self.contact);
            self.area = InputArea::new(extent.0, extent.1);
            self.area.set_long_press_enabled(true);
            self.extent = extent;
        }
        if !enabled {
            self.area.disable(&mut self.contact);
            self.contact = Contact::default();
            return false;
        }
        let consumed = self.area.update(&mut self.contact, frame);
        if std::mem::take(&mut self.contact.tapped) {
            self.show_hint(true);
        }
        consumed
    }
    pub fn take_request(&mut self) -> bool {
        std::mem::take(&mut self.contact.skipped)
    }
    pub fn show_hint(&mut self, touch: bool) {
        // Native bb9f0 prevents restarting an outstanding32/120/32tick queue.
        if self.hint_time.is_none() {
            self.hint_time = Some(0.0);
            self.touch_hint = touch;
        }
    }
    pub fn update(&mut self, delta: Ticks) {
        if let Some(time) = &mut self.hint_time {
            *time += delta.as_f32();
            if *time >= 184.0 {
                self.hint_time = None;
            }
        }
    }
    pub fn render(&self, pass: &mut RenderPass, atlas: &Arc<CharacterTextures>) {
        let Some(time) = self.hint_time else { return };
        let opacity = (time / 32.0).min((184.0 - time) / 32.0).clamp(0.0, 1.0);
        let row = if self.touch_hint { 0.5 } else { 0.0 };
        // skip.png contains the authored B/A/ロングタップでスキップできます rows.
        QuadVertices::new()
            .with_box(16.0, 1000.0, 656.0, 1064.0)
            .with_tex_box(0.0, row, 1.0, row + 0.25)
            .with_color(FloatColor4::from_rgba(1.0, 1.0, 1.0, opacity).into_unorm())
            .render_sprite(
                pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Layer1),
                atlas.get("skip").as_source(),
                crate::render::top_left_projection_matrix(),
            );
    }
}

#[cfg(test)]
pub(super) fn authored_movie_entry(
    scenario: &shin_core::format::scenario::Scenario,
) -> (
    shin_core::format::save::GameDataEntry,
    shin_core::format::save::SaveData,
) {
    use super::{StartableCommand, VmState, command};
    use shin_core::{
        format::save::SaveData,
        vm::{
            Scripter,
            command::{CommandResult, RuntimeCommand, types::LayerType},
        },
    };
    let mut script = Scripter::new(scenario, 0, 42);
    let mut state = VmState::for_scenario(0, 42);
    let mut result = CommandResult::None;
    for _ in 0..500_000 {
        let next = script.run(result).unwrap();
        if matches!(&next, RuntimeCommand::LAYERLOAD(load) if load.layer_type == LayerType::Movie)
            && state.current_event.is_some()
            && state.system_controls_allowed
        {
            return (
                state.snapshot_entry(),
                SaveData {
                    settings: state.settings.clone(),
                    persist_data: state.persist.clone(),
                    save_vectors: state.save_vectors.clone(),
                    auto_save_slot: state.auto_save_slot.clone(),
                    ..Default::default()
                },
            );
        }
        result = match next {
            RuntimeCommand::SGET(get) => {
                let value = get.apply_state(&mut state);
                get.token.finish(value)
            }
            RuntimeCommand::SELECT(_) | RuntimeCommand::QUIZ(_) => {
                panic!("Interactive boundary before Episode1 opening movie")
            }
            cmd => {
                command::apply_command_state_ref(&cmd, &mut state).unwrap();
                cmd.execute_dummy()
                    .expect("Episode1 exits before its movie")
            }
        };
    }
    panic!("No authored skippable movie found")
}

#[cfg(test)]
pub(super) fn assert_reference_render_and_live<'a>(
    context: &mut crate::render::PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<crate::asset::system::AssetServer>,
) {
    use super::{Adv, ExecutingCommand, assets::AdvAssets, reconstruction::ReconstructedScene};
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
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Native event skip hint"),
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
                "Native ADV event hint",
            );
            pass.clear(Some(UnormColor::BLACK), None, None);
            live.render(&mut pass);
        }
        crate::layer::user::readback_test_texture(context, &target)
    };
    let blank = draw(&live, context);
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file("/adv.txa").unwrap())
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let atlas = shin_core::format::texture_archive::read_texture_archive(&bytes).unwrap();
    let art = &atlas.textures[atlas.name_to_index["skip"]];
    assert_eq!(art.dimensions(), (640, 256));
    for touch in [false, true] {
        for time in [0.0, 16.0, 32.0, 152.0, 168.0, 184.0] {
            live.event_skip = EventSkip::new();
            live.event_skip.show_hint(touch);
            live.event_skip.update(Ticks::from_f32(time));
            let pixels = draw(&live, context);
            let opacity = if time == 0.0 || time == 184.0 {
                0.0
            } else if time == 16.0 || time == 168.0 {
                128.0 / 255.0
            } else {
                1.0
            };
            let mut worst = 0.0f32;
            for y in 0..1080usize {
                for x in 0..1920usize {
                    let o = (y * 1920 + x) * 4;
                    if (16..656).contains(&x) && (1000..1064).contains(&y) {
                        let src = (((if touch { 128 } else { 0 }) + y - 1000) * 640 + x - 16) * 4;
                        let a = art.as_raw()[src + 3] as f32 / 255.0 * opacity;
                        for c in 0..3 {
                            let expected =
                                art.as_raw()[src + c] as f32 * a + blank[o + c] as f32 * (1.0 - a);
                            worst = worst.max((pixels[o + c] as f32 - expected).abs());
                        }
                    } else {
                        assert_eq!(
                            &pixels[o..o + 4],
                            &blank[o..o + 4],
                            "Outside authored hint rect{x},{y}"
                        );
                    }
                }
            }
            assert!(
                worst < 1.1,
                "Native event hint touch{touch} time{time} RGB error{worst}"
            );
            eprintln!(
                "Native event hint: touch{touch} time{time} full1920x1080 frame maxRGB{worst}"
            );
        }
    }
    let (entry, globals) = authored_movie_entry(&assets.scenario);
    let game = shin_core::format::save::GameData::from_entry(entry.clone());
    let canonical =
        ReconstructedScene::from_game_data_for_load(&assets.scenario, &game, &globals, 1_000_000)
            .unwrap();
    // The saved-worker restart prefix returns event checkpoints to the last
    // normal chapter. Entry56 is EVBEGIN, not a saved Movie descriptor.
    assert_eq!(entry.save_position, 56);
    assert_eq!(canonical.state.save_position, 55);
    assert_eq!(canonical.state.current_event, None);
    assert!(matches!(
        &canonical.stop,
        super::reconstruction::ReconstructionStop::Command(
            shin_core::vm::command::RuntimeCommand::MSGSET(_)
        )
    ));
    eprintln!(
        "Episode1 canonical load: saved{} -> dialogue{} pc{:x} route_bits{} route_first_response{}",
        entry.save_position,
        canonical.state.save_position,
        canonical.scripter.position().0,
        canonical.state.route.bit_position(),
        entry.route_data.reader().read_persistent().unwrap(),
    );
    let canonical_position = canonical.state.save_position;
    drop(canonical);
    let expected =
        ReconstructedScene::skip_event(&assets.scenario, &entry, &globals, 1_000_000).unwrap();
    let end_position = expected.state.save_position;
    eprintln!(
        "Authored Episode1 movie boundary{} -> end{}",
        entry.save_position, end_position
    );
    drop(expected);
    let mut frame = shin_core::primitives::update::FrameId::default();
    let mut step = |live: &mut Adv,
                    input: enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState>,
                    delta: f32| {
        use crate::update::UpdateContext;
        live.update(
            &mut UpdateContext {
                frame_id: frame,
                frame_started_at: std::time::Instant::now(),
                delta_ticks: Ticks::from_f32(delta),
                asset_server: server,
                pre_render: context,
            },
            input,
        );
        frame.advance();
    };
    for long_tap in [false, true] {
        live.event_skip = EventSkip::new();
        live.load_game_data(&globals, &game, server.clone())
            .unwrap();
        let started = std::time::Instant::now();
        while live.is_loading_saved_scene() {
            assert!(
                started.elapsed().as_secs() < 30,
                "Canonical dialogue load timed out: {:?}",
                live.saved_scene_load_error()
            );
            step(&mut live, Default::default(), 1.0);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(live.saved_scene_load_error().is_none());
        assert_eq!(live.vm_state.save_position, canonical_position);
        assert_eq!(live.vm_state.current_event, None);
        assert!(matches!(
            live.current_command,
            Some(ExecutingCommand::MSGSET(_))
        ));
        assert!(!live.adv_state.playback_modes.skip && !live.adv_state.playback_modes.autoplay);
        // Demonstrate why an unassisted movie wait is invalid: the canonical
        // dialogue remains open until real gameplay confirm input advances it.
        for _ in 0..8 {
            step(&mut live, Default::default(), 1.0);
            assert_eq!(live.vm_state.save_position, canonical_position);
            assert!(matches!(
                live.current_command,
                Some(ExecutingCommand::MSGSET(_))
            ));
        }
        let mut confirms = 0;
        while !matches!(live.current_command, Some(ExecutingCommand::MOVIEWAIT(_)))
            || live.is_loading_saved_scene()
        {
            assert!(
                started.elapsed().as_secs() < 30,
                "Movie after canonical dialogue timed out: saved{} pc{:x} {:?}, error{:?}",
                live.vm_state.save_position,
                live.scripter.position().0,
                live.current_command,
                live.saved_scene_load_error(),
            );
            let mut input =
                enum_map::EnumMap::<crate::app::AppAction, shin_input::ActionState>::default();
            if !live.is_loading_saved_scene()
                && live.vm_state.current_event.is_none()
                && matches!(live.current_command, Some(ExecutingCommand::MSGSET(_)))
            {
                input[crate::app::AppAction::Enter].is_clicked = true;
                confirms += 1;
                assert!(confirms <= 128, "bounded canonical pre-movie dialogue");
            }
            step(&mut live, input, 1.0);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(confirms > 0);
        eprintln!(
            "Episode1 canonical dialogue advanced: long_tap{long_tap} confirms{confirms} -> actual MOVIEWAIT"
        );
        assert!(live.vm_state.current_event.is_some());
        assert_eq!(live.vm_state.save_position, entry.save_position);
        // App::set_suspended -> SceneHost -> Adv uses this cancellation path.
        let p = Vec2::new(70.0, 1010.0);
        live.sample_modal_pointer(p, (1920, 1080), true, 0.0);
        step(&mut live, Default::default(), 0.0);
        live.sample_modal_pointer(p, (1920, 1080), true, 0.8);
        step(&mut live, Default::default(), 0.0);
        live.cancel_event_contact();
        live.sample_modal_pointer(p, (1920, 1080), true, 1.2);
        step(&mut live, Default::default(), 0.0);
        assert!(
            !live.is_loading_saved_scene(),
            "Suspension cancels an earlier hold"
        );
        live.sample_modal_pointer(p, (1920, 1080), false, 0.0);
        step(&mut live, Default::default(), 0.0);
        assert!(
            live.event_skip.hint_time.is_none(),
            "Cancelled release does not synthesize a tap"
        );
        let mut input =
            enum_map::EnumMap::<crate::app::AppAction, shin_input::ActionState>::default();
        if long_tap {
            let p = Vec2::new(70.0, 1010.0);
            live.sample_modal_pointer(p, (1920, 1080), true, 0.0);
            step(&mut live, Default::default(), 0.0);
            live.sample_modal_pointer(p, (1920, 1080), false, 0.1);
            step(&mut live, Default::default(), 0.0);
            assert!(live.event_skip.touch_hint && live.event_skip.hint_time.is_some());
            assert!(
                !live.is_loading_saved_scene(),
                "Short tap only reveals prompt"
            );
            live.sample_modal_pointer(p, (1920, 1080), true, 0.0);
            step(&mut live, Default::default(), 0.0);
            live.sample_modal_pointer(p, (1920, 1080), true, 1.151);
        } else {
            // The app maps controller B to Cancel and HideMessage.
            input[crate::app::AppAction::Cancel].is_clicked = true;
            input[crate::app::AppAction::HideMessage].is_clicked = true;
        }
        step(&mut live, input, 0.0);
        assert!(live.is_loading_saved_scene());
        let started = std::time::Instant::now();
        while live.pending_restore.is_some() {
            assert!(
                started.elapsed().as_secs() < 30,
                "Event-end restore timed out: {:?}",
                live.saved_scene_load_error()
            );
            step(&mut live, Default::default(), 1.0);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(live.saved_scene_load_error().is_none());
        assert_eq!(live.vm_state.current_event, None);
        assert_eq!(live.vm_state.save_position, end_position);
        assert!(
            live.adv_state.scene_load_fade.is_some(),
            "Native mode0 retains source during30tick reveal"
        );
        live.sample_modal_pointer(Vec2::ZERO, (1920, 1080), false, 0.0);
        eprintln!(
            "Live authored movie event skip: long_tap{long_tap}, real MOVIEWAIT, boundary{} -> {end_position}, the default audio output",
            entry.save_position
        );
    }
}
