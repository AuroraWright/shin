//! Concrete Scene pending WIPE/WIPEWAIT ownership through the real
//! ReconstructionMode callback order, compared with48 frozen original runs.
//! Asset-ready empty descriptors, source/target VM ownership and audio/UI are
//! named CPU service boundaries. No GPU/audio/SDK-future implementation claim.
use super::*;
use crate::adv::higurashi::{
    reconstruction_mode::{OverlayClock, Phase, ReconstructionMode, Services},
    scene::{CommandStatus, HigurashiScene},
};
use serde::Deserialize;
use std::collections::BTreeMap;

const CORPUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/reconstruction-source-command-capture/parsed.json"
));
type Row = (i32, u32, u32, u32, u32, u64);
#[derive(Debug, Deserialize)]
struct Snapshot([u32; 14], [Vec<Row>; 2]);
#[derive(Debug, Deserialize)]
struct Native {
    header: [i32; 9],
    snapshots: BTreeMap<String, Snapshot>,
    command_drops: Vec<[u32; 5]>,
    source_drops: Vec<u32>,
    target_drops: Vec<u32>,
    lifecycle: [u32; 5],
}

fn owner(kind: usize, label: i32) -> HigurashiUserLayer {
    let mut owner = if kind == 0 {
        HigurashiUserLayer::null([11, 12, 13, 14])
    } else {
        HigurashiUserLayer::tile(
            shin_core::primitives::color::FloatColor4::from_4bpp_property(0xf123),
            glam::vec4(0., 0., 1280., 720.),
            [21, 22, 23, 24],
        )
    };
    owner.set_owner_id(label);
    for (i, &initial) in INITIAL_VALUES.iter().enumerate() {
        owner
            .properties
            .tweener_mut(i)
            .unwrap()
            .fast_forward_to((initial + (i % 5) as i32) as f32);
    }
    for (oscillator, phase) in owner
        .properties
        .oscillators
        .iter_mut()
        .zip([0.25, 0.5, 1., 2.])
    {
        oscillator.phase = phase;
    }
    owner
        .properties
        .control(0, 101., Ticks::from_f32(8.), 0)
        .unwrap();
    owner
        .properties
        .control(0, 37., Ticks::from_f32(11.), 0x80)
        .unwrap();
    owner
        .properties
        .tweener_mut(0)
        .unwrap()
        .update(Ticks::from_f32(2.));
    owner
}
fn fixture(native: &Native) -> HigurashiScene {
    let mut scene = HigurashiScene::new(HigurashiLayers::new(|| [1, 2, 3, 4]));
    scene
        .layers_mut()
        .unwrap()
        .root
        .tweener_mut(39)
        .unwrap()
        .fast_forward_to(752.);
    for _ in 0..native.header[7] {
        scene.advance(Ticks::ZERO, true, false, None);
    }
    let layers = scene.layers_mut().unwrap();
    for bank in 0..2 {
        for slot in 0..3 {
            let owner = owner(slot % 2, 1000 + 100 * bank as i32 + slot as i32);
            if slot < 2 {
                layers.retiring[bank].prepend_retiring(owner, (5 + 4 * slot) as f32);
            } else {
                layers.owners[slot][bank] = Some(owner);
            }
        }
    }
    let kind = native.header[3];
    if kind == 0 || kind == 2 {
        let mut empty = HigurashiLayers::new(|| [1, 2, 3, 4]);
        scene.reference_set_transition(empty.reference_rotate_planes().unwrap(), 1000);
    }
    let command = if kind == 2 {
        HigurashiRuntimeCommand::WIPEWAIT {}
    } else {
        HigurashiRuntimeCommand::WIPE {
            args: [0, native.header[6], native.header[5]],
            params: UntypedNumberArray::default(),
        }
    };
    assert_eq!(
        scene.dispatch(&command, false).unwrap(),
        Some(if kind == 3 {
            CommandStatus::Complete
        } else {
            CommandStatus::Pending
        })
    );
    scene
}
fn empty_ready() -> ReadyReconstruction {
    // Actual native18db0 prepares every physical FRONT, but these supplied
    // newVM records are all type-1 and every BACK job is Empty. Asset workers
    // and VM decoding remain separate previously verified transports.
    let state = LayerBanks::default();
    ReadyReconstruction {
        owners: state
            .reconstruction_plan()
            .into_iter()
            .map(|plan| (plan, None, None))
            .collect(),
        state: Box::new(state),
    }
}
fn groups(scene: &HigurashiScene) -> [Vec<(i32, Option<u32>, i32)>; 2] {
    std::array::from_fn(|bank| {
        scene
            .layers()
            .reference_group_entries(bank)
            .into_iter()
            .map(|(key, time, owner)| (key, (key < 0).then_some(time), owner.owner_id()))
            .collect()
    })
}
fn native_groups(snapshot: &Snapshot) -> [Vec<(i32, Option<u32>, i32)>; 2] {
    std::array::from_fn(|bank| {
        snapshot.1[bank]
            .iter()
            .map(|row| (row.0, (row.0 < 0).then_some(row.1), row.3 as i32))
            .collect()
    })
}
fn retained(scene: &HigurashiScene) -> Vec<(usize, usize, String)> {
    (0..2)
        .flat_map(|bank| {
            scene.layers().retiring[bank]
                .entries()
                .iter()
                .map(move |entry| {
                    let owner = entry.owner.as_ref().unwrap();
                    (
                        bank,
                        owner as *const HigurashiUserLayer as usize,
                        format!("{owner:?}"),
                    )
                })
        })
        .collect()
}

