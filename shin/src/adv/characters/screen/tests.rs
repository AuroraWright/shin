use super::*;
use shin_core::format::{
    scenario::{
        info::{
            CharsGridInfoItem, CharsGridSegment, CharsPortraitDisplayMode, CharsSpriteId,
            CharsSpriteInfoItem, CharsSpriteSegment,
        },
        instruction_elements::FromNumber,
    },
    text::U16String,
};
fn model() -> CharacterScreenModel {
    let mut sprites = Vec::new();
    for id in 0..2 {
        let mut segments = Vec::new();
        for state in 1..=3 {
            segments.push(CharsSpriteSegment::BeginState { index: state });
            segments.push(CharsSpriteSegment::Texts {
                name: U16String::new(format!("Name{id}/{state}")),
                description: U16String::new("Description"),
            });
            for variant in 0..2 {
                segments.push(CharsSpriteSegment::SpriteVariant {
                    variant_index: variant,
                    portrait_display_mode: CharsPortraitDisplayMode::Alive,
                    portrait_texture_name: U16String::new("p"),
                    full_texture_name: U16String::new("full"),
                });
            }
        }
        sprites.push(CharsSpriteInfoItem {
            episode: 0,
            segments,
        });
    }
    let portrait = |page, x, id| CharsGridSegment::Portrait {
        page,
        grid_x: x,
        grid_y: 1,
        character_id: CharsSpriteId::from_number(id),
        default_state: 1,
        default_variant: 0,
    };
    CharacterScreenModel::new(CharacterModel::new(
        &CharsGridInfoItem {
            segments: vec![portrait(0, 0, 0), portrait(0, 4, 1), portrait(3, 0, 0)],
        },
        &sprites.into_iter().collect(),
        0,
        0,
    ))
}
#[test]
fn opening_flash_page_scroll_and_close_suspend_at_native_boundaries() {
    let mut m = model();
    m.request_page(1);
    m.request_close();
    assert!(m.pending.is_empty());
    assert!(!m.closing_requested);
    m.update(Ticks::from_f32(10.0));
    assert!(m.transition.is_open());
    m.request_page(1);
    m.update(Ticks::from_f32(3.5));
    assert_eq!(m.characters.page, 0);
    assert!((m.button_flashes[1].value() - 0.7).abs() < 1e-6);
    m.update(Ticks::from_f32(3.5));
    assert_eq!(m.characters.page, 0);
    m.update(Ticks::ZERO);
    assert_eq!(m.characters.page, 3);
    assert_eq!(m.page_scroll.value(), 0.0);
    m.update(Ticks::from_f32(5.0));
    assert!((m.page_scroll.value() - 400.0 * std::f32::consts::FRAC_1_SQRT_2).abs() < 0.001);
    m.navigate(Vec2::X);
    assert_eq!(m.characters.selected_index(), Some(2));
    m.update(Ticks::from_f32(5.0));
    assert_eq!(m.page_scroll.value(), 400.0);
    m.request_close();
    m.update(Ticks::from_f32(6.0));
    assert_eq!(m.transition.progress(), 1.0);
    assert!(!m.is_closed());
    m.update(Ticks::from_f32(1.0));
    assert_eq!(m.transition.progress(), 1.0);
    m.update(Ticks::ZERO);
    m.update(Ticks::from_f32(9.0));
    assert!(!m.is_closed());
    m.update(Ticks::from_f32(1.0));
    assert!(m.is_closed());
}
#[test]
fn idle_action_dispatches_before_new_controller_flash_and_only_once_per_frame() {
    let mut m = model();
    m.update(Ticks::from_f32(10.0));
    m.request_state(1);
    m.update(Ticks::from_f32(7.0));
    assert_eq!(
        m.characters
            .selected_cell()
            .unwrap()
            .definition()
            .state()
            .unwrap()
            .index,
        1
    );
    let mut input: enum_map::EnumMap<crate::app::AppAction, shin_input::ActionState> =
        enum_map::EnumMap::default();
    input[crate::app::AppAction::CharacterState].is_clicked = true;
    m.handle_controller_input(&input, 0.0);
    assert_eq!(
        m.characters
            .selected_cell()
            .unwrap()
            .definition()
            .state()
            .unwrap()
            .index,
        2
    );
    assert_eq!(m.pending.len(), 1);
    m.update(Ticks::ZERO);
    assert_eq!(
        m.characters
            .selected_cell()
            .unwrap()
            .definition()
            .state()
            .unwrap()
            .index,
        2
    );
    assert_eq!(m.pending.len(), 1);
}
#[test]
fn close_task_starts_after_flash_poll_and_defers_slide_even_for_large_delta() {
    let mut m = model();
    m.update(Ticks::from_f32(10.0));
    m.request_close();
    m.update(Ticks::from_f32(7.0));
    assert!(m.transition.is_open());
    m.update(Ticks::from_f32(100.0));
    assert!(!m.transition.is_open());
    assert_eq!(m.transition.progress(), 1.0);
    assert!(!m.is_closed());
    m.update(Ticks::from_f32(10.0));
    assert!(m.is_closed());
}
#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_native_character_async_sprite_swap() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    for (a, value) in [
        (0x50d18, 0x9402bce6),
        (0x100300, 0xf85f8ee0),
        (0x50e24, 0x4ea81d00),
        (0x56074, 0x9401965f),
        (0x56078, 0x360000a0),
        (0x56090, 0x97fff66c),
        (0x53a58, 0xb90003ff),
        (0x53a78, 0xd63f0100),
        (0x53ad0, 0x9402b168),
        (0x56180, 0xbd400020),
        (0x56194, 0x1e202820),
        (0x561c4, 0xd63f0100),
        (0x52d04, 0x3903000b),
        (0x52d34, 0xf9427c0b),
        (0x52d44, 0x8b091108),
        (0x52d4c, 0xf9400508),
        (0x52d54, 0x9b0b2188),
        (0x52d58, 0x9b092148),
        (0x52e5c, 0x97ffb6f5),
        (0x52e64, 0x91032268),
        (0x52e68, 0xc85ffd13),
        (0x52e6c, 0xc809fd00),
        (0x50ec0, 0x39430268),
        (0x50ec4, 0x340006c8),
        (0x50ec8, 0x91032268),
        (0x50edc, 0xf9402508),
        (0x50ee0, 0xd63f0100),
        (0x50ee4, 0x360005c0),
        (0x50f34, 0x94031893),
        (0x50f3c, 0x91034268),
        (0x50f44, 0xc809fd00),
        (0x50f5c, 0x9401f259),
        (0x50f98, 0x3903027f),
    ] {
        assert_eq!(word(a), value, "Native sprite request/swap{a:x}");
    }
}

