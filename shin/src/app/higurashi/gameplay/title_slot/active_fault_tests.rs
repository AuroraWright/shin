//! Retained desktop constructor faults; native source-death/constructor order
//! remains unchanged. Register this focused fixture only when its parent batches
//! the normal application interface checks.
use super::*;
use crate::{
    adv::higurashi::{audio::HigurashiAudio, reference_gpu_assets, reference_render_to_texture},
    app::higurashi::{
        runtime_game_data::RuntimeGameData, runtime_saves::Slots, title_demo::TitleRequest,
    },
};
use shin_core::{
    format::scenario::higurashi::{HigurashiScenario, HigurashiVariant},
    primitives::color::UnormColor,
};
use std::sync::atomic::{AtomicUsize, Ordering};

fn globals(server: &Arc<AssetServer>, common: &Arc<CommonAssets>) -> Globals {
    let manager = Arc::new(shin_audio::AudioManager::new_for_tests());
    let mut data = SaveData::default();
    data.save_vectors.seen_messages = vec![0xa5a5_0123; 4];
    data.save_vectors.extra_words_0 = vec![u32::MAX; 20];
    let mut slots = Slots::loaded(&data);
    slots.manual[2].valid = true;
    slots.manual[2].data = RuntimeGameData::from(super::super::tests::fresh());
    slots.manual[2].data.progress.extra_word = 0xdead_beef;
    slots.manual[2].data.progress.entries[1].response_data = vec![0x45, 0xf1];
    slots.manual[2].data.trailing_values = vec![0xffff, 0x1234];
    Globals {
        common: common.clone(),
        data,
        storage: None,
        runtime_saves: Some(slots),
        clock: PlayClock::from_saved_seconds(77),
        platform_flag: true,
        last_title_mode: 0,
        audio: HigurashiAudio::new(manager.clone(), server.clone(), common.scenario.clone()),
        server: server.clone(),
        manager: manager.clone(),
        sounds: Arc::new(Mutex::new(SystemSoundPlayer::new(manager))),
    }
}

fn original_variant(common: &Arc<CommonAssets>) -> Arc<CommonAssets> {
    let mut scenario = HigurashiScenario::new(common.scenario.raw().to_vec().into()).unwrap();
    scenario.variant = HigurashiVariant::Original;
    Arc::new(CommonAssets {
        scenario: Arc::new(scenario),
        font: common.font.clone(),
        system: common.system.clone(),
        message: common.message.clone(),
        effects: common.effects.clone(),
        sysse: common.sysse.clone(),
    })
}