struct Pipeline {
    scene: HigurashiScene,
    kind: i32,
    source_vm: bool,
    target_vm: bool,
    animated: bool,
    prepared: Option<ReadyReconstruction>,
    constructed: bool,
    publications: u32,
    seeds: u32,
    command_drops: u32,
    source_drops: u32,
    audio_fail_once: bool,
    audio_successes: u32,
    events: Vec<&'static str>,
}
impl Pipeline {
    fn new(native: &Native) -> Self {
        Self {
            scene: fixture(native),
            kind: native.header[3],
            source_vm: true,
            target_vm: false,
            animated: true,
            prepared: Some(empty_ready()),
            constructed: false,
            publications: 0,
            seeds: 0,
            command_drops: u32::from(native.header[3] == 3),
            source_drops: 0,
            audio_fail_once: false,
            audio_successes: 0,
            events: Vec::new(),
        }
    }
    fn compare(&self, native: &Snapshot, index: usize, stage: &str) {
        let pending = self.scene.command_pending();
        let phase = if !pending || self.kind == 2 || self.scene.layers().pending_front_ids.is_some()
        {
            0
        } else {
            2
        };
        // Source/target VM booleans and animation permission are explicit
        // callback services. Concrete Scene owns the pending command andwiper.
        let expected: [u32; 9] = native.0[..9].try_into().unwrap();
        assert_eq!(
            [
                u32::from(self.source_vm),
                u32::from(pending),
                phase,
                u32::from(self.animated),
                u32::from(self.source_vm),
                u32::from(self.target_vm),
                u32::from(self.scene.transition().is_some()),
                self.command_drops,
                self.source_drops
            ],
            expected,
            "profile{index}/{stage}: native ownership"
        );
        assert_eq!(
            groups(&self.scene),
            native_groups(native),
            "profile{index}/{stage}: physical group membership/order"
        );
        let history = self.scene.history().reference_snapshot();
        assert_eq!(
            (history.1, history.2.unwrap_or(0)),
            (u64::from(native.0[10]), u64::from(native.0[11])),
            "profile{index}/{stage}: unchanged current/previous logical history"
        );
    }
}
impl Services for Pipeline {
    fn publish_resources(&mut self) -> Result<()> {
        assert!(
            self.source_vm,
            "native sourceVM isretained DURING visual publication"
        );
        if !self.constructed {
            let seeds = &mut self.seeds;
            self.scene.publish_reconstruction_in_place(
                self.prepared.take().unwrap(),
                || {
                    *seeds += 1;
                    [0; 4]
                },
                None,
            )?;
            self.constructed = true;
            self.publications += 1;
            self.events.push("resources");
        }
        assert!(
            !self.scene.transition().is_some(),
            "b905c runsbefore audio/UI/fade/sourceVMretirement"
        );
        if std::mem::take(&mut self.audio_fail_once) {
            self.events.push("audio fault");
            anyhow::bail!("Explicit desktop audio service fault afternative visual prefix");
        }
        self.audio_successes += 1;
        self.events.push("audio");
        Ok(())
    }
    fn reset_blackout_and_indicators(&mut self) {
        self.events.push("ui");
    }
    fn clear_message(&mut self) {
        panic!("Frozen source-command corpus uses no mask");
    }
    fn retire_source_vm(&mut self) {
        assert!(self.constructed && self.source_vm);
        assert!(self.scene.transition().is_none());
        let before = (
            groups(&self.scene),
            retained(&self.scene),
            self.scene.history().reference_snapshot(),
            replay_snapshot(&self.scene.layers().state),
        );
        self.command_drops += u32::from(self.scene.command_pending());
        self.scene.retire_source_command();
        assert!(!self.scene.command_pending());
        assert!(self.scene.layers().pending_front_ids.is_none());
        assert_eq!(
            (
                groups(&self.scene),
                retained(&self.scene),
                self.scene.history().reference_snapshot(),
                replay_snapshot(&self.scene.layers().state)
            ),
            before,
            "175b4→6208 retires command preparation withoutpoll/copy/rotation/VM rollback"
        );
        self.source_vm = false;
        self.source_drops += 1;
        self.events.push("sourceVM");
    }
    fn enable_animations(&mut self) {
        self.animated = true;
        self.events.push("animations");
    }
    fn install_replayed_vm(&mut self) {
        assert!(!self.source_vm && self.constructed && !self.scene.command_pending());
        self.target_vm = true;
        self.events.push("targetVM");
    }
}