#[test]
fn pending_sprite_retains_display_and_superseded_result_cannot_swap() {
    use crate::{
        adv::characters::view::CharacterTextureBuilder,
        asset::texture_archive::TextureArchiveBuilder,
    };
    crate::init_test_task_pools();
    let shown = Arc::new(CharacterTextureBuilder::new().build());
    let superseded = Arc::new(CharacterTextureBuilder::new().build());
    let latest = Arc::new(CharacterTextureBuilder::new().build());
    let (old_tx, old_rx) = futures::channel::oneshot::channel();
    let mut screen = CharacterScreen {
        model: model(),
        atlas: shown.clone(),
        system: shown.clone(),
        text: None,
        full_sprite: Some(shown.clone()),
        pending_full_sprite: Some(shin_tasks::async_io::spawn(
            async move { old_rx.await.unwrap() },
        )),
        text_revision: 0,
        sprite_revision: 0,
        pointer: Default::default(),
    };
    screen.poll_full_sprite();
    assert!(Arc::ptr_eq(screen.full_sprite.as_ref().unwrap(), &shown));
    old_tx.send(Ok(superseded)).unwrap();
    let (latest_tx, latest_rx) = futures::channel::oneshot::channel();
    screen.pending_full_sprite = Some(shin_tasks::async_io::spawn(async move {
        latest_rx.await.unwrap()
    }));
    screen.poll_full_sprite();
    assert!(Arc::ptr_eq(screen.full_sprite.as_ref().unwrap(), &shown));
    latest_tx.send(Ok(latest.clone())).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while screen.pending_full_sprite.is_some() {
        screen.poll_full_sprite();
        assert!(
            std::time::Instant::now() < deadline,
            "Async sprite did not finish"
        );
        std::thread::yield_now();
    }
    assert!(Arc::ptr_eq(screen.full_sprite.as_ref().unwrap(), &latest));
    screen.poll_full_sprite();
    assert!(Arc::ptr_eq(screen.full_sprite.as_ref().unwrap(), &latest));
}

