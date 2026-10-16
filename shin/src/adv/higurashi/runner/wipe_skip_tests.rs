//! Actual first-prologue WIPE bytes through native read-policy dispatch.
//! Real masks/assets and a held decoded-result seam; no original executable,
//! audio, window, saves or full-prologue playback in this focused regression.
use super::super::{input::ReadState, read_state::Checkpoint};
use super::{
    tests::{Voice, policy},
    *,
};
use crate::asset::mask::MaskTexture;
use shin_core::format::save::higurashi::SaveData;
use std::{
    sync::{
        Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Default)]
struct Services {
    saved: SaveData,
    event_checkpoints: usize,
}
impl ReadServices for Services {
    fn layer_settings(&mut self) -> Option<&mut SaveData> {
        Some(&mut self.saved)
    }
    fn checkpoint(&mut self, kind: Checkpoint, _: ReadState) -> Result<()> {
        assert_eq!(kind, Checkpoint::EventBegin);
        self.event_checkpoints += 1;
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    loading_modes: u32,
    ready_modes: u32,
    forced: bool,
    read_only: bool,
    protected: bool,
    skip: bool,
}
impl Case {
    fn native(self, ready: bool) -> ReadPolicy {
        ReadPolicy {
            modes: if ready {
                self.ready_modes
            } else {
                self.loading_modes
            },
            forced: self.forced,
            read_only: self.read_only,
        }
    }
}

fn pump(r: &mut HigurashiRunner, s: &mut Services, v: &mut Voice, n: ReadPolicy) -> Status {
    let p = policy();
    assert!(!p.fast_skip && !p.suppress_transient_audio);
    shin_tasks::block_on(r.pump_native(p, n, s, 1, 1000, || panic!("WIPE consumed entropy"), v))
        .unwrap()
}

fn scenario(
    authored: &HigurashiScenario,
    start: usize,
    end: usize,
    protected: bool,
) -> Arc<HigurashiScenario> {
    // Copy the authored command span unchanged (WIPE + WIPEWAIT, or WAIT).
    // Only the bounded entry/exit and optional protected event are seams.
    let mut code = if protected { vec![0xa2, 0] } else { Vec::new() }; // EVBEGIN0 denies137d0.
    code.extend_from_slice(&authored.raw()[start..end]);
    code.extend([0, 0, 0]); // Explicit test EXIT after original WIPEWAIT.
    let mut raw = authored.raw().to_vec();
    let at = authored.header.code_offset as usize;
    raw[at..at + code.len()].copy_from_slice(&code);
    Arc::new(HigurashiScenario::new(raw.into()).unwrap())
}

fn staged(assets: Arc<AssetServer>, scenario: Arc<HigurashiScenario>) -> HigurashiRunner {
    let mut r = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        scenario.clone(),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    // Explicit real front/back owners, not an invented pending metadata row.
    // Picture746 is the actual prologue logo used immediately before mask7.
    for command in [
        Cmd::LAYERLOAD {
            args: [1, 1, 0],
            params: (0xf800, -640, -720, 1280, 720, 0, 0, 0),
        },
        Cmd::LAYERBACK { argument: 1 },
        Cmd::LAYERLOAD {
            args: [1, 2, 0],
            params: (746, 0, 0, 0, 0, 0, 0, 0),
        },
    ] {
        assert!(
            shin_tasks::block_on(
                r.reference_frame_mut()
                    .scene
                    .layers_mut()
                    .unwrap()
                    .dispatch(&command, &assets, &scenario, || [1, 2, 3, 4],)
            )
            .unwrap()
        );
    }
    r
}

#[test]
#[ignore = "actual prologue mask8/X and7/UP loading with native per-command skip gates; offscreen Metal, no audio/window/saves"]
fn higurashi_runner_prologue_masks_use_native_skip_after_real_loading() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    assert_eq!(authored.picture_info[746].name.as_str(), "a/c_logo");
    let cases = [
        Case {
            name: "eligible",
            loading_modes: 2,
            ready_modes: 2,
            forced: false,
            read_only: false,
            protected: false,
            skip: true,
        },
        Case {
            name: "ordinary",
            loading_modes: 0,
            ready_modes: 0,
            forced: false,
            read_only: false,
            protected: false,
            skip: false,
        },
        Case {
            name: "autoplay",
            loading_modes: 1,
            ready_modes: 1,
            forced: false,
            read_only: false,
            protected: false,
            skip: false,
        },
        Case {
            name: "unread-read-only",
            loading_modes: 2,
            ready_modes: 2,
            forced: false,
            read_only: true,
            protected: false,
            skip: false,
        },
        Case {
            name: "protected-event",
            loading_modes: 2,
            ready_modes: 2,
            forced: false,
            read_only: false,
            protected: true,
            skip: false,
        },
        Case {
            name: "forced-unread",
            loading_modes: 0,
            ready_modes: 0,
            forced: true,
            read_only: true,
            protected: false,
            skip: true,
        },
        Case {
            name: "enabled-after-worker",
            loading_modes: 0,
            ready_modes: 2,
            forced: false,
            read_only: false,
            protected: false,
            skip: true,
        },
        Case {
            name: "withdrawn-after-worker",
            loading_modes: 2,
            ready_modes: 0,
            forced: false,
            read_only: false,
            protected: false,
            skip: false,
        },
    ];
    let mut profiles = 0;
    for (start, end, mask_id, duration, name, raw) in [
        (
            0x5a8f4,
            0x5a8fb,
            8,
            60,
            "X",
            &[0x8e, 1, 0, 60, 1, 8, 0x8f][..],
        ),
        (
            0x5a916,
            0x5a91e,
            7,
            120,
            "UP",
            &[0x8e, 1, 0, 0x80, 0x78, 1, 7, 0x8f][..],
        ),
    ] {
        assert_eq!(&authored.raw()[start..end], raw, "authored prologue bytes");
        assert_eq!(authored.mask_info[mask_id].name.as_str(), name);
        let path = format!("/mask/{}.msk", name.to_ascii_lowercase());
        assert!(assets.contains(&path));
        for case in cases {
            let scenario = scenario(&authored, start, end, case.protected);
            let mut r = staged(assets.clone(), scenario);
            let mut services = Services::default();
            let mut voice = Voice::default();
            let prefix = u64::from(case.protected);
            if case.protected {
                assert_eq!(
                    pump(&mut r, &mut services, &mut voice, case.native(false)),
                    Status::CommandBudget
                );
                assert_eq!(r.native_read_state().unwrap().flags, 0);
            }
            assert_eq!(services.event_checkpoints, usize::from(case.protected));
            let before_gallery = services.saved.save_vectors.unlocked_cgs.clone();
            let requests_before = assets
                .reference_requested_paths()
                .iter()
                .filter(|p| p.as_str() == path)
                .count();
            let (release, receiver) = futures::channel::oneshot::channel();
            let called = Arc::new(AtomicBool::new(false));
            let decoded = Arc::new(Mutex::new(None::<Weak<MaskTexture>>));
            let observed_called = called.clone();
            let observed_decoded = decoded.clone();
            let expected_path = path.clone();
            r.reference_frame_mut()
                .scene
                .reference_gate_next_mask(Box::new(move |result| {
                    let mask = result.as_ref().expect("actual authored MSK3 decode");
                    assert!(mask.label.contains(&expected_path));
                    assert_eq!(
                        mask.texture.wgpu_texture().format(),
                        wgpu::TextureFormat::R8Unorm
                    );
                    assert!(
                        observed_decoded
                            .lock()
                            .unwrap()
                            .replace(Arc::downgrade(mask))
                            .is_none()
                    );
                    observed_called.store(true, Ordering::Release);
                    Box::pin(async move {
                        let _ = receiver.await;
                    })
                }));
            assert_eq!(
                pump(&mut r, &mut services, &mut voice, case.native(false)),
                Status::Waiting
            );
            let deadline = Instant::now() + Duration::from_secs(30);
            while !called.load(Ordering::Acquire) {
                assert!(
                    Instant::now() < deadline,
                    "{}: actual mask decode timeout",
                    case.name
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(
                assets
                    .reference_requested_paths()
                    .iter()
                    .filter(|p| p.as_str() == path)
                    .count(),
                requests_before + 1
            );
            let weak = decoded.lock().unwrap().as_ref().unwrap().clone();
            assert!(weak.upgrade().is_some());
            let retained = (
                r.position(),
                r.next_position(),
                r.random_state(),
                r.frame().visual_epoch(),
            );
            for _ in 0..4 {
                assert_eq!(
                    pump(&mut r, &mut services, &mut voice, case.native(false)),
                    Status::Waiting
                );
                assert!(!r.visual_updates_admitted(policy()));
                r.advance(Ticks::from_f32(3.0), policy(), &mut voice, None);
                assert_eq!(
                    (
                        r.position(),
                        r.next_position(),
                        r.random_state(),
                        r.frame().visual_epoch()
                    ),
                    retained
                );
                assert_eq!(r.completed_commands(), prefix);
                assert!(r.frame().scene.transition().is_none());
                assert_eq!(services.saved.save_vectors.unlocked_cgs, before_gallery);
            }
            release.send(()).unwrap();
            // The result may become ready, but paused pump cannot publish it.
            let mut paused = policy();
            paused.paused = true;
            assert_eq!(
                shin_tasks::block_on(r.pump_native(
                    paused,
                    case.native(true),
                    &mut services,
                    1,
                    1000,
                    || [1, 2, 3, 4],
                    &mut voice
                ))
                .unwrap(),
                Status::Waiting
            );
            assert_eq!(r.completed_commands(), prefix);
            assert_eq!(services.saved.save_vectors.unlocked_cgs, before_gallery);
            let status = loop {
                assert!(
                    Instant::now() < deadline,
                    "{}: ready mask publication timeout",
                    case.name
                );
                let status = pump(&mut r, &mut services, &mut voice, case.native(true));
                if status != Status::Waiting || r.frame().scene.transition().is_some() {
                    break status;
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            assert_eq!(
                status,
                if case.skip {
                    Status::CommandBudget
                } else {
                    Status::Waiting
                },
                "{} mask{mask_id}",
                case.name
            );
            assert_eq!(
                r.frame().scene.transition().is_none(),
                case.skip,
                "{} mask{mask_id}",
                case.name
            );
            assert_eq!(r.completed_commands(), prefix + u64::from(case.skip));
            assert_ne!(
                services.saved.save_vectors.unlocked_cgs[746 / 32] & (1 << (746 % 32)),
                0,
                "genuine PIC metadata publication"
            );
            assert_eq!(weak.upgrade().is_some(), !case.skip);
            if !case.skip {
                r.advance(
                    Ticks::from_f32((duration - 1) as f32),
                    policy(),
                    &mut voice,
                    None,
                );
                assert!(r.frame().scene.transition().is_some());
                assert_eq!(
                    pump(&mut r, &mut services, &mut voice, case.native(true)),
                    Status::Waiting
                );
                r.advance(Ticks::from_f32(1.0), policy(), &mut voice, None);
                assert!(r.frame().scene.transition().is_none());
                assert_eq!(
                    pump(&mut r, &mut services, &mut voice, case.native(true)),
                    Status::CommandBudget
                );
            }
            assert_eq!(r.completed_commands(), prefix + 1);
            assert!(
                weak.upgrade().is_none(),
                "retired WIPE releases actual mask"
            );
            assert_eq!(
                pump(&mut r, &mut services, &mut voice, case.native(true)),
                Status::CommandBudget
            );
            assert_eq!(
                r.completed_commands(),
                prefix + 2,
                "original WIPEWAIT retires once"
            );
            assert_eq!(
                pump(&mut r, &mut services, &mut voice, case.native(true)),
                Status::Exited
            );
            assert_eq!(r.completed_commands(), prefix + 3);
            assert_eq!(
                assets
                    .reference_requested_paths()
                    .iter()
                    .filter(|p| p.as_str() == path)
                    .count(),
                requests_before + 1
            );
            assert_eq!(services.event_checkpoints, usize::from(case.protected));
            profiles += 1;
        }
    }
    // The two real masks necessarily revisit Owner::Frame after loading. Also
    // cover the initial WIPE dispatch site with the actual immediate-ready
    // default fade later in the same prologue tail; this needs no Mask worker.
    assert_eq!(
        &authored.raw()[0x5a99a..0x5a9a0],
        &[0x8e, 0, 0, 60, 0, 0x8f]
    );
    for case in [cases[0], cases[1]] {
        let mut r = staged(assets.clone(), scenario(&authored, 0x5a99a, 0x5a9a0, false));
        let mut services = Services::default();
        let mut voice = Voice::default();
        assert_eq!(
            pump(&mut r, &mut services, &mut voice, case.native(true)),
            if case.skip {
                Status::CommandBudget
            } else {
                Status::Waiting
            }
        );
        assert_eq!(r.frame().scene.transition().is_none(), case.skip);
        assert_ne!(
            services.saved.save_vectors.unlocked_cgs[746 / 32] & (1 << (746 % 32)),
            0
        );
        if !case.skip {
            r.advance(Ticks::from_f32(60.0), policy(), &mut voice, None);
            assert!(r.frame().scene.transition().is_none());
            assert_eq!(
                pump(&mut r, &mut services, &mut voice, case.native(true)),
                Status::CommandBudget
            );
        }
        assert_eq!(r.completed_commands(), 1);
        assert_eq!(
            pump(&mut r, &mut services, &mut voice, case.native(true)),
            Status::CommandBudget
        );
        assert_eq!(
            pump(&mut r, &mut services, &mut voice, case.native(true)),
            Status::Exited
        );
        assert_eq!(r.completed_commands(), 3);
    }
    println!(
        "Prologue WIPE:{profiles} actual mask8/X60 and7/UP120 native-policy profiles plus2 actual default-fade controls, fast_skip=false, real loading/paused-ready/metadata ownership, eligible and changing137d0 gates, ordinary/autoplay/unread/protected retention, exact retirement/WIPEWAIT pass; no full chapter/native rerun/audio/saves"
    );
}

#[test]
#[ignore = "authored prologue WAIT30/60 through initial and retained native read-policy dispatch; offscreen Metal, no audio/window/saves"]
fn higurashi_runner_wait_uses_native_skip_before_a_frame_boundary() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    for (start, end, amount) in [(0x5a8d0, 0x5a8d3, 30.), (0x5a91e, 0x5a921, 60.)] {
        for (
            name,
            loading_modes,
            ready_modes,
            forced,
            read_only,
            protected,
            initial_skip,
            retained_skip,
        ) in [
            ("eligible", 2, 2, false, false, false, true, true),
            ("ordinary", 0, 0, false, false, false, false, false),
            ("autoplay", 1, 1, false, false, false, false, false),
            ("unread-read-only", 2, 2, false, true, false, false, false),
            ("forced", 0, 0, true, false, false, true, true),
            ("protected", 2, 2, false, false, true, false, false),
            ("begin-skip", 0, 2, false, false, false, false, true),
            ("stop-skip", 2, 0, false, false, false, true, false),
            ("forced-protected", 0, 0, true, false, true, false, false),
        ] {
            let case = Case {
                name,
                loading_modes,
                ready_modes,
                forced,
                read_only,
                protected,
                skip: retained_skip,
            };
            let script = scenario(&authored, start, end, protected);
            let mut runner =
                shin_tasks::block_on(HigurashiRunner::load(assets.clone(), script, 0, 42, || {
                    [1, 2, 3, 4]
                }))
                .unwrap();
            let mut services = Services::default();
            let mut voice = Voice::default();
            if protected {
                assert_eq!(
                    pump(&mut runner, &mut services, &mut voice, case.native(false)),
                    Status::CommandBudget
                );
                assert_eq!(services.event_checkpoints, 1);
            }
            let status = pump(&mut runner, &mut services, &mut voice, case.native(false));
            if initial_skip {
                assert_eq!(
                    status,
                    Status::CommandBudget,
                    "{name}/{amount} initial skip"
                );
                assert!(runner.reference_wait().is_none());
            } else {
                assert_eq!(status, Status::Waiting, "{name}/{amount} initial hold");
                assert_eq!(runner.reference_wait(), Some((amount, 0)));
                assert!(matches!(runner.pending_command(), Some(Cmd::WAIT { .. })));
                let status = pump(&mut runner, &mut services, &mut voice, case.native(true));
                if retained_skip {
                    assert_eq!(
                        status,
                        Status::CommandBudget,
                        "{name}/{amount} retained skip"
                    );
                    assert!(runner.reference_wait().is_none());
                } else {
                    assert_eq!(status, Status::Waiting, "{name}/{amount} retained hold");
                    assert_eq!(runner.reference_wait(), Some((amount, 0)));
                }
            }
        }
    }
    eprintln!(
        "WAIT_POLICY_PASS 18 authored profiles: initial and retained native skip, mode changes, unread/protected/forced gates; fast_skip=false"
    );
}
