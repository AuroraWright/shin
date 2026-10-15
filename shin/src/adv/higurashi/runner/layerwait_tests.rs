//! Actual updated helper bytecode, through both ordinary and native-read VM pumps.
use super::{
    tests::{Voice, policy},
    *,
};

struct Read;
impl ReadServices for Read {
    fn checkpoint(
        &mut self,
        _: super::super::read_state::Checkpoint,
        _: super::super::input::ReadState,
    ) -> Result<()> {
        panic!("this authored control/wait/signal slice has no checkpoint constructor");
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        panic!("the slice contains no MSGSET");
    }
}

fn pump(
    runner: &mut HigurashiRunner,
    native: bool,
    policy: Policy,
    budget: usize,
    voice: &mut Voice,
) -> Status {
    let no_seeds = || panic!("LAYERCTRL/LAYERWAIT/MSGSIGNAL consumes no owner entropy");
    if native {
        shin_tasks::block_on(runner.pump_native(
            policy,
            ReadPolicy {
                modes: 0,
                forced: false,
                read_only: false,
            },
            &mut Read,
            budget,
            100_000,
            no_seeds,
            voice,
        ))
    } else {
        shin_tasks::block_on(runner.pump(policy, budget, 100_000, no_seeds, voice))
    }
    .unwrap()
}

#[test]
#[ignore = "original updated Bonus CALL/helper/wait bytecode with offscreen Metal message resources; no OS audio/window/save"]
fn higurashi_runner_layerwait_runs_authored_bonus_helper_and_resumes_exactly_once() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reverse-engineering/higurashi/layerwait-authored-helper-disassembly.txt"
    ));
    let mut checked_bytes = 0;
    for line in source.lines() {
        let words: Vec<_> = line.split_whitespace().collect();
        let start = usize::from_str_radix(words[0], 16).unwrap();
        let end = usize::from_str_radix(words[1], 16).unwrap();
        if (0x57e1a..0x57e79).contains(&start) || (0x1750b23..0x1750b3a).contains(&start) {
            let raw = line.split(" raw=").nth(1).unwrap();
            let bytes: Vec<_> = (0..raw.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&raw[i..i + 2], 16).unwrap())
                .collect();
            assert_eq!(
                &authored.raw()[start..end],
                bytes,
                "original helper/caller {start:x}"
            );
            checked_bytes += bytes.len();
        }
    }
    assert_eq!(checked_bytes, 0x57e79 - 0x57e1a + 0x1750b3a - 0x1750b23);
    assert_eq!(&authored.raw()[0x57e73..0x57e78], &[0xc4, 0xd0, 2, 28, 31]);
    assert_eq!(authored.raw()[0x1750b39], 0x88); // Original MSGSIGNAL.
    for native in [false, true] {
        for fast_skip in [false, true] {
            // Start at the original two CALLs. Their actual arguments drive the
            // same authored four-control helper twice: Page then Screen root.
            // All helper branches, argument registers, return stacks, wait and
            // next MSGSIGNAL remain original. This is a bounded helper fixture,
            // not evidence of an uninterrupted entire Bonus chapter.
            let mut raw = authored.raw().to_vec();
            let entry = authored.header.code_offset as usize;
            raw[entry] = 0x47;
            raw[entry + 1..entry + 5].copy_from_slice(&0x01750b23u32.to_le_bytes());
            let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
            let mut runner = shin_tasks::block_on(HigurashiRunner::load(
                assets.clone(),
                scenario,
                0,
                42,
                || [1, 2, 3, 4],
            ))
            .unwrap();
            let mut voice = Voice::default();
            let mut active = policy();
            active.fast_skip = fast_skip;
            assert_eq!(
                pump(&mut runner, native, active, 32, &mut voice),
                Status::Waiting
            );
            assert_eq!(runner.completed_commands(), 8);
            assert_eq!(runner.position().0, 0x57e73);
            assert_eq!(runner.next_position().0, 0x57e78);
            assert!(matches!(
                runner.pending.as_ref().unwrap().owner,
                Owner::LayerWait(_)
            ));
            assert!(
                matches!(runner.pending_command(), Some(Cmd::LAYERWAIT { id: -1, properties }) if properties.as_slice() == [28, 31])
            );
            assert_eq!(
                runner.frame.scene.layers().screen_properties().values()[31],
                40.
            );
            assert_eq!(
                runner.frame.scene.layers().message_properties().values()[31],
                40.
            );
            let seed = runner.random_state();
            for _ in 0..3 {
                assert_eq!(
                    pump(&mut runner, native, active, 32, &mut voice),
                    Status::Waiting
                );
                assert_eq!(runner.random_state(), seed);
                assert_eq!(runner.completed_commands(), 8);
                assert_eq!(
                    runner.frame.scene.layers().screen_properties().values()[31],
                    40.
                );
            }
            // The live a560 poll does not forward tweens merely because the VM
            // policy has fast_skip. Only the existing visual/input path may do
            // so; a paused pump and a disabled visual callback also preserve it.
            let mut paused = active;
            paused.paused = true;
            assert_eq!(
                pump(&mut runner, native, paused, 32, &mut voice),
                Status::Waiting
            );
            runner.advance(Ticks::from_f32(30.), paused, &mut voice, None);
            let mut disabled = active;
            disabled.animations_enabled = false;
            runner.advance(Ticks::from_f32(30.), disabled, &mut voice, None);
            assert_eq!(
                runner.frame.scene.layers().screen_properties().values()[31],
                40.
            );
            for _ in 0..9 {
                runner.advance(Ticks::from_f32(3.), active, &mut voice, None);
                assert_eq!(
                    pump(&mut runner, native, active, 32, &mut voice),
                    Status::Waiting
                );
            }
            runner.advance(Ticks::from_f32(2.), active, &mut voice, None);
            assert_eq!(
                pump(&mut runner, native, active, 32, &mut voice),
                Status::Waiting
            );
            assert_eq!(runner.completed_commands(), 8);
            runner.advance(Ticks::from_f32(1.), active, &mut voice, None);
            assert_eq!(
                pump(&mut runner, native, active, 1, &mut voice),
                Status::CommandBudget
            );
            assert_eq!(runner.completed_commands(), 9);
            assert!(runner.pending_command().is_none());
            assert_eq!(runner.random_state(), seed);
            assert_eq!(
                runner.frame.scene.layers().screen_properties().values()[31],
                0.
            );
            assert_eq!(
                runner.frame.scene.layers().message_properties().values()[31],
                0.
            );
            assert_eq!(
                pump(&mut runner, native, active, 1, &mut voice),
                Status::CommandBudget
            );
            assert_eq!(runner.completed_commands(), 10);
            assert_eq!(runner.position().0, 0x1750b39);
            assert!(runner.pending_command().is_none());
        }
    }
    eprintln!(
        "LAYERWAIT:4 actual authored Bonus helper VM/read-prefix journeys; original CALL arguments,8 controls,29/30tick wait,paused/disabled/fast_skip gates,retained PC/PRNG,one completion and original MSGSIGNAL continuation; no full chapter/hardware claim"
    );
}