#[test]
fn controller_priority_consumes_direction_and_uses_continuous_scroll() {
    use crate::app::AppAction as A;
    let mut m = model();
    m.update(Ticks::from_f32(10.0));
    m.text_scroll.set_maximum(200.0);
    let mut input = enum_map::EnumMap::<A, shin_input::ActionState>::default();
    let click = shin_input::ActionState {
        is_clicked: true,
        is_clicked_or_repeated: true,
        ..Default::default()
    };
    input[A::Right] = click;
    input[A::CharacterState] = click;
    input[A::Cancel] = click;
    m.handle_controller_input(&input, 0.75);
    assert_eq!(m.characters.selected_index(), Some(1));
    assert_eq!(m.take_sounds(), ["move"]);
    assert_eq!(m.text_scroll.position, 0.0);
    input[A::Left] = click;
    m.handle_controller_input(&input, 0.75);
    assert_eq!(m.characters.selected_index(), Some(0));
    assert_eq!(m.take_sounds(), ["move"]);
    input[A::Right] = Default::default();
    input[A::Left] = Default::default();
    input[A::CharacterVariant] = click;
    m.handle_controller_input(&input, 0.75);
    assert_eq!(m.take_sounds(), ["decide"]);
    assert!(!m.closing_requested);
    input = Default::default();
    m.handle_controller_input(&input, 0.75);
    assert_eq!(m.text_scroll.position, 12.0);
    m.handle_controller_input(&input, -0.25);
    assert_eq!(m.text_scroll.position, 8.0);
    input[A::CharacterPageDown] = click;
    input[A::Right] = click;
    m.handle_controller_input(&input, 0.75);
    assert_eq!(m.take_sounds(), ["move"]);
    assert_eq!(m.characters.selected_index(), Some(0));
    assert_eq!(m.text_scroll.position, 8.0);
    assert!(matches!(
        m.pending.last().unwrap().action,
        PendingAction::Page(3)
    ));
}

#[test]
fn edits_retain_captured_targets_and_variant_keeps_text_scroll() {
    let mut m = model();
    m.update(Ticks::from_f32(10.0));
    m.request_state(1);
    m.navigate(Vec2::X);
    assert_eq!(m.characters.selected_index(), Some(1));
    m.update(Ticks::from_f32(7.0));
    m.update(Ticks::ZERO);
    assert_eq!(m.characters.cells[0].definition().state().unwrap().index, 2);
    assert_eq!(m.characters.cells[1].definition().state().unwrap().index, 1);
    m.text_scroll.position = 123.0;
    let revision = m.text_revision;
    m.request_variant();
    m.update(Ticks::from_f32(7.0));
    m.update(Ticks::ZERO);
    assert_eq!(
        m.characters
            .selected_cell()
            .unwrap()
            .definition()
            .state()
            .unwrap()
            .variant()
            .unwrap()
            .index,
        1
    );
    assert_eq!(m.text_scroll.position, 123.0);
    assert_eq!(m.text_revision, revision);
    m.request_state(1);
    m.update(Ticks::from_f32(7.0));
    m.update(Ticks::ZERO);
    assert_eq!(m.text_scroll.position, 0.0);
    m.request_state(1);
    m.update(Ticks::from_f32(7.0));
    m.update(Ticks::ZERO);
    assert_eq!(
        m.characters.button_states()[2],
        CharacterButtonState::Disabled
    );
    m.request_button(CharacterButton::Execute);
    assert!(m.pending.is_empty());
    m.request_state(1);
    m.update(Ticks::from_f32(7.0));
    m.update(Ticks::ZERO);
    assert_eq!(
        m.characters
            .selected_cell()
            .unwrap()
            .definition()
            .state()
            .unwrap()
            .index,
        1
    );
}
#[test]
fn concurrent_variant_task_uses_state_selected_at_completion() {
    let mut m = model();
    m.update(Ticks::from_f32(10.0));
    m.request_state(1);
    m.request_variant();
    m.update(Ticks::from_f32(7.0));
    m.update(Ticks::ZERO);
    let definition = m.characters.selected_cell().unwrap().definition();
    assert_eq!(definition.state().unwrap().index, 2);
    assert_eq!(definition.state().unwrap().variant().unwrap().index, 0);
    assert_eq!(definition.states[0].variant().unwrap().index, 1);
    // Reversing insertion order makes the state task run first; Variant
    // then addresses that newly selected state at completion.
    let mut m = model();
    m.update(Ticks::from_f32(10.0));
    m.request_variant();
    m.request_state(1);
    m.update(Ticks::from_f32(7.0));
    m.update(Ticks::ZERO);
    let definition = m.characters.selected_cell().unwrap().definition();
    assert_eq!(definition.state().unwrap().index, 2);
    assert_eq!(definition.state().unwrap().variant().unwrap().index, 1);
    assert_eq!(definition.states[0].variant().unwrap().index, 0);
}

