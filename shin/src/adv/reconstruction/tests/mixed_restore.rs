// Real ROM resources and desktop task pools. Publication and active Movie
// preparation gates do not claim native concurrent IO scheduling equivalence.
mod post_install_failure;
mod scene_load_errors;
pub(super) use post_install_failure::verify_post_install_failure;
pub(super) use scene_load_errors::verify_scene_load_errors;
use super::*;
use crate::{
    adv::{Adv, AdvAssets, restoration::LoadedScene},
    asset::{bustup::Bustup, picture::Picture, system::AssetServer},
    layer::{
        DrawableLayer,
        user::{ReferenceMovieOwner, UserLayer},
    },
    render::PreRenderContext,
    update::UpdateContext,
};
use shin_core::{
    format::{
        save::GameData,
        scenario::{
            instruction_elements::{MessageId, U8Bool},
            types::U16FixupString,
        },
    },
    primitives::update::FrameId,
    time::Ticks,
    vm::command::types::{LayerbankId, PlaneId},
};
use std::{
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

enum Owner {
    Picture(Weak<Picture>),
    Bustup(Weak<Bustup>),
    Movie(ReferenceMovieOwner, Weak<Picture>),
}
impl Owner {
    fn of(layer: &UserLayer) -> Self {
        match layer {
            UserLayer::Picture(p) => Self::Picture(p.reference_resource_owner()),
            UserLayer::Bustup(b) => Self::Bustup(b.reference_resource_owner()),
            UserLayer::Movie(m) => {
                Self::Movie(m.reference_publication_owner(), m.reference_still_owner())
            }
            _ => panic!("Expected a real mixed resource"),
        }
    }
    fn assert_live(&self, layer: &UserLayer) {
        match (self, layer) {
            (Self::Picture(w), UserLayer::Picture(p)) => {
                assert!(w.upgrade().is_some());
                assert_eq!(w.as_ptr() as usize, p.reference_resource_identity());
            }
            (Self::Bustup(w), UserLayer::Bustup(b)) => {
                assert!(w.upgrade().is_some());
                assert_eq!(w.as_ptr() as usize, b.reference_resource_identity());
            }
            (Self::Movie(w, still), UserLayer::Movie(m)) => {
                assert_eq!(w.identity(), m.reference_publication_owner().identity());
                assert!(w.owners() > 0);
                assert!(still.upgrade().is_some());
                assert_eq!(still.as_ptr() as usize, m.reference_still_state().0);
            }
            _ => panic!("Publication changed layer kind"),
        }
    }
    fn assert_released(&self) {
        match self {
            Self::Picture(w) => assert!(w.upgrade().is_none(), "Picture owner leaked"),
            Self::Bustup(w) => assert!(w.upgrade().is_none(), "Bustup owner leaked"),
            Self::Movie(w, still) => {
                w.assert_released();
                assert!(still.upgrade().is_none(), "Linked still owner leaked");
            }
        }
    }
}
fn owners(scene: &LoadedScene) -> Vec<(LayerbankId, Owner)> {
    scene
        .reference_layers()
        .iter()
        .map(|(b, l)| (*b, Owner::of(l)))
        .collect()
}
fn n<T>(value: i32) -> NumberSpec<T> {
    NumberSpec::new(NumberSpec::constant(value).into_untyped())
}
fn source(original: &Scenario, late_failure: bool, invalid: Option<i32>) -> Arc<Scenario> {
    let movie = original
        .info_tables()
        .movie_info
        .iter()
        .position(|m| m.name.as_str().eq_ignore_ascii_case("op1"))
        .unwrap();
    let mut bytes = original.raw().to_vec();
    if late_failure {
        // The first sized table follows the 36-byte header. Change only its
        // first name, preserving length, so the genuine final mask read fails.
        let table = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
        bytes[table + 10] = b'!';
    }
    let offset = bytes.len();
    bytes[32..36].copy_from_slice(&(offset as u32).to_le_bytes());
    let mut cursor = std::io::Cursor::new(&mut bytes);
    cursor.set_position(offset as u64);
    let register = "$v0".parse().unwrap();
    let register_id = matches!(invalid, Some(i32::MIN | i32::MAX));
    if register_id {
        use shin_core::format::scenario::instructions::{
            BinaryOperation, BinaryOperationType, UnaryOperation, UnaryOperationType,
        };
        // NumberSpec constants have a 28-bit range. Produce the full-width
        // rejected ID through the real VM, then pass its register to loaders.
        Instruction::bo(BinaryOperation {
            ty: BinaryOperationType::LeftShift,
            destination: register,
            left: NumberSpec::constant(1),
            right: NumberSpec::constant(31),
        })
        .write_le(&mut cursor)
        .unwrap();
        if invalid == Some(i32::MAX) {
            Instruction::uo(UnaryOperation {
                ty: UnaryOperationType::Not16,
                destination: register,
                source: NumberSpec::register(register),
            })
            .write_le(&mut cursor)
            .unwrap();
        }
    }
    // Preserve real metadata, isolate a sequence with three save boundaries.
    for (index, (kind, id)) in [(2, 1), (3, 0), (6, movie as i32)].into_iter().enumerate() {
        let mut params = std::io::Cursor::new(vec![255u8]);
        params.set_position(1);
        let id = match invalid {
            Some(0) => match kind {
                2 => original.info_tables().picture_info.len() as i32,
                3 => original.info_tables().bustup_info.len() as i32,
                6 => original.info_tables().movie_info.len() as i32,
                _ => unreachable!(),
            },
            Some(value) => value,
            None => id,
        };
        for (parameter, value) in [id, 1000, 0, 0, 0, 0, 0, 0].into_iter().enumerate() {
            let number = if register_id && parameter == 0 {
                NumberSpec::register(register)
            } else {
                NumberSpec::constant(value)
            };
            number.write_le(&mut params).unwrap();
        }
        params.set_position(0);
        let params =
            shin_core::format::scenario::instruction_elements::BitmaskNumberArray::read_le(
                &mut params,
            )
            .unwrap();
        for cmd in [
            C::LAYERLOAD(c::LAYERLOAD {
                layer_id: n(index as i32 + 1),
                layer_type: n(kind),
                flags: n(0),
                params,
            }),
            C::MSGSET(c::MSGSET {
                msg_id: MessageId(index as u32 + 1),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("@rMixed saved resource boundary"),
            }),
        ] {
            Instruction::Command(cmd).write_le(&mut cursor).unwrap();
        }
    }
    if late_failure {
        for cmd in [
            C::MASKLOAD(c::MASKLOAD {
                mask_id: n(0),
                mask_flags: n(0),
                smth_smth_transition: n(1),
            }),
            C::MSGSET(c::MSGSET {
                msg_id: MessageId(4),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("@rFailing mask boundary"),
            }),
        ] {
            Instruction::Command(cmd).write_le(&mut cursor).unwrap();
        }
    }
    Instruction::Command(C::EXIT(c::EXIT {
        arg1: 0,
        arg2: n(0),
    }))
    .write_le(&mut cursor)
    .unwrap();
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    let scenario = Arc::new(Scenario::new(bytes.into()).unwrap());
    if late_failure {
        assert!(
            scenario.info_tables().mask_info[0]
                .name
                .as_str()
                .starts_with('!')
        );
    }
    scenario
}
fn source_with_movie_link(original: &Scenario, linked: i16) -> Arc<Scenario> {
    use shin_core::format::{scenario::info::MovieInfoItem, text::U16String};
    let source = source(original, false, None);
    let movie = source.info_tables().movie_info.iter()
        .position(|item| item.name.as_str().eq_ignore_ascii_case("op1")).unwrap();
    let mut bytes = source.raw().to_vec();
    // The sixth table pointer is MovieInfo. Walk its actual variable-length
    // entries to edit just the signed16-bit cross-reference in supplied SNR.
    let table = u32::from_le_bytes(bytes[56..60].try_into().unwrap()) as usize;
    let mut cursor = std::io::Cursor::new(&bytes);
    cursor.set_position((table + 8) as u64);
    for _ in 0..movie { MovieInfoItem::read_le(&mut cursor).unwrap(); }
    U16String::read_le(&mut cursor).unwrap();
    let at = cursor.position() as usize;
    bytes[at..at + 2].copy_from_slice(&linked.to_le_bytes());
    let source = Arc::new(Scenario::new(bytes.into()).unwrap());
    assert_eq!(source.info_tables().movie_info[movie].linked_picture_id.into_i32(), i32::from(linked));
    source
}
fn step<'a>(
    live: &mut Adv,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    frame: &mut FrameId,
) {
    let input = enum_map::EnumMap::from_fn(|action| shin_input::ActionState {
        is_clicked: matches!(action, crate::app::AppAction::Enter),
        is_clicked_or_repeated: matches!(action, crate::app::AppAction::Enter),
        is_held: matches!(action, crate::app::AppAction::Enter),
        ..Default::default()
    });
    live.update(
        &mut UpdateContext {
            frame_started_at: Instant::now(),
            frame_id: *frame,
            delta_ticks: Ticks::from_f32(1.0),
            asset_server: server,
            pre_render: context,
        },
        input,
    );
    frame.advance();
    let encoder = std::mem::replace(
        context.encoder,
        context.device.create_command_encoder(&Default::default()),
    );
    let mut upload = context.device.create_command_encoder(&Default::default());
    context.dynamic_buffer.finish(&mut upload);
    context.queue.submit([upload.finish(), encoder.finish()]);
    context.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU work must complete before readback or retirement");
}
fn finish<'a>(
    live: &mut Adv,
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    frame: &mut FrameId,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while live.is_loading_saved_scene() {
        assert!(Instant::now() < deadline, "Mixed restoration timed out");
        step(live, context, server, frame);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(live.saved_scene_load_error().is_none());
}
pub(super) fn verify<'a>(
    context: &mut PreRenderContext<'a, '_, '_, '_>,
    server: &'a Arc<AssetServer>,
    assets: &AdvAssets,
    original: &Arc<Scenario>,
) {
    let scenario = source(original, false, None);
    let mut assets = assets.clone();
    assets.scenario = scenario.clone();
    let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
    let globals = SaveData::default();
    let game = |boundary| {
        GameData::from_entry(GameDataEntry {
            scenario_id: 0,
            random_seed: 0,
            save_position: boundary,
            route_data: RouteRecorder::default().snapshot(),
        })
    };
    let mut live = Adv::new(
        audio.clone(),
        server.clone(),
        assets.clone(),
        Scripter::new(&scenario, 0, 0),
    );
    let mut frame = FrameId::default();
    // Publish exact prepared assets, then clear all banks and verify release.
    live.load_game_data(&globals, &game(3), server.clone())
        .unwrap();
    let task = live.pending_restore.as_mut().unwrap().task.take().unwrap();
    let (release, gate) = futures::channel::oneshot::channel();
    let (ready, received) = std::sync::mpsc::channel();
    live.pending_restore.as_mut().unwrap().task =
        Some(shin_tasks::async_io::spawn(async move {
            let result = task.await?;
            ready.send(owners(&result.0)).unwrap();
            gate.await
                .map_err(|_| anyhow::anyhow!("Mixed publication gate retired"))?;
            Ok(result)
        }));
    let prepared = received.recv_timeout(Duration::from_secs(15)).unwrap();
    assert_eq!(prepared.len(), 3);
    let position = live.scripter.position();
    for _ in 0..3 {
        step(&mut live, context, server, &mut frame);
        assert_eq!(
            live.scripter.position(),
            position,
            "Pending input advanced the old VM"
        );
        assert!(
            live.adv_state
                .plane_layer_group(PlaneId::new(0))
                .get_used_layerbank_ids()
                .is_empty()
        );
    }
    release.send(()).unwrap();
    finish(&mut live, context, server, &mut frame);
    assert_eq!(live.vm_state.save_position, 3);
    for (bank, owner) in &prepared {
        owner.assert_live(
            &live.adv_state
                .plane_layer_group(PlaneId::new(0))
                .get_layer(*bank)
                .unwrap(),
        );
    }
    live.load_game_data(&globals, &game(0), server.clone())
        .unwrap();
    finish(&mut live, context, server, &mut frame);
    for (_, owner) in &prepared {
        owner.assert_released();
    }
    eprintln!(
        "Mixed Picture/Bustup/Movie+linked still: prepared identity, pending input gate, publication and replacement retirement pass"
    );
    // Cancel the actual Adv loader while its CPU job is active, both before
    // and after real FFmpeg/Metal/audio player creation. The helper only
    // releases gates; retirement runs through the normal caller on this thread.
    for after_preparation in [false, true] {
        for shutdown in [false, true] {
            use crate::layer::user::{ReferencePreparationGate, reference_preparation_gate};
            use std::sync::mpsc;
            let (entered, entry) = mpsc::channel();
            let (resume, gate) = mpsc::channel();
            let (prepared, preparation) = mpsc::channel();
            let (release_player, final_gate) = mpsc::channel();
            let (cancelled, cancellation) = mpsc::channel();
            let (returned, caller_return) = mpsc::channel();
            reference_preparation_gate(ReferencePreparationGate {
                label: "/movie/op1.mp4".into(), entered, resume: gate,
                prepared, finish: final_gate, cancelled,
            });
            let mut active = Adv::new(audio.clone(), server.clone(), assets.clone(), Scripter::new(&scenario, 0, 0),
            );
            active.load_game_data(&globals, &game(3), server.clone()).unwrap();
            let movie = entry.recv_timeout(Duration::from_secs(15)).unwrap();
            let player = if after_preparation {
                resume.send(()).unwrap();
                Some(preparation.recv_timeout(Duration::from_secs(15)).unwrap())
            } else { None };
            assert_eq!(active.vm_state.save_position, 0);
            assert!(active.adv_state.plane_layer_group(PlaneId::new(0)).get_used_layerbank_ids().is_empty());
            let retained = movie.clone();
            let helper = std::thread::spawn(move || {
                cancellation.recv_timeout(Duration::from_secs(15)).unwrap();
                assert!(retained.upgrade().is_some(), "Active producer still owns Movie asset");
                let player = player.unwrap_or_else(|| {
                    resume.send(()).unwrap();
                    preparation.recv_timeout(Duration::from_secs(15)).unwrap()
                });
                assert!(player.upgrade().is_some(), "Actual prepared player remains owned until producer finishes");
                assert_eq!(shin_tasks::async_io::spawn(async { 42 }).poll_with_timeout(Duration::from_secs(15)), Some(42),
                    "Caller retirement leaves the single I/O worker free");
                // Detect a caller that merely cancels the receiver and
                // returns while the producer still owns its prepared player.
                let premature = caller_return.recv_timeout(Duration::from_millis(5)).is_ok();
                release_player.send(()).unwrap();
                (player, premature)
            });
            if shutdown {
                drop(active);
            } else {
                active.load_game_data(&globals, &game(0), server.clone()).unwrap();
                let _ = returned.send(());
                assert_eq!(active.vm_state.save_position, 0);
                finish(&mut active, context, server, &mut frame);
            }
            if shutdown { let _ = returned.send(()); }
            let (player, premature) = helper.join().unwrap();
            assert!(!premature, "Caller must wait for the active producer, not only its future");
            assert!(movie.upgrade().is_none(), "Caller returns after late Movie input retires");
            assert!(player.upgrade().is_none(), "Caller returns after late decoder/player retires");
            eprintln!("Active Movie cancellation after_preparation={after_preparation} shutdown={shutdown}: actual Adv caller drains late FFmpeg/Metal/audio owner and leaves I/O available");
        }
    }
    // Exercise the real capacity gate after the task output moves into ready.
    // Cancelling this unpublished owner must still join its decoder I/O.
    for action in ["replace", "shutdown", "publish"] {
        // Earlier cases have tracks awaiting callback-side reclamation.
        // Use a fresh real manager so that reclamation cannot reopen the
        // capacity gate while this fixture's Movie is being prepared.
        let capacity_audio = Arc::new(shin_audio::AudioManager::new_for_tests());
        let mut held = Adv::new(capacity_audio.clone(), server.clone(), assets.clone(), Scripter::new(&scenario, 0, 0),
        );
        let blockers = {
            let mut manager = capacity_audio.kira_manager().lock();
            let available = manager.sub_track_capacity() - manager.num_sub_tracks();
            (0..available).map(|_| {
                    manager.add_sub_track(kira::track::TrackBuilder::new()).unwrap()
                }).collect::<Vec<_>>()
        };
        held.load_game_data(&globals, &game(3), server.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while held.pending_restore.as_ref().unwrap().ready.is_none() {
            assert!(Instant::now() < deadline, "Prepared capacity-gated scene timed out");
            step(&mut held, context, server, &mut frame);
            std::thread::sleep(Duration::from_millis(1));
        }
        let pending = held.pending_restore.as_ref().unwrap();
        assert!(pending.task.is_none());
        let retirement = pending.retirement.as_ref().unwrap().clone();
        assert!(!retirement.is_finished(), "Prepared Movie retains actual decoder I/O");
        let prepared = owners(&pending.ready.as_ref().unwrap().as_ref().unwrap().0);
        assert_eq!(prepared.len(), 3);
        assert_eq!(held.vm_state.save_position, 0);
        for _ in 0..2 {
            step(&mut held, context, server, &mut frame);
            assert_eq!(held.vm_state.save_position, 0, "Capacity wait retains old VM");
            assert!(held.pending_restore.as_ref().unwrap().ready.is_some());
        }
        match action {
            "replace" => {
                held.load_game_data(&globals, &game(0), server.clone()).unwrap();
                assert!(retirement.is_finished(), "Replacement joins already-ready decoder work");
                for (_, owner) in &prepared { owner.assert_released(); }
                drop(blockers);
                finish(&mut held, context, server, &mut frame);
            }
            "shutdown" => {
                drop(held);
                assert!(retirement.is_finished(), "Shutdown joins already-ready decoder work");
                for (_, owner) in &prepared { owner.assert_released(); }
                drop(blockers);
            }
            "publish" => {
                drop(blockers);
                finish(&mut held, context, server, &mut frame);
                assert_eq!(held.vm_state.save_position, 3);
                for (bank, owner) in &prepared {
                    owner.assert_live(&held.adv_state.plane_layer_group(PlaneId::new(0)).get_layer(*bank).unwrap(),
                    );
                }
                // The token itself does not join resources whose ownership
                // has transferred to the published scene.
                drop(held);
            }
            _ => unreachable!(),
        }
        eprintln!("Capacity-gated ready scene action={action}: exact prepared owners, retained VM, loader handle transfer/retirement pass");
    }
    // Cancel a fully prepared mixed scene by replacement and by shutdown.
    for shutdown in [false, true] {
        live.load_game_data(&globals, &game(3), server.clone())
            .unwrap();
        let task = live.pending_restore.as_mut().unwrap().task.take().unwrap();
        let (release, gate) = futures::channel::oneshot::channel::<()>();
        let (ready, received) = std::sync::mpsc::channel();
        live.pending_restore.as_mut().unwrap().task =
            Some(shin_tasks::async_io::spawn(async move {
                let result = task.await?;
                ready.send(owners(&result.0)).unwrap();
                gate.await
                    .map_err(|_| anyhow::anyhow!("Cancelled mixed worker"))?;
                Ok(result)
            }));
        let pending = received.recv_timeout(Duration::from_secs(15)).unwrap();
        assert_eq!(pending.len(), 3);
        if shutdown {
            let mut dying = Adv::new(
                audio.clone(),
                server.clone(),
                assets.clone(),
                Scripter::new(&scenario, 0, 0),
            );
            dying.pending_restore = live.pending_restore.take();
            drop(dying);
        } else {
            live.load_game_data(&globals, &game(0), server.clone())
                .unwrap();
            finish(&mut live, context, server, &mut frame);
        }
        for (_, owner) in pending {
            owner.assert_released();
        }
        drop(release);
        assert_eq!(live.vm_state.save_position, 0);
        eprintln!(
            "Mixed completed-worker cancellation shutdown={shutdown}: all top-level resource and player owners retired without publication"
        );
    }
    drop(live);
    let scenario = source(original, true, None);
    assets.scenario = scenario.clone();
    let mut live = Adv::new(
        audio.clone(),
        server.clone(),
        assets.clone(),
        Scripter::new(&scenario, 0, 0),
    );
    live.load_game_data(&globals, &game(3), server.clone())
        .unwrap();
    finish(&mut live, context, server, &mut frame);
    let current = live
        .adv_state
        .plane_layer_group(PlaneId::new(0))
        .get_used_layerbank_ids()
        .into_iter()
        .map(|bank| {
            (
                bank,
                Owner::of(
                    &live.adv_state
                        .plane_layer_group(PlaneId::new(0))
                        .get_layer(bank)
                        .unwrap(),
                ),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(current.len(), 3);
    let counts = current
        .iter()
        .filter_map(|(_, owner)| match owner {
            Owner::Picture(w) => Some(w.strong_count()),
            Owner::Bustup(w) => Some(w.strong_count()),
            Owner::Movie(_, _) => None,
        })
        .collect::<Vec<_>>();
    let position = live.scripter.position();
    let requested = server.reference_requested_paths().len();
    live.load_game_data(&globals, &game(4), server.clone())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while live.pending_restore.is_some() {
        assert!(Instant::now() < deadline, "Mixed failure timed out");
        step(&mut live, context, server, &mut frame);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(live.saved_scene_load_error().is_some());
    assert_eq!(live.vm_state.save_position, 3);
    assert_eq!(live.scripter.position(), position);
    let paths = server.reference_requested_paths();
    let paths = &paths[requested..];
    assert!(paths.iter().any(|p| p.starts_with("/picture/")));
    assert!(paths.iter().any(|p| p.starts_with("/bustup/")));
    assert!(paths.iter().any(|p| p.starts_with("/movie/")));
    assert_eq!(
        paths.last().unwrap(),
        &scenario.info_tables().mask_info[0].path()
    );
    for (bank, owner) in &current {
        owner.assert_live(
            &live.adv_state
                .plane_layer_group(PlaneId::new(0))
                .get_layer(*bank)
                .unwrap(),
        );
    }
    assert_eq!(
        counts,
        current
            .iter()
            .filter_map(|(_, owner)| match owner {
                Owner::Picture(w) => Some(w.strong_count()),
                Owner::Bustup(w) => Some(w.strong_count()),
                Owner::Movie(_, _) => None,
            })
            .collect::<Vec<_>>(),
        "Failed mixed load retains no extra Picture/Bustup owners"
    );
    eprintln!(
        "Mixed genuine late mask IO failure: old VM/resource/player identities preserved, Picture/Bustup partial-load owners retired"
    );
    drop(live);
    for invalid in [-1, i32::MIN, i32::MAX, 0] {
        let scenario = source(original, false, Some(invalid));
        let mut invalid_assets = assets.clone();
        invalid_assets.scenario = scenario.clone();
        let mut live = Adv::new(
            audio.clone(),
            server.clone(),
            invalid_assets,
            Scripter::new(&scenario, 0, 0),
        );
        let requested = server.reference_requested_paths().len();
        live.load_game_data(&globals, &game(3), server.clone())
            .unwrap();
        finish(&mut live, context, server, &mut frame);
        let group = live.adv_state.plane_layer_group(PlaneId::new(0));
        let banks = group.get_used_layerbank_ids();
        assert_eq!(banks.len(), 3);
        for bank in banks {
            let saved = &live.vm_state.layers.layerbanks[bank];
            let expected = if invalid == 0 {
                match saved.layer_type.unwrap() {
                    shin_core::vm::command::types::LayerType::Picture => {
                        original.info_tables().picture_info.len() as i32
                    }
                    shin_core::vm::command::types::LayerType::Bustup => {
                        original.info_tables().bustup_info.len() as i32
                    }
                    shin_core::vm::command::types::LayerType::Movie => {
                        original.info_tables().movie_info.len() as i32
                    }
                    _ => panic!("Unexpected saved layer"),
                }
            } else {
                invalid
            };
            assert_eq!(
                saved.params.0, expected,
                "Test the exact full-width rejected ID"
            );
            assert!(
                group.get_layer(bank).is_some_and(|layer| matches!(&*layer, UserLayer::Null(_))),
                "Native fallback requires an installed Null layer"
            );
        }
        assert_eq!(
            server.reference_requested_paths().len(),
            requested,
            "Rejected metadata IDs must not request assets"
        );
        assert_eq!(live.vm_state.save_position, 3);
        let preview = shin_tasks::block_on(crate::adv::save_menu::preview::SaveMenuPreviewScene::load(
            server, &audio, &scenario, &live.vm_state,
            )).unwrap();
        for bank in live.adv_state.plane_layer_group(PlaneId::new(0)).get_used_layerbank_ids() {
            assert!(preview.reference_layer(PlaneId::new(0), bank).is_some_and(|layer| matches!(&*layer, UserLayer::Null(_))));
        }
        assert_eq!(server.reference_requested_paths().len(), requested,
            "Rejected metadata thumbnail must not request assets");
        eprintln!(
            "Invalid metadata id mode{invalid}: actual Picture/Bustup/Movie saved publication and thumbnail install three Null layers, no asset requests, no panic"
        );
    }
    for linked in [i16::MIN, -1, i16::try_from(original.info_tables().picture_info.len()).unwrap(), i16::MAX,
    ] {
        let scenario = source_with_movie_link(original, linked);
        let mut movie_assets = assets.clone();
        movie_assets.scenario = scenario.clone();
        let mut live = Adv::new(audio.clone(), server.clone(), movie_assets, Scripter::new(&scenario, 0, 0),
        );
        let requested = server.reference_requested_paths().len();
        live.load_game_data(&globals, &game(3), server.clone()).unwrap();
        finish(&mut live, context, server, &mut frame);
        let group = live.adv_state.plane_layer_group(PlaneId::new(0));
        let bank = group.get_used_layerbank_ids().into_iter()
            .find(|bank| {
                group.get_layer(*bank).is_some_and(|layer| matches!(&*layer, UserLayer::Movie(_)))
            }).unwrap();
        let layer = group.get_layer(bank).unwrap();
        let UserLayer::Movie(movie) = &*layer else { unreachable!() };
        assert!(!movie.reference_has_still_picture());
        let owner = movie.reference_publication_owner();
        assert!(owner.owners() > 0);
        let paths = server.reference_requested_paths();
        let paths = &paths[requested..];
        assert!(paths.iter().any(|p| p == "/movie/op1.mp4"));
        assert!(!paths.iter().any(|p| p == &original.info_tables().picture_info[0].path()),
            "Rejected cross-reference must not load the original still");
        assert_eq!(live.vm_state.save_position, 3);
        let requested = server.reference_requested_paths().len();
        let preview = shin_tasks::block_on(crate::adv::save_menu::preview::SaveMenuPreviewScene::load(
            server, &audio, &scenario, &live.vm_state,
            )).unwrap();
        assert!(preview.reference_layer(PlaneId::new(0), bank).is_some_and(|layer| matches!(&*layer, UserLayer::Null(_))));
        let paths = server.reference_requested_paths();
        assert!(!paths[requested..].iter().any(|p| p.starts_with("/movie/") || p == &original.info_tables().picture_info[0].path()));
        drop(preview);
        live.load_game_data(&globals, &game(0), server.clone()).unwrap();
        finish(&mut live, context, server, &mut frame);
        owner.assert_released();
        eprintln!("Rejected Movie linked Picture{linked}: saved Movie backend publishes without still, preview uses Null, resource/player owners retire");
    }
}