#[test]
#[ignore = "real Title/Story/TIPS globals, source retirement, retained constructor fault/retry, full native runtime rows and entropy on offscreen Metal/the default audio output; no windows/saves"]
fn higurashi_active_scene_retains_fresh_story_and_tips_constructor_faults() {
    let (device, queue, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let (_, common) = super::super::tests::character::common(&server, &[0x83, 0, 1]);
    for extent in [(1280, 720), (1920, 1080)] {
        for tips in [false, true] {
            let globals = globals(&server, &common);
            let manual = globals.runtime_saves.as_ref().unwrap().manual[2]
                .data
                .clone();
            let words = globals.data.save_vectors.seen_messages.as_ptr();
            let mut slot = TitleSlot::new(
                TitleTransition {
                    request: TitleRequest {
                        selected: 0,
                        mode: 0,
                    },
                    globals,
                },
                None,
            );
            let request = crate::app::higurashi::tips_menu::factory::Request {
                data: None,
                selected: 0,
                alternate: 0,
            };
            if tips {
                slot.queue_tips(request.clone());
            } else {
                slot.pending = Some(Pending::Story(super::super::tests::fresh(), 0));
            }
            let seeds = Arc::new(AtomicUsize::new(0));
            slot.reference_fail_next_constructor();
            reference_render_to_texture(&device, &queue, extent, |context, target| {
                let mut composed = context.new_render_texture("Retained factory source".into());
                for output in [
                    composed.as_texture_target(),
                    shin_render::shaders::types::texture::TextureTarget {
                        view: target,
                        kind:
                            shin_render::shaders::types::texture::TextureTargetKind::RenderTexture,
                    },
                ] {
                    let mut pass =
                        context.begin_pass(output, Some(context.depth_stencil), "Retained source");
                    pass.clear(
                        Some(UnormColor::from_rgba(31, 63, 127, 255)),
                        Some(0),
                        Some(1.),
                    );
                }
                let result = if tips {
                    slot.dispatch_tips(context, &composed, (1280, 720))
                } else {
                    let count = seeds.clone();
                    slot.dispatch_story(context, &composed, move || {
                        count.fetch_add(1, Ordering::SeqCst);
                        [0x1234_5678; 4]
                    })
                };
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("Injected retained")
                );
                assert_eq!(slot.active_scene().unwrap(), ActiveScene::RetiredFactory);
                assert!(slot.title.is_none() && slot.game.is_none() && slot.tips.is_none());
                assert_eq!(slot.reference_scene_events, [1]);
                assert_eq!(seeds.load(Ordering::SeqCst), 0);
                let retained = slot.active_globals().unwrap();
                assert_eq!(retained.data.save_vectors.seen_messages.as_ptr(), words);
                assert_eq!(retained.runtime_saves.unwrap().manual[2].data, manual);
                assert_eq!(retained.clock.saved_seconds(), 77);
                assert!(!slot.has_save_task());
                let original = slot.retained_globals().unwrap().common.clone();
                slot.retained_globals_mut().unwrap().common = original_variant(&original);
                // Exercise the owner's actual fallible constructor after its
                // source is already gone; no further retirement or snapshot.
                let rejected = if tips {
                    slot.dispatch_tips(context, &composed, (1280, 720))
                } else {
                    slot.dispatch_story(context, &composed, || panic!("retry replaced seeds"))
                };
                assert!(rejected.is_err());
                assert_eq!(slot.active_scene().unwrap(), ActiveScene::RetiredFactory);
                assert_eq!(slot.reference_scene_events, [1]);
                assert_eq!(seeds.load(Ordering::SeqCst), 0);
                assert_eq!(
                    slot.active_globals().unwrap().runtime_saves.unwrap().manual[2].data,
                    manual
                );
                slot.retained_globals_mut().unwrap().common = original;
                if tips {
                    let Some(Pending::PreparedTips(transfer)) = &slot.pending else {
                        unreachable!()
                    };
                    assert_eq!(transfer.request, request);
                    assert!(transfer.snapshot.is_some());
                    assert!(
                        slot.dispatch_tips(context, &composed, (1920, 1080))
                            .unwrap()
                    );
                    assert_eq!(slot.active_scene().unwrap(), ActiveScene::Tips);
                    assert_eq!(seeds.load(Ordering::SeqCst), 0);
                } else {
                    assert!(
                        slot.dispatch_story(context, &composed, || panic!("retry replaced seeds"))
                            .unwrap()
                    );
                    assert_eq!(slot.active_scene().unwrap(), ActiveScene::Gameplay);
                    assert_eq!(seeds.load(Ordering::SeqCst), 4);
                }
                assert_eq!(slot.reference_scene_events, [1, 2]);
                assert!(!slot.pending_scene_factory());
                assert_eq!(
                    slot.active_globals().unwrap().runtime_saves.unwrap().manual[2].data,
                    manual
                );
                assert_eq!(slot.active_globals().unwrap().clock.saved_seconds(), 77);
            });
            drop(slot);
        }
    }
    println!(
        "Active scene: 4 real Story/TIPS factory profiles, 2 constructor faults then same-owner retry, source-before-constructor retirement, frozen snapshot/request, 0 pre-constructor seeds, 4 native Story base groups and complete manual runtime rows pass"
    );
}