#[test]
fn selection_indicator_preserves_partial_values_when_direction_reverses() {
    let mut m = model();
    m.update(Ticks::from_f32(10.0));
    assert_eq!(m.grid_effects().1, [1.0, 0.0, 1.0]);
    m.navigate(Vec2::X);
    assert_eq!(m.grid_effects().1, [1.0, 0.0, 1.0]);
    m.update(Ticks::from_f32(2.0));
    let selected = m.grid_effects().1;
    assert!((selected[0] - 0.8).abs() < 1e-6);
    assert!((selected[1] - 0.5).abs() < 1e-6);
    assert_eq!(selected[2], 1.0);
    m.navigate(-Vec2::X);
    m.update(Ticks::from_f32(1.0));
    let selected = m.grid_effects().1;
    assert_eq!(selected[0], 1.0);
    assert!((selected[1] - 0.4).abs() < 1e-6);
    m.update(Ticks::from_f32(4.0));
    assert_eq!(m.grid_effects().1, [1.0, 0.0, 1.0]);
    m.pointer(glam::vec2(205.0, 230.0), true);
    m.update(Ticks::from_f32(1.75));
    let (_, selected, clicked) = m.grid_effects();
    assert!((selected[0] - 0.825).abs() < 1e-6);
    assert!((selected[1] - 0.4375).abs() < 1e-6);
    assert!((clicked[1] - 0.35).abs() < 1e-6);
}
#[test]
fn page_palette_waits_for_flash_then_interpolates_linearly_with_scroll() {
    let mut m = model();
    m.update(Ticks::from_f32(10.0));
    m.request_page(1);
    m.update(Ticks::from_f32(3.5));
    assert_eq!(m.grid_effects().0, [166.0 / 255.0, 0.0, 0.0]);
    m.update(Ticks::from_f32(3.5));
    assert_eq!(m.characters.page, 0);
    m.update(Ticks::ZERO);
    assert_eq!(m.characters.page, 3);
    assert_eq!(m.grid_effects().0, [166.0 / 255.0, 0.0, 0.0]);
    m.update(Ticks::from_f32(5.0));
    assert_eq!(m.grid_effects().0, [83.0 / 255.0, 0.0, 0.0]);
    assert!((m.page_scroll.value() - 400.0 * std::f32::consts::FRAC_1_SQRT_2).abs() < 0.001);
    m.update(Ticks::from_f32(5.0));
    assert_eq!(m.grid_effects().0, [0.0, 0.0, 0.0]);
}