#[test]
fn higurashi_source_wipe_restoration_matches_48_original_mode_lifecycles() {
    let corpus: Vec<Native> = serde_json::from_str(CORPUS).unwrap();
    assert_eq!(corpus.len(), 48);
    let (mut publications, mut cancellations, mut prefix_retirements) = (0, 0, 0);
    for (index, native) in corpus.iter().enumerate() {
        assert_eq!(native.header[0], index as i32);
        let mut pipeline = Pipeline::new(native);
        pipeline.compare(&native.snapshots["B"], index, "source livepoll");
        let mut mode = ReconstructionMode::new(1);
        let mut overlay = OverlayClock::captured(false);
        //18b78 disables animation permission but keeps source command/Scene.
        pipeline.animated = false;
        pipeline.compare(&native.snapshots["C"], index, "mode constructor");
        let before = (
            retained(&pipeline.scene),
            pipeline.scene.history().reference_snapshot(),
        );
        let pending_front = pipeline.scene.layers().pending_front_ids;
        assert_eq!(
            mode.poll(0, false, &mut overlay, &mut pipeline).unwrap(),
            Phase::Preparing
        );
        assert!(pipeline.events.is_empty());
        if native.header[4] != 0 {
            //193f4 drops only new task/factories/targetVM, leaving source intact.
            // CPU descriptor drop is its named transport; do not call Scene
            // retirement or publish_back on prepublication cancellation.
            drop(pipeline.prepared.take());
            drop(mode);
            pipeline.compare(&native.snapshots["X"], index, "cancelled beforepublication");
            assert_eq!(
                (
                    retained(&pipeline.scene),
                    pipeline.scene.history().reference_snapshot()
                ),
                before
            );
            assert_eq!(pipeline.scene.layers().pending_front_ids, pending_front);
            assert_eq!(native.source_drops, [5]);
            assert_eq!(native.target_drops, [2]);
            cancellations += 1;
        } else {
            pipeline.compare(&native.snapshots["P"], index, "ready priorpublication");
            assert_eq!(
                mode.poll(1, false, &mut overlay, &mut pipeline).unwrap(),
                Phase::Fading
            );
            pipeline.compare(&native.snapshots["A"], index, "published/sourceVMretired");
            assert_eq!(pipeline.publications, 1);
            assert_eq!(pipeline.seeds, 0);
            assert_eq!(
                pipeline.events,
                ["resources", "audio", "ui", "sourceVM", "animations"]
            );
            assert_eq!(
                (
                    retained(&pipeline.scene),
                    pipeline.scene.history().reference_snapshot()
                ),
                before
            );
            assert_eq!(native.source_drops, [3]);
            assert_eq!(native.target_drops, [5]);
            overlay.advance(Ticks::from_f32(30.));
            assert_eq!(
                mode.poll(1, false, &mut overlay, &mut pipeline).unwrap(),
                Phase::Complete
            );
            pipeline.compare(&native.snapshots["H"], index, "newVMhandedoff");
            assert_eq!(pipeline.events.last(), Some(&"targetVM"));
            assert_eq!(native.command_drops.len(), 1);
            assert_eq!(
                native.command_drops[0][0],
                if pipeline.kind == 3 { 0 } else { 3 }
            );
            assert_eq!(native.lifecycle[..3], [1, 1, 1]);
            publications += 1;
            prefix_retirements += usize::from(pending_front.is_some());
        }
    }
    assert_eq!(
        (publications, cancellations, prefix_retirements),
        (24, 24, 6)
    );
    println!(
        "Higurashi source WIPE/WIPEWAIT restoration:48 frozen original lifecycles/24 concrete Scene publications/24 prepublication descriptor cancellations; pendingphase0/phase2/wait/bit0done,negative-prefix membership,address/queued-clock retention,history,current/source/newVM callback order; SDK bitmap/future andnative outer ADV input loop remain named transports"
    );
}

