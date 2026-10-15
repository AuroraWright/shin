use super::*;
use std::{
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    operations: usize,
    words: usize,
}
impl Native {
    fn new() -> Self {
        let mut child = Command::new(std::env::var_os("SHIN_GEROKASU2_LOAD_ORACLE").unwrap())
            .args([
                std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_MEDIUM_FONT").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_SYSTEM_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
            operations: 0,
            words: 0,
        }
    }
    fn send(&mut self, words: &[u32]) {
        self.input
            .as_mut()
            .unwrap()
            .write_all(
                &words
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    fn compare(&mut self, tag: &[u8; 4], words: &[u32]) {
        let mut bytes = vec![0; 4 + words.len() * 4];
        self.output.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes[..4], tag);
        let actual = bytes[4..]
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(actual, words);
        self.operations += 1;
        self.words += words.len();
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        self.input.take();
        let status = self.child.wait().unwrap();
        if !std::thread::panicking() {
            assert!(status.success());
        }
    }
}

#[derive(Default)]
struct Services {
    elapsed: u64,
    wait_advance: u64,
    resource: bool,
    resource_ready: bool,
    transition_present: bool,
    transition_ready: bool,
    transition: LoadTransition,
    animations: bool,
    wipes: u64,
    events: Vec<u32>,
    plane: u32,
    property_count: u32,
    skip: bool,
    fail_once: Option<u32>,
}
impl Services {
    fn event(&mut self, code: u32, value: u32) {
        self.events.extend([code, value]);
    }
    fn fail(&mut self, code: u32) -> Result<()> {
        if self.fail_once == Some(code) {
            self.fail_once = None;
            anyhow::bail!("Injected load service failure {code}");
        }
        Ok(())
    }
}
impl LoadServices for Services {
    fn frame_elapsed_us(&self) -> u64 {
        self.elapsed
    }
    fn prepare_plan(
        &mut self,
        _: &mut Gerokasu2Layers,
        _: &Gerokasu2RuntimeCommand,
    ) -> Result<LoadPlan> {
        unreachable!()
    }
    fn start_resource(&mut self, p: &LoadPlan) -> Result<()> {
        self.event(1, p.kind as u32);
        self.resource = true;
        Ok(())
    }
    fn resource_present(&self) -> bool {
        self.resource
    }
    fn wait_resource(&mut self, t: u32) -> Result<()> {
        self.event(5, t);
        self.elapsed += self.wait_advance;
        self.fail(5)
    }
    fn resource_ready(&mut self) -> Result<bool> {
        self.event(3, self.resource_ready as u32);
        Ok(self.resource_ready)
    }
    fn set_animations_allowed(&mut self, a: bool) {
        self.animations = a;
    }
    fn animations_allowed(&self) -> bool {
        self.animations
    }
    fn transition(&self) -> LoadTransition {
        self.transition
    }
    fn configure_transition(&mut self, settings: LoadTransition) -> Result<()> {
        self.transition = settings;
        Ok(())
    }
    fn start_transition(&mut self, t: LoadTransition) -> Result<()> {
        self.event(2, t.kind as u32);
        self.transition_present = (0..=9).contains(&t.kind);
        Ok(())
    }
    fn transition_present(&self) -> bool {
        self.transition_present
    }
    fn wait_transition(&mut self, t: u32) -> Result<()> {
        self.event(6, t);
        self.elapsed += self.wait_advance;
        self.fail(6)
    }
    fn clear_transition(&mut self) {
        self.transition_present = false;
    }
    fn transition_ready(&mut self) -> Result<bool> {
        self.event(4, self.transition_ready as u32);
        Ok(self.transition_ready)
    }
    fn skip_requested(&mut self) -> bool {
        self.event(7, self.skip as u32);
        self.skip
    }
    fn publish(&mut self, p: &LoadPlan, with_transition: bool) -> Result<()> {
        self.event(8, p.plane.raw() as u32);
        for target in &p.targets {
            let bank = u32::from(target.bank.raw());
            self.event(9, bank);
            if target.already_same {
                self.event(10, bank);
            } else {
                self.event(11, bank);
            }
            let property = if target.keep_properties {
                self.event(13, bank + 1);
                if with_transition && p.flags & 8 == 0 {
                    self.event(15, bank + 1);
                    self.property_count += 1;
                    self.property_count
                } else {
                    bank + 1
                }
            } else {
                self.property_count += 1;
                self.event(14, self.property_count);
                self.event(16, target.logical.raw() as u32);
                self.property_count
            };
            self.event(17, target.load_counter);
            self.event(18, property);
            if with_transition {
                self.event(12, bank);
            }
            self.event(19, bank);
            self.event(20, with_transition as u32);
        }
        Ok(())
    }
    fn loaded_layer(&mut self, kind: i32, _: [i32; 8]) -> Result<()> {
        self.event(22, kind as u32);
        self.fail(22)
    }
    fn any_target_wiping(&mut self, targets: &[LoadTarget]) -> Result<bool> {
        self.event(8, self.plane);
        for t in targets {
            let bank = u32::from(t.bank.raw());
            self.event(21, bank);
            if self.wipes & (1u64 << bank) != 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[test]
fn retained_load_retries_do_not_resubmit_jobs_or_republish_owners() {
    for failure in [5, 6, 22] {
        let mut layers = Gerokasu2Layers::default();
        let mut counters = LoadCounters::new(0, [0; 48]);
        let plan = LoadPlan::prepare(
            &mut layers,
            &mut counters,
            &Gerokasu2RuntimeCommand::LayerLoad {
                layer_id: 0,
                layer_type: 1,
                flags: 4,
                params: (0, 0, 0, 0, 0, 0, 0, 0),
            },
        )
        .unwrap();
        let mut controller = LayerLoadController::new(plan);
        let mut services = Services {
            resource_ready: true,
            transition_ready: true,
            transition: LoadTransition {
                duration: 60,
                ..Default::default()
            },
            property_count: 48,
            fail_once: Some(failure),
            ..Default::default()
        };
        assert!(controller.poll(&mut services).is_err());
        assert!(controller.poll(&mut services).unwrap());
        assert!(controller.poll(&mut services).unwrap());
        for code in [1, 2, 5, 6, 19] {
            assert_eq!(
                services
                    .events
                    .chunks_exact(2)
                    .filter(|e| e[0] == code)
                    .count(),
                1,
                "service {code} repeated after failure {failure}"
            );
        }
        assert_eq!(
            services
                .events
                .chunks_exact(2)
                .filter(|e| e[0] == 22)
                .count(),
            if failure == 22 { 2 } else { 1 }
        );
        assert!(services.animations);
        assert_eq!(counters.next(), 1);
    }
    println!(
        "GEROKASU2_LOAD_RETRY warm_resource_warm_transition_gallery_failure_retained_jobs_publication_counters=verified cases=3"
    );
}

#[test]
#[ignore = "requires supplied executable/fonts and complete original LAYERLOAD oracle;CPU only"]
fn native_layer_load_constructor_and_complete_poll_phases() {
    let mut native = Native::new();
    let mut constructors = 0;
    let mut polls = 0;
    for flags in 0..32 {
        for (old_kind, kind, same_params, mixed) in [
            (-1, 1, false, false),
            (1, 1, true, false),
            (1, 1, false, false),
            (9, 9, false, false),
            (1, 1, true, true),
        ] {
            for count in [1, 3, 48] {
                let selected = count != 1;
                let mut layers = Gerokasu2Layers::default();
                let mut banks = [0u32; 48];
                let mut records = Vec::new();
                for id in 0..count {
                    let params = if same_params && !(mixed && id % 2 == 1) {
                        [17; 8]
                    } else {
                        [18; 8]
                    };
                    let target = layers.load(id, old_kind, 0, params).unwrap()[0];
                    banks[usize::from(target.layerbank.raw())] = 100 + id as u32;
                    records.push([
                        id as u32,
                        old_kind as u32,
                        100 + id as u32,
                        params[0] as u32,
                        params[1] as u32,
                        params[2] as u32,
                        params[3] as u32,
                        params[4] as u32,
                        params[5] as u32,
                        params[6] as u32,
                        params[7] as u32,
                        0,
                        0,
                        0,
                    ]);
                }
                if selected {
                    layers.select(0, count - 1);
                }
                let before_init = layers.loads_with_init();
                let next = if flags & 16 == 0 { 400 } else { u32::MAX };
                let mut counters = LoadCounters::new(next, banks);
                if flags & 16 != 0 {
                    layers.begin_back();
                }
                let command = Gerokasu2RuntimeCommand::LayerLoad {
                    layer_id: if selected { -6 } else { 0 },
                    layer_type: kind,
                    flags,
                    params: (17, 17, 17, 17, 17, 17, 17, 17),
                };
                let plan = LoadPlan::prepare(&mut layers, &mut counters, &command).unwrap();
                let selected_word = ((count - 1) as u32) << 16;
                native.send(&[
                    0,
                    count as u32,
                    next,
                    selected_word,
                    (flags & 16 != 0) as u32,
                    if selected { (-6i32) as u32 } else { 0 },
                    kind as u32,
                    flags as u32,
                    17,
                    17,
                    17,
                    17,
                    17,
                    17,
                    17,
                    17,
                ]);
                for record in records {
                    native.send(&record);
                }
                let mut expected = vec![
                    plan.flags as u32,
                    plan.plane.raw() as u32,
                    plan.targets.len() as u32,
                    (!plan.needs_loader) as u32,
                    counters.next(),
                    layers.loads_with_init().wrapping_sub(before_init),
                ];
                for t in &plan.targets {
                    expected.extend([
                        t.bank.raw() as u32,
                        t.logical.raw() as u32,
                        t.load_counter,
                        t.keep_properties as u32,
                        t.already_same as u32,
                    ]);
                }
                native.compare(b"GLC1", &expected);
                constructors += 1;
                let mut controller = LayerLoadController::new(plan);
                let mut services = Services {
                    animations: true,
                    plane: 0,
                    property_count: 48,
                    ..Default::default()
                };
                for (
                    elapsed,
                    wait,
                    ready,
                    transition_ready,
                    skip,
                    wipes,
                    ty,
                    transition_flags,
                    duration,
                ) in [
                    (
                        match count {
                            1 => 0,
                            3 => 9999,
                            _ => 10000,
                        },
                        5000,
                        false,
                        false,
                        count == 48,
                        0u64,
                        0,
                        0,
                        50,
                    ),
                    (9999, 5000, true, false, false, 1, 0, 0, 50),
                    (10000, 0, true, false, false, 1, 0, 1, 50),
                    (0, 0, true, false, true, 1, 0, 0, 50),
                    (u32::MAX, 0, true, true, false, 1, 0, 2, 50),
                    (0, 0, true, true, false, 0, 0, 0, 50),
                ] {
                    native.send(&[
                        1,
                        elapsed,
                        wait,
                        ready as u32,
                        transition_ready as u32,
                        skip as u32,
                        wipes as u32,
                        ty,
                        transition_flags,
                        duration,
                        (wipes >> 32) as u32,
                        0,
                        0,
                        0,
                        0,
                        0,
                    ]);
                    services.elapsed = u64::from(elapsed);
                    services.wait_advance = u64::from(wait);
                    services.resource_ready = ready;
                    services.transition_ready = transition_ready;
                    services.wipes = wipes;
                    services.transition = LoadTransition {
                        kind: ty as i32,
                        flags: transition_flags as i32,
                        duration: duration as i32,
                        params: [0; 8],
                    };
                    services.events.clear();
                    services.skip = skip;
                    let done = controller.poll(&mut services).unwrap();
                    let events = services.events.clone();
                    let stage = match controller.stage() {
                        LoadStage::Start => 0,
                        LoadStage::Resource => 1,
                        LoadStage::Transition => 2,
                        LoadStage::Publication => 3,
                        LoadStage::Wipes => 4,
                        LoadStage::Complete => u32::MAX,
                    };
                    let mut out = vec![
                        done as u32,
                        stage,
                        services.animations as u32,
                        services.resource as u32,
                        services.transition_present as u32,
                        events.len() as u32,
                    ];
                    out.extend(events);
                    native.compare(b"GLP1", &out);
                    polls += 1;
                }
            }
        }
    }
    println!(
        "GEROKASU2_LOAD_NATIVE constructors={constructors} polls={polls} snapshots={} words={} original_complete_ctor_poll_reuse_keep_counter_quality_ready_wait_skip_publication_wipe_flags=verified",
        native.operations, native.words
    );
}

#[test]
#[ignore = "requires supplied executable/fonts and original TRANSSET constructor/poll;CPU only"]
fn native_transition_settings_retain_all_raw_words_and_complete() {
    let mut native = Native::new();
    let mut services = Services::default();
    let mut seed = 0x52415731u32;
    for case in 0..4096 {
        let mut values = [0i32; 11];
        for value in &mut values {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *value = seed as i32;
        }
        if case < 64 {
            values[0] = case % 16 - 3;
            values[1] = case;
            values[2] = case - 32;
        }
        let settings = LoadTransition {
            kind: values[0],
            flags: values[1],
            duration: values[2],
            params: values[3..].try_into().unwrap(),
        };
        services.configure_transition(settings).unwrap();
        let actual = services.transition();
        let mut row = vec![2];
        row.extend(values.map(|v| v as u32));
        row.extend([0; 4]);
        native.send(&row);
        let mut expected = vec![
            actual.kind as u32,
            actual.flags as u32,
            actual.duration as u32,
        ];
        expected.extend(actual.params.map(|v| v as u32));
        expected.push(1);
        native.compare(b"GLT1", &expected);
    }
    println!(
        "GEROKASU2_TRANSSET_NATIVE cases=4096 words=49152 original_ctor_raw_type_flags_signed_duration_all8_params_immediate_poll=verified"
    );
}