#[test]
fn sounds_follow_native_action_origin_and_ignore_unavailable_actions() {
    let mut m = model();
    m.request_state(1);
    m.request_close();
    assert!(m.take_sounds().is_empty());
    m.update(Ticks::from_f32(10.0));
    m.navigate(Vec2::X);
    m.pointer(glam::vec2(100.0, 220.0), true);
    assert_eq!(m.take_sounds(), ["move", "decide"]);
    m.request_state(1);
    assert_eq!(m.take_sounds(), ["execute"]);
    m.update(Ticks::from_f32(7.0));
    m.update(Ticks::ZERO);
    m.request_state(-1);
    assert_eq!(m.take_sounds(), ["resurrect"]);
    m.update(Ticks::from_f32(7.0));
    m.update(Ticks::ZERO);
    m.request_character_cycle();
    assert!(m.take_sounds().is_empty());
    m.request_variant();
    m.request_page(1);
    assert_eq!(m.take_sounds(), ["decide", "move"]);
    m.update(Ticks::from_f32(17.0));
    m.update(Ticks::from_f32(10.0));
    m.request_page(1);
    assert!(m.take_sounds().is_empty());
    m.request_close();
    assert_eq!(m.take_sounds(), ["cancel"]);
    m.request_close();
    assert!(m.take_sounds().is_empty());
    let mut pointer = model();
    pointer.update(Ticks::from_f32(10.0));
    pointer.pointer(glam::vec2(1800.0, 64.0), true);
    assert_eq!(pointer.take_sounds(), ["decide"]);
}
#[test]
fn text_drag_axis_and_inertia_follow_native_fractional_tick_rules() {
    let mut s = CharacterTextScroll::default();
    s.set_maximum(200.0);
    s.axis(6.25);
    assert_eq!(s.position, 100.0);
    s.drag(-20.0);
    assert_eq!(s.position, 120.0);
    s.release(-8.0);
    s.update(Ticks::from_f32(2.5));
    assert_eq!(s.position, 140.0);
    assert!((s.velocity + 8.0 * 0.95f32.powf(2.5)).abs() < 1e-6);
    let before = s.position;
    s.update(Ticks::from_f32(0.5));
    assert!((s.position - before - 4.0 * 0.95f32.powf(2.5)).abs() < 1e-5);
    s.axis(10.0);
    assert_eq!(s.position, 200.0);
    assert_eq!(s.velocity, 0.0);
    s.axis(-100.0);
    assert_eq!(s.position, 0.0);
    s.drag(-10.0);
    s.release(20.0);
    s.update(Ticks::from_f32(1.0));
    assert_eq!(s.position, 0.0);
    assert_eq!(s.velocity, 19.0);
    s.update(Ticks::from_f32(1.0));
    assert_eq!(s.velocity, 0.0);
    s.axis(1.0);
    s.release(0.099);
    s.update(Ticks::from_f32(1.0));
    assert!((s.position - 15.901).abs() < 1e-5);
    assert_eq!(s.velocity, 0.0);
    s.release(3.0);
    s.update(Ticks::ZERO);
    assert_eq!(s.velocity, 0.0);
    s.reset();
    s.set_maximum(0.0);
    s.axis(10.0);
    s.drag(-100.0);
    s.release(-30.0);
    s.update(Ticks::from_f32(10.0));
    assert_eq!(s.position, 0.0);
    assert_eq!(s.velocity, 0.0);
}
#[test]
fn trackpad_wheel_scrolls_character_description_without_switching_pages() {
    let mut screen = model();
    screen.update(Ticks::from_f32(10.0));
    screen.text_scroll.set_maximum(500.0);
    for (amount, expected) in [(-3.25, 52.0), (-0.125, 54.0), (3.25, 2.0), (0.0, 2.0)] {
        let (input, scroll) = crate::app::wheel_scroll_test_input(amount);
        screen.handle_controller_input(&input, scroll);
        assert_eq!(screen.text_scroll.position, expected);
        assert!(screen.take_sounds().is_empty());
    }
}
#[test]
#[ignore = "requires supplied executable next to SHIN_REFERENCE_ROM"]
fn reference_native_character_sounds_and_scroll() {
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&path)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
    let rela: std::collections::HashMap<u64, u64> = exe[0x11e040 + 256..0x152eb8 + 256]
        .chunks_exact(24)
        .map(|r| {
            (
                u64::from_le_bytes(r[..8].try_into().unwrap()),
                u64::from_le_bytes(r[16..].try_into().unwrap()),
            )
        })
        .collect();
    for (got, name) in [
        (0x2246e0, "decide"),
        (0x224778, "cancel"),
        (0x224780, "move"),
        (0x224ad8, "move"),
        (0x224ae0, "resurrect"),
        (0x224ae8, "execute"),
    ] {
        let pointer = rela[&rela[&got]] as usize + 256;
        assert_eq!(&exe[pointer..pointer + name.len()], name.as_bytes());
        assert_eq!(exe[pointer + name.len()], 0);
    }
    // Selection/click draw weights and speed-based transitions; page RGB tweens.
    for (a, opcode) in [
        (0x4ecbc, 0x9401b189u32),
        (0x51ba4, 0x91002360u32),
        (0x51bac, 0x1e2c1001u32),
        (0x51bb0, 0x1e21080bu32),
        (0x51bbc, 0x91018360u32),
        (0x517bc, 0x4eab1d63u32),
        (0x5185c, 0x4eab1d63u32),
        (0x514c0, 0x910fc260u32),
        (0x514c8, 0x91112260u32),
        (0x514d4, 0x91128260u32),
        (0x53e44, 0xbd457101u32),
        (0x53e54, 0x94019eebu32),
        (0x53e74, 0x1e2a1001u32),
        (0x53e84, 0x94019edfu32),
        (0x54b08, 0xbd457101u32),
        (0x54b18, 0x94019bbau32),
        (0x54b38, 0x1e2a1001u32),
        (0x54b48, 0x94019baeu32),
        (0x55820, 0x2a1f03e1u32),
        (0x55834, 0x94019873u32),
        (0x5584c, 0x2a1f03e1u32),
        (0x55854, 0x9401986bu32),
        (0x5586c, 0x2a1f03e1u32),
        (0x55874, 0x94019863u32),
    ] {
        assert_eq!(word(a), opcode, "native indicator{a:x}");
    }
    // InputArea physical-to-canvas scale, movement and release thresholds,
    // tick-time conversion and last-displacement sampling.
    for (a, opcode) in [
        (0x6b000, 0x1e211800u32),
        (0x6b018, 0x1e211800u32),
        (0x6b1ec, 0x1e601940u32),
        (0x6b234, 0x0b202908u32),
        (0x6b238, 0x531f7908u32),
        (0x6b240, 0x1e2b1800u32),
        (0x6b248, 0x54001b2du32),
        (0x6b25c, 0x1e2c8800u32),
        (0x6b260, 0x1e2d8821u32),
        (0x6b594, 0x2d4b0660u32),
        (0x6b598, 0x2d410e62u32),
        (0x6b718, 0x1e21c008u32),
        (0x6b734, 0x531d7108u32),
        (0x6b744, 0x1e211800u32),
        (0x6b74c, 0x540001adu32),
        (0x6b774, 0x1e212000u32),
        (0x6b778, 0x5400056cu32),
        (0x6b84c, 0xd63f0100u32),
        (0x6b854, 0x2d0b326du32),
        (0x6b85c, 0x2d0c2a6bu32),
        (0x6b864, 0xfd002669u32),
        (0xfba70, 0x5280a000u32),
        (0xfba80, 0x52805a00u32),
    ] {
        assert_eq!(word(a), opcode, "native pointer{a:x}");
    }
    for (a, value) in [(0x164d08, 0.016683333333333335f64), (0x164cf8, 0.000001)] {
        assert_eq!(
            u64::from_le_bytes(exe[a + 256..a + 264].try_into().unwrap()),
            value.to_bits()
        );
    }
    for (a, v) in [
        (0x1595d8, 0.95f32),
        (0x159570, 0.1),
        (0x159ab8, f32::EPSILON),
    ] {
        assert_eq!(word(a), v.to_bits());
    }
    for (a, opcode) in [
        (0x53004, 0xf9456d08u32),
        (0x53018, 0x97ff50a6u32),
        (0x531c8, 0xf9437108u32),
        (0x531dc, 0x97ff5035u32),
        (0x53478, 0xf9457108u32),
        (0x534c0, 0xf9457508u32),
        (0x534cc, 0x97ff4f79u32),
        (0x53658, 0xf9437108u32),
        (0x53668, 0x97ff4f12u32),
        (0x53e0c, 0xf943c108u32),
        (0x53e1c, 0x97ff4d25u32),
        (0x53f70, 0xf943bd08u32),
        (0x53f7c, 0x97ff4ccdu32),
        (0x549d0, 0xf9437108u32),
        (0x549dc, 0x97ff4a35u32),
        (0x54a7c, 0xf9437108u32),
        (0x54a8c, 0x97ff4a09u32),
        (0x50d3c, 0x1e280800u32),
        (0x50d40, 0x1e203822u32),
        (0x50d84, 0x1e222020u32),
        (0x50d88, 0x540001e4u32),
        (0x50da4, 0x94032fa7u32),
        (0x50da8, 0x1e200920u32),
        (0x50dbc, 0x1e212000u32),
        (0x50dc4, 0xb901367fu32),
        (0x54cec, 0x1e293901u32),
        (0x54cf0, 0x1e213801u32),
        (0x54d70, 0xbd013401u32),
        (0x53f24, 0xb901367fu32),
    ] {
        assert_eq!(word(a), opcode, "native character sound/scroll{a:x}");
    }
}

#[test]
fn restarting_flash_preserves_current_value_and_converts_speed_to_duration() {
    let mut t = Tweener::new(0.0);
    flash(&mut t);
    t.update(Ticks::from_f32(1.75));
    assert!((t.value() - 0.35).abs() < 1e-6);
    flash(&mut t);
    assert!((t.value() - 0.35).abs() < 1e-6);
    t.update(Ticks::from_f32(1.75));
    assert!((t.value() - 0.7).abs() < 1e-6);
    t.update(Ticks::from_f32(3.5));
    assert!(t.is_idle());
    assert_eq!(t.value(), 0.0);
}