#[test]
fn higurashi_source_pending_wipe_survives_acknowledged_visual_audio_retry() {
    let corpus: Vec<Native> = serde_json::from_str(CORPUS).unwrap();
    let native = corpus
        .iter()
        .find(|row| row.header[3] == 0 && row.header[4] == 0 && row.header[7] == 1)
        .unwrap();
    let mut pipeline = Pipeline::new(native);
    pipeline.animated = false;
    pipeline.audio_fail_once = true;
    let before = (
        retained(&pipeline.scene),
        pipeline.scene.history().reference_snapshot(),
    );
    let pending_front = pipeline.scene.layers().pending_front_ids;
    let mut mode = ReconstructionMode::new(1);
    let mut overlay = OverlayClock::captured(false);
    assert!(mode.poll(1, false, &mut overlay, &mut pipeline).is_err());
    assert_eq!(mode.phase(), Phase::Preparing);
    assert!(pipeline.source_vm && pipeline.scene.command_pending());
    assert!(pipeline.scene.transition().is_none());
    assert_eq!(pipeline.scene.layers().pending_front_ids, pending_front);
    assert_eq!(
        (
            retained(&pipeline.scene),
            pipeline.scene.history().reference_snapshot()
        ),
        before
    );
    assert_eq!(pipeline.events, ["resources", "audio fault"]);
    assert_eq!(pipeline.publications, 1);
    assert_eq!(
        mode.poll(1, false, &mut overlay, &mut pipeline).unwrap(),
        Phase::Fading
    );
    pipeline.compare(
        &native.snapshots["A"],
        native.header[0] as usize,
        "desktop acknowledgedprefix retry",
    );
    assert_eq!(
        pipeline.events,
        [
            "resources",
            "audio fault",
            "audio",
            "ui",
            "sourceVM",
            "animations"
        ]
    );
    assert_eq!(
        (
            pipeline.publications,
            pipeline.seeds,
            pipeline.audio_successes,
            pipeline.source_drops
        ),
        (1, 0, 1, 1)
    );
    assert!(
        !pipeline.scene.command_pending() && pipeline.scene.layers().pending_front_ids.is_none()
    );
    // Error/retry is an explicit desktop fault boundary, not an original
    // exception execution claim. Actual resources andsourcecommand are used.
}
