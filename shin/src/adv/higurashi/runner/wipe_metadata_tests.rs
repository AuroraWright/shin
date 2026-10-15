//! NEW actual back-PIC publication into native ReadServices, including a
//! checkpoint in the same pump and metadata failure/retry after publication.
//! Original gallery20d20 bit semantics use existing native acceptance; this
//! closes the Runner interception/acknowledgement integration gap only.
use super::super::{input::ReadState, read_state::Checkpoint};
use super::{
    tests::{Voice, policy},
    *,
};
use binrw::BinWrite;
use shin_core::format::{
    save::higurashi::SaveData, scenario::instruction_elements::UntypedNumberSpec,
};

struct Services {
    data: SaveData,
    before: shin_core::format::save::higurashi::SaveVectors,
    ids: Vec<usize>,
    settings_available: bool,
    checkpoints: usize,
}
impl Services {
    fn new(scenario: &HigurashiScenario, picture: usize, available: bool) -> Self {
        let mut data = SaveData::default();
        // Native vectors retain existing bits/padding independently of the
        // requested and optionally linked IDs acquired at front publication.
        data.save_vectors.unlocked_cgs = vec![0x80000000, 0x01000000];
        let before = data.save_vectors.clone();
        let mut ids = vec![picture];
        if let Ok(linked) = usize::try_from(scenario.picture_info[picture].linked_cg_id) {
            ids.push(linked);
        }
        Self {
            data,
            before,
            ids,
            settings_available: available,
            checkpoints: 0,
        }
    }
    fn assert_gallery(&self) {
        let words = &self.data.save_vectors.unlocked_cgs;
        assert_eq!(words.len() % 256, 0, "original gallery256-word growth");
        for &id in &self.ids {
            assert_ne!(
                words[id / 32] & (1 << (id % 32)),
                0,
                "WIPE requested/linked CG{id} missing at next constructor"
            );
        }
        for (at, &word) in words.iter().enumerate() {
            let mut expected = self.before.unlocked_cgs.get(at).copied().unwrap_or(0);
            for &id in &self.ids {
                if id / 32 == at {
                    expected |= 1 << (id % 32);
                }
            }
            assert_eq!(word, expected, "gallery unused/existing word{at}");
        }
        let mut other = self.data.save_vectors.clone();
        other.unlocked_cgs = self.before.unlocked_cgs.clone();
        assert_eq!(other, self.before, "WIPE acquired unrelated vectors");
    }
}
impl ReadServices for Services {
    fn layer_settings(&mut self) -> Option<&mut SaveData> {
        self.settings_available.then_some(&mut self.data)
    }
    fn checkpoint(&mut self, kind: Checkpoint, _: ReadState) -> Result<()> {
        assert_eq!(kind, Checkpoint::EventBegin);
        // This ACTUAL second command callback must observe the PIC promoted
        // by the first command in the SAME pump_native invocation.
        self.assert_gallery();
        self.checkpoints += 1;
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
}
fn number(code: &mut Vec<u8>, value: i32) {
    let mut out = std::io::Cursor::new(Vec::new());
    UntypedNumberSpec::Constant(value)
        .write_le(&mut out)
        .unwrap();
    code.extend(out.into_inner());
}
fn scenario(
    authored: &HigurashiScenario,
    kind: i32,
    flags: i32,
    duration: i32,
) -> Arc<HigurashiScenario> {
    let mut code = vec![0x8e];
    for value in [kind, flags, duration] {
        number(&mut code, value);
    }
    if kind == 1 {
        code.push(1);
        number(&mut code, 65535);
    } else {
        code.push(0);
    }
    code.extend([0xa2, 0, 0, 0, 0]); // EVBEGIN0; EXIT0,0.
    let mut raw = authored.raw().to_vec();
    let at = authored.header.code_offset as usize;
    raw[at..at + code.len()].copy_from_slice(&code);
    Arc::new(HigurashiScenario::new(raw.into()).unwrap())
}
fn staged(
    assets: Arc<AssetServer>,
    scenario: Arc<HigurashiScenario>,
    picture: i32,
) -> HigurashiRunner {
    let mut r = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        scenario.clone(),
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    // Ordinary front Tile then an actual back PIC load, not a synthesized
    // pending-metadata row or a prepared/default Wiper.
    for command in [
        Cmd::LAYERLOAD {
            args: [1, 1, 0],
            params: (0xf800, -640, -720, 1280, 720, 0, 0, 0),
        },
        Cmd::LAYERBACK { argument: 1 },
        Cmd::LAYERLOAD {
            args: [1, 2, 0],
            params: (picture, 0, 0, 0, 0, 0, 0, 0),
        },
    ] {
        assert!(
            shin_tasks::block_on(
                r.reference_frame_mut()
                    .scene
                    .layers_mut()
                    .unwrap()
                    .dispatch(&command, &assets, &scenario, || [1, 2, 3, 4])
            )
            .unwrap()
        );
    }
    assert_eq!(
        r.frame().scene.layers().reference_group_entries(0)[0]
            .2
            .layer_type()
            .native_id(),
        1
    );
    assert_eq!(
        r.frame().scene.layers().reference_group_entries(1)[0]
            .2
            .layer_type()
            .native_id(),
        2
    );
    r
}
fn pump(
    r: &mut HigurashiRunner,
    p: Policy,
    services: &mut Services,
    voice: &mut Voice,
) -> Result<Status> {
    shin_tasks::block_on(r.pump_native(
        p,
        ReadPolicy {
            modes: 0,
            forced: false,
            read_only: true,
        },
        services,
        2,
        100,
        || panic!("WIPE sampled layer seeds"),
        voice,
    ))
}
fn published(r: &HigurashiRunner) -> [Vec<(i32, i32, i32, Vec<u32>)>; 2] {
    std::array::from_fn(|bank| {
        r.frame()
            .scene
            .layers()
            .reference_group_entries(bank)
            .into_iter()
            .map(|(key, _, owner)| {
                (
                    key,
                    owner.owner_id(),
                    owner.layer_type().native_id(),
                    owner
                        .properties
                        .values()
                        .iter()
                        .map(|v| v.to_bits())
                        .collect(),
                )
            })
            .collect()
    })
}

#[test]
#[ignore = "real back PIC and native ReadServices checkpoint within actual Runner pump; offscreen asset services/no audio"]
fn higurashi_runner_immediate_wipe_commits_actual_pic_metadata_before_same_pump_checkpoint_and_retry()
 {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let picture = 424usize; // Real supplied PIC, also used by retained loading acceptance.
    assert!(authored.picture_info.get(picture).is_some());
    let mut voice = Voice::default();
    for (kind, flags, duration, skip) in [
        (0, 0, 0, false),
        (0, 1, 8, false),
        (0, 0, 8, true),
        (1, 1, 8, false),
    ] {
        let scenario = scenario(&authored, kind, flags, duration);
        let mut r = staged(assets.clone(), scenario.clone(), picture as i32);
        let mut services = Services::new(&scenario, picture, true);
        assert_eq!(services.checkpoints, 0);
        assert_eq!(services.data.save_vectors, services.before);
        let mut p = policy();
        p.fast_skip = skip;
        // Before the branch correction, requested bit424 is absent when
        // EVBEGIN's actual checkpoint runs, and this assertion fails there.
        assert_eq!(
            pump(&mut r, p, &mut services, &mut voice).unwrap(),
            Status::CommandBudget
        );
        assert_eq!(services.checkpoints, 1);
        assert_eq!(r.completed_commands(), 2);
        services.assert_gallery();
        assert_eq!(published(&r)[0][0].2, 2);
        assert!(published(&r)[1].is_empty());
        assert_eq!(
            r.frame().scene.transition().is_some(),
            duration > 0 && !skip
        );
        assert_eq!(
            pump(&mut r, policy(), &mut services, &mut voice).unwrap(),
            Status::Exited
        );
    }
    // Real queued metadata errors AFTER the first plane publication because
    // valid PIC gallery acquisition requires actual global settings.
    let scenario = scenario(&authored, 0, 1, 8);
    let mut r = staged(assets.clone(), scenario.clone(), picture as i32);
    let mut services = Services::new(&scenario, picture, false);
    let error = pump(&mut r, policy(), &mut services, &mut voice)
        .unwrap_err()
        .to_string();
    assert!(error.contains("front picture metadata requires actual settings"));
    assert_eq!(services.checkpoints, 0);
    assert_eq!(services.data.save_vectors, services.before);
    assert_eq!(r.completed_commands(), 0);
    assert!(matches!(r.pending_command(), Some(Cmd::WIPE { .. })));
    assert!(matches!(r.pending.as_ref().unwrap().owner, Owner::Frame));
    assert!(!r.frame().scene.command_pending());
    let saved = (r.position(), r.next_position(), r.random_state());
    let owners = published(&r);
    let banks = super::super::layers::replay_snapshot(&r.frame().scene.layers().state);
    let transition = r.frame().scene.transition().unwrap();
    let slot = owners[0][0].0 as u8;
    assert_eq!(
        transition
            .previous
            .layer(slot)
            .unwrap()
            .layer_type()
            .native_id(),
        1
    );
    assert_eq!(transition.progress().to_bits(), 0f32.to_bits());
    assert_eq!(
        pump(&mut r, policy(), &mut services, &mut voice)
            .unwrap_err()
            .to_string(),
        error
    );
    assert_eq!((r.position(), r.next_position(), r.random_state()), saved);
    assert_eq!(published(&r), owners);
    assert_eq!(
        super::super::layers::replay_snapshot(&r.frame().scene.layers().state),
        banks
    );
    services.settings_available = true;
    assert_eq!(
        pump(&mut r, policy(), &mut services, &mut voice).unwrap(),
        Status::CommandBudget
    );
    assert_eq!(services.checkpoints, 1);
    assert_eq!(r.completed_commands(), 2);
    services.assert_gallery();
    assert_eq!(published(&r), owners);
    assert_eq!(
        super::super::layers::replay_snapshot(&r.frame().scene.layers().state),
        banks
    );
    let transition = r.frame().scene.transition().unwrap();
    assert_eq!(
        transition
            .previous
            .layer(slot)
            .unwrap()
            .layer_type()
            .native_id(),
        1,
        "retry rotated old PIC into outgoing again"
    );
    assert_eq!(transition.progress().to_bits(), 0f32.to_bits());
    // Failed polls preserve the WIPE's RNG state above. The successful retry
    // then executes EVBEGIN, which legitimately advances the VM RNG again.
    let mut expected = HigurashiScripter::new(&scenario, 0, 42).unwrap();
    assert!(matches!(
        expected.run(CommandResult::None, 100).unwrap(),
        Cmd::WIPE { .. }
    ));
    assert!(matches!(
        expected.run(CommandResult::None, 100).unwrap(),
        Cmd::EVBEGIN { .. }
    ));
    assert_eq!(r.random_state(), expected.random_state());
    assert_eq!(
        pump(&mut r, policy(), &mut services, &mut voice).unwrap(),
        Status::Exited
    );
    println!(
        "Higurashi actual Runner/nativeReadServices: four immediateWIPE paths commit staged realPIC requested/linkedCG+padding before same-pump EVBEGIN; actual metadata-service failure retains OwnerFrame, PC/PRNG/physicalbanks/outgoingTile/clock and retries without second publication; no audio"
    );
}
