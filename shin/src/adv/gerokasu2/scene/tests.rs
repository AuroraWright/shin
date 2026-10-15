use super::*;
use crate::adv::gerokasu2::{
    layer_load::{LoadPlan, LoadTarget},
    layers::Gerokasu2Layers,
};
use shin_core::vm::gerokasu2::Gerokasu2RuntimeCommand;
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
        let mut child = Command::new(std::env::var_os("SHIN_GEROKASU2_SCENE_ORACLE").unwrap())
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
    fn compare(&mut self, row: [u32; 16], tag: &[u8; 4], expected: &[u32]) {
        self.input
            .as_mut()
            .unwrap()
            .write_all(
                &row.into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let mut actual = vec![0; 4 + expected.len() * 4];
        self.output.read_exact(&mut actual).unwrap();
        assert_eq!(&actual[..4], tag);
        assert_eq!(
            actual[4..]
                .chunks_exact(4)
                .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
                .collect::<Vec<_>>(),
            expected
        );
        self.operations += 1;
        self.words += expected.len();
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
    job: bool,
    ready: bool,
    skip: bool,
    active: bool,
    previous: bool,
    animations: bool,
    events: Vec<u32>,
}
impl Services {
    fn event(&mut self, code: u32, value: u32) {
        self.events.extend([code, value]);
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
    fn start_resource(&mut self, _: &LoadPlan) -> Result<()> {
        unreachable!()
    }
    fn resource_present(&self) -> bool {
        unreachable!()
    }
    fn wait_resource(&mut self, _: u32) -> Result<()> {
        unreachable!()
    }
    fn resource_ready(&mut self) -> Result<bool> {
        unreachable!()
    }
    fn set_animations_allowed(&mut self, allowed: bool) {
        self.animations = allowed;
    }
    fn animations_allowed(&self) -> bool {
        self.animations
    }
    fn transition(&self) -> LoadTransition {
        LoadTransition {
            flags: 2,
            ..Default::default()
        }
    }
    fn configure_transition(&mut self, _: LoadTransition) -> Result<()> {
        unreachable!()
    }
    fn start_transition(&mut self, settings: LoadTransition) -> Result<()> {
        self.event(2, settings.kind as u32);
        self.job = (0..=9).contains(&settings.kind);
        Ok(())
    }
    fn transition_present(&self) -> bool {
        self.job
    }
    fn wait_transition(&mut self, micros: u32) -> Result<()> {
        self.event(3, micros);
        Ok(())
    }
    fn clear_transition(&mut self) {
        self.job = false;
    }
    fn transition_ready(&mut self) -> Result<bool> {
        self.event(5, self.ready as u32);
        Ok(self.ready)
    }
    fn skip_requested(&mut self) -> bool {
        self.event(4, self.skip as u32);
        self.skip
    }
    fn publish(&mut self, _: &LoadPlan, _: bool) -> Result<()> {
        unreachable!()
    }
    fn loaded_layer(&mut self, _: i32, _: [i32; 8]) -> Result<()> {
        unreachable!()
    }
    fn any_target_wiping(&mut self, _: &[LoadTarget]) -> Result<bool> {
        unreachable!()
    }
}
impl SceneServices for Services {
    fn observe_screen(&mut self) -> Result<()> {
        self.event(1, 0);
        Ok(())
    }
    fn capture_previous_scene(&mut self) -> Result<()> {
        if !self.previous {
            self.event(6, 0);
            self.previous = true;
        }
        Ok(())
    }
    fn pageback(&mut self, _: bool) -> Result<()> {
        self.capture_previous_scene()?;
        self.observe_screen()?;
        self.event(9, 0);
        Ok(())
    }
    fn promote_pending_page(&mut self, with: bool) -> Result<()> {
        self.observe_screen()?;
        if with {
            self.event(10, 1);
        }
        self.event(7, with as u32);
        Ok(())
    }
    fn scene_wiping(&mut self) -> Result<bool> {
        self.observe_screen()?;
        self.event(8, self.active as u32);
        Ok(self.active)
    }
}

#[test]
#[ignore = "requires supplied executable/fonts and original scene constructor/poll oracle;CPU only"]
fn native_wipe_pageback_and_wait_phases_match_retained_controller() {
    let mut native = Native::new();
    let mut constructors = 0;
    let mut polls = 0;
    for flags in 0..8 {
        for kind in [-1, 0, 1, 9, 10, i32::MIN, i32::MAX] {
            for duration in [-1, 0, 1, 60] {
                for back in [false, true] {
                    for previous in [false, true] {
                        for elapsed in [0, 9999, 10000] {
                            let settings = LoadTransition {
                                kind,
                                flags,
                                duration,
                                params: [-1, 2, 3, 4, 5, 6, 7, 8],
                            };
                            let mut controller = WipeController::new(back, settings);
                            let mut services = Services {
                                animations: true,
                                previous,
                                ..Default::default()
                            };
                            let mut row = [0u32; 16];
                            row[1] = back as u32;
                            row[2] = previous as u32;
                            row[3] = kind as u32;
                            row[4] = flags as u32;
                            row[5] = duration as u32;
                            row[6..14].copy_from_slice(&settings.params.map(|v| v as u32));
                            native.compare(row, b"GWC1", &[0, if back { 0 } else { u32::MAX }]);
                            constructors += 1;
                            for (clock, ready, skip, active) in [
                                (elapsed, false, flags & 4 != 0, true),
                                (9999, false, false, true),
                                (10000, true, false, true),
                                (0, true, false, true),
                                (u32::MAX, true, false, false),
                                (0, true, true, false),
                            ] {
                                services.elapsed = u64::from(clock);
                                services.ready = ready;
                                services.skip = skip;
                                services.active = active;
                                services.events.clear();
                                let done = controller.poll(&mut services).unwrap();
                                let stage = match controller.stage() {
                                    WipeStage::Start => 0,
                                    WipeStage::Promotion => 1,
                                    WipeStage::Wipes => 2,
                                    WipeStage::Complete => u32::MAX,
                                };
                                let mut expected = vec![
                                    done as u32,
                                    stage,
                                    services.animations as u32,
                                    services.job as u32,
                                    services.previous as u32,
                                    services.events.len() as u32,
                                ];
                                expected.extend(&services.events);
                                let mut row = [0; 16];
                                row[..5].copy_from_slice(&[
                                    1,
                                    clock,
                                    ready as u32,
                                    skip as u32,
                                    active as u32,
                                ]);
                                native.compare(row, b"GWP1", &expected);
                                polls += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    for back in [false, true] {
        for previous in [false, true] {
            let mut services = Services {
                previous,
                ..Default::default()
            };
            if !back {
                services.pageback(false).unwrap();
            }
            let mut expected = vec![1, 1, services.previous as u32, services.events.len() as u32];
            expected.extend(&services.events);
            let mut row = [0; 16];
            row[..3].copy_from_slice(&[2, back as u32, previous as u32]);
            native.compare(row, b"GPB1", &expected);
        }
    }
    for active in [false, true] {
        let mut services = Services {
            active,
            ..Default::default()
        };
        let done = !services.scene_wiping().unwrap();
        let mut expected = vec![done as u32, services.events.len() as u32];
        expected.extend(&services.events);
        let mut row = [0; 16];
        row[0] = 3;
        row[4] = active as u32;
        native.compare(row, b"GWW1", &expected);
    }
    println!(
        "GEROKASU2_SCENE_NATIVE constructors={constructors} polls={polls} snapshots={} words={} original_WIPE_PAGEBACK_WIPEWAIT_phases_flags_budget_skip_readiness_history_promotion_wait=verified",
        native.operations, native.words
    );
}
