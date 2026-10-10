mod scene_reuse {
    use super::*;
    use crate::{
        adv::{Adv, AdvAssets},
        asset::system::AssetServer,
        layer::{user::UserLayer, DrawableLayer},
        render::PreRenderContext,
        update::UpdateContext,
    };
    use shin_core::{
        format::save::{GameData, GameDataEntry, RouteRecorder},
        format::scenario::{
            instruction_elements::{MessageId, U8Bool},
            types::U16FixupString,
        },
        primitives::update::FrameId,
        time::Ticks,
        vm::command::types::{LayerId, LayerProperty},
    };
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    fn n<T>(v: i32) -> NumberSpec<T> {
        NumberSpec::new(NumberSpec::constant(v).into_untyped())
    }

    fn source(original: &Scenario, quiz: bool) -> Arc<Scenario> {
        let movie = original
            .info_tables()
            .movie_info
            .iter()
            .position(|m| m.name.as_str().eq_ignore_ascii_case("door"))
            .unwrap();
        let mut bytes = original.raw().to_vec();
        let offset = bytes.len();
        bytes[32..36].copy_from_slice(&(offset as u32).to_le_bytes());
        let mut cursor = std::io::Cursor::new(&mut bytes);
        cursor.set_position(offset as u64);
        let values = if quiz {
            [0, 31, 0, 0, 0, 0, 0, 0]
        } else {
            [movie as i32, 1000, 1, 0, 0, 0, 0, 0]
        };
        let mut params = std::io::Cursor::new(vec![255u8]);
        params.set_position(1);
        for value in values {
            NumberSpec::constant(value).write_le(&mut params).unwrap();
        }
        params.set_position(0);
        let params =
            shin_core::format::scenario::instruction_elements::BitmaskNumberArray::read_le(
                &mut params,
            )
            .unwrap();
        for cmd in [
            C::LAYERLOAD(c::LAYERLOAD {
                layer_id: n(1),
                layer_type: n(if quiz { 9 } else { 6 }),
                flags: n(0),
                params,
            }),
            C::MSGSET(c::MSGSET {
                msg_id: MessageId(1),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("@rFirst retained scene"),
            }),
            C::MSGSET(c::MSGSET {
                msg_id: MessageId(2),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("@rSecond retained scene"),
            }),
            C::EXIT(c::EXIT {
                arg1: 0,
                arg2: NumberSpec::constant(0),
            }),
        ] {
            Instruction::Command(cmd).write_le(&mut cursor).unwrap();
        }
        let len = bytes.len() as u32;
        bytes[4..8].copy_from_slice(&len.to_le_bytes());
        Arc::new(Scenario::new(bytes.into()).unwrap())
    }
    fn step<'a>(
        live: &mut Adv,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
        ticks: f32,
    ) {
        frame.advance();
        live.update(
            &mut UpdateContext {
                frame_started_at: Instant::now(),
                frame_id: *frame,
                delta_ticks: Ticks::from_f32(ticks),
                asset_server: server,
                pre_render: context,
            },
            enum_map::EnumMap::default(),
        );
        let encoder = std::mem::replace(
            context.encoder,
            context.device.create_command_encoder(&Default::default()),
        );
        let mut upload = context.device.create_command_encoder(&Default::default());
        context.dynamic_buffer.finish(&mut upload);
        context.queue.submit([upload.finish(), encoder.finish()]);
        context.device.poll(wgpu::Maintain::Wait);
    }
    fn finish<'a>(
        live: &mut Adv,
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        frame: &mut FrameId,
    ) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while live.is_loading_saved_scene() {
            assert!(Instant::now() < deadline, "retained scene loading timeout");
            step(live, context, server, frame, 1.0);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(live.saved_scene_load_error().is_none());
    }
    fn container(live: &Adv, id: usize) -> &crate::layer::LayerProperties {
        match id {
            0 => live.adv_state.root_layer_group().properties(),
            1 => live.adv_state.screen_layer().properties(),
            2 => live.adv_state.page_layer().properties(),
            _ => live
                .adv_state
                .plane_layer_group(shin_core::vm::command::types::PlaneId::new((id - 3) as u8))
                .properties(),
        }
    }
    fn prepare_containers(live: &mut Adv) -> Vec<crate::layer::LayerProperties> {
        (0..7)
            .map(|id| {
                let properties = match id {
                    0 => live.adv_state.root_layer_group_mut().properties_mut(),
                    1 => live.adv_state.screen_layer_mut().properties_mut(),
                    2 => live.adv_state.page_layer_mut().properties_mut(),
                    _ => live
                        .adv_state
                        .plane_layer_group_mut(shin_core::vm::command::types::PlaneId::new(
                            (id - 3) as u8,
                        ))
                        .properties_mut(),
                };
                properties.set_layer_id(LayerId::new(230 + id as u16));
                properties.set_layerload_counter1(400 + id as u32);
                properties
                    .property_tweener_mut(LayerProperty::TranslateX)
                    .fast_forward_to(77.0);
                properties.share()
            })
            .collect()
    }
    fn check_containers(live: &Adv, aliases: &[crate::layer::LayerProperties]) {
        for (id, alias) in aliases.iter().enumerate() {
            let properties = container(live, id);
            assert_eq!(
                properties.identity(),
                alias.identity(),
                "container {id} property owner retained"
            );
            assert_eq!(properties.get_value(LayerProperty::TranslateX), 0.0);
            assert_eq!(properties.get_layer_id(), LayerId::new(230 + id as u16));
            assert_eq!(properties.layer_load_counter(), 400 + id as u32);
        }
    }
    pub(super) fn verify<'a>(
        context: &mut PreRenderContext<'a, '_, '_, '_>,
        server: &'a Arc<AssetServer>,
        assets: &AdvAssets,
        original: &Arc<Scenario>,
    ) {
        for quiz in [false, true] {
            let scenario = source(original, quiz);
            let mut assets = assets.clone();
            assets.scenario = scenario.clone();
            let audio = Arc::new(shin_audio::AudioManager::new_for_tests());
            let mut live = Adv::new(
                audio.clone(),
                server.clone(),
                assets,
                Scripter::new(&scenario, 0, 42),
            );
            let containers = prepare_containers(&mut live);
            let globals = SaveData::default();
            let game = GameData::from_entry(GameDataEntry {
                scenario_id: 0,
                random_seed: 42,
                save_position: 2,
                route_data: RouteRecorder::default().snapshot(),
            });
            let mut frame = FrameId::default();
            live.load_game_data(&globals, &game, server.clone())
                .unwrap();
            finish(&mut live, context, server, &mut frame);
            check_containers(&live, &containers);
            step(&mut live, context, server, &mut frame, 30.0);
            let plane = live.vm_state.layers.current_plane;
            let bank = live
                .vm_state
                .layers
                .layerbank_allocator
                .get_layerbank_id(plane, LayerId::new(1))
                .unwrap();
            let layer = live
                .adv_state
                .plane_layer_group(plane)
                .get_layer(bank)
                .unwrap();
            let property_alias = layer.properties().share();
            let property_owner = property_alias.identity();
            let movie = match layer {
                UserLayer::Movie(layer) => Some(layer.reference_publication_owner()),
                _ => None,
            };
            let quiz_before = match layer {
                UserLayer::Quiz(layer) => {
                    layer.navigate(1);
                    Some(layer.reference_controller_state())
                }
                _ => None,
            };
            assert_eq!(live.vm_state.quick_return_position(), Some(1));
            assert!(live.load_previous_scene(server.clone()).unwrap());
            assert_eq!(
                live.vm_state.save_position, 2,
                "source remains current while loader waits"
            );
            assert_eq!(
                live.adv_state
                    .plane_layer_group(plane)
                    .get_layer(bank)
                    .unwrap()
                    .properties()
                    .identity(),
                property_owner
            );
            finish(&mut live, context, server, &mut frame);
            assert_eq!(live.vm_state.save_position, 1);
            for mode in ["history", "resume"] {
                if mode == "resume" {
                    live.vm_state.resume_position = Some(2);
                    live.queue_resume_scene(&mut UpdateContext {
                        frame_started_at: Instant::now(),
                        frame_id: frame,
                        delta_ticks: Ticks::ZERO,
                        asset_server: server,
                        pre_render: context,
                    });
                    finish(&mut live, context, server, &mut frame);
                    assert_eq!(live.vm_state.save_position, 2);
                }
                check_containers(&live, &containers);
                let layer = live
                    .adv_state
                    .plane_layer_group(plane)
                    .get_layer(bank)
                    .unwrap();
                assert_eq!(
                    layer.properties().identity(),
                    property_owner,
                    "{mode} retains the property owner"
                );
                assert_eq!(layer.properties().get_value(LayerProperty::TranslateX), 0.0);
                if let Some(owner) = &movie {
                    let UserLayer::Movie(layer) = layer else {
                        panic!()
                    };
                    assert_eq!(
                        layer.reference_publication_owner().identity(),
                        owner.identity(),
                        "{mode} preserves decoder and playback identity"
                    );
                    assert_eq!(owner.owners(), 1);
                }
                if let Some(state) = quiz_before {
                    let UserLayer::Quiz(layer) = layer else {
                        panic!()
                    };
                    assert_eq!(
                        layer.reference_controller_state(),
                        state,
                        "{mode} preserves interactive model and selection"
                    );
                }
            }
            // Ordinary manual/Continue loads do not collect retained layers.
            live.load_game_data(&globals, &game, server.clone())
                .unwrap();
            finish(&mut live, context, server, &mut frame);
            check_containers(&live, &containers);
            let layer = live
                .adv_state
                .plane_layer_group(plane)
                .get_layer(bank)
                .unwrap();
            assert_ne!(
                layer.properties().identity(),
                property_owner,
                "ordinary load constructs fresh properties"
            );
            if let Some(owner) = movie {
                owner.assert_released();
            }
            if let Some(state) = quiz_before {
                let UserLayer::Quiz(layer) = layer else {
                    panic!()
                };
                assert_ne!(layer.reference_controller_state().1, state.1);
            }
            eprintln!(
                "Retained {}: actual quick return/RESUME preserve drawable backend/model and properties; ordinary load retires source and creates a fresh drawable; all seven container property owners/IDs/counters survive every mode",
                if quiz { "Quiz" } else { "Movie" }
            );
            drop(live);
            drop(audio);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
