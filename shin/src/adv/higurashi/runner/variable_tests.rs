use super::super::variables::{Execution, HigurashiVariables};
use super::*;
use shin_core::{format::save::PersistData, format::scenario::instruction_elements::Register};

#[test]
#[ignore = "retained variable feedback and authored startup on offscreen Metal"]
fn higurashi_runner_resolves_variable_commands_through_concrete_memory_owner() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    let mut startup = shin_tasks::block_on(HigurashiRunner::load(
        assets.clone(),
        authored,
        0,
        42,
        || [1, 2, 3, 4],
    ))
    .unwrap();
    let mut variables = HigurashiVariables::new(PersistData::new(), Vec::new());
    let mut voice = super::tests::Voice::default();
    assert_eq!(
        shin_tasks::block_on(startup.pump(
            super::tests::policy(),
            2,
            10000,
            || [1, 2, 3, 4],
            &mut voice
        ))
        .unwrap(),
        Status::External
    );
    assert!(matches!(
        startup.pending_command(),
        Some(Cmd::SGET { slot: 2, .. })
    ));
    assert_eq!(startup.position(), CodeAddress(0x1f78f));
    assert!(
        startup
            .complete_variable_command(&mut variables, Execution::Live)
            .unwrap()
    );
    assert_eq!(variables.route().1, 2);
    assert!(
        startup
            .complete_variable_command(&mut variables, Execution::Live)
            .is_err()
    );

    let code = [
        0x82, 2, 0xb0, 0x81, 31, 0, 2, 0xdb, 3, 0xb0, 0xda, 32, 0, 3, 0x83, 1, 0x83, 0xe8, 0x81,
        33, 0, 2, 0xda, 34, 0, 3, 0x83, 0, 0, 0, 0, 0,
    ];
    let mut raw = startup.scenario.raw().to_vec();
    let start = startup.scenario.header.code_offset as usize;
    raw[start..start + code.len()].copy_from_slice(&code);
    let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
    let mut runner =
        shin_tasks::block_on(HigurashiRunner::load(assets, scenario, 65535, 42, || {
            [1, 2, 3, 4]
        }))
        .unwrap();
    let mut variables = HigurashiVariables::new(PersistData::new(), Vec::new());
    for _ in 0..4 {
        assert_eq!(
            shin_tasks::block_on(runner.pump(
                super::tests::policy(),
                1,
                10000,
                || [1, 2, 3, 4],
                &mut voice
            ))
            .unwrap(),
            Status::External
        );
        let before = (
            runner.position(),
            runner.next_position(),
            runner.random_state(),
        );
        assert!(
            runner
                .complete_variable_command(&mut variables, Execution::Live)
                .unwrap()
        );
        assert_eq!(
            before,
            (
                runner.position(),
                runner.next_position(),
                runner.random_state()
            )
        );
    }
    assert_eq!(
        shin_tasks::block_on(runner.pump(
            super::tests::policy(),
            1,
            10000,
            || [1, 2, 3, 4],
            &mut voice
        ))
        .unwrap(),
        Status::Waiting
    );
    assert_eq!(
        runner.vm.as_ref().unwrap().read_register(Register::from_regular_register(31)),
        65535
    );
    assert_eq!(
        runner.vm.as_ref().unwrap().read_register(Register::from_regular_register(32)),
        65535
    );
    assert!(
        runner
            .complete_variable_command(&mut variables, Execution::Live)
            .is_err()
    );
    variables.commit_pending(true).unwrap();
    assert!(runner.interrupt_wait());
    assert_eq!(
        shin_tasks::block_on(runner.pump(
            super::tests::policy(),
            1,
            10000,
            || [1, 2, 3, 4],
            &mut voice
        ))
        .unwrap(),
        Status::CommandBudget
    );
    for _ in 0..2 {
        assert_eq!(
            shin_tasks::block_on(runner.pump(
                super::tests::policy(),
                1,
                10000,
                || [1, 2, 3, 4],
                &mut voice
            ))
            .unwrap(),
            Status::External
        );
        assert!(
            runner
                .complete_variable_command(&mut variables, Execution::Live)
                .unwrap()
        );
    }
    assert_eq!(
        shin_tasks::block_on(runner.pump(
            super::tests::policy(),
            8,
            10000,
            || [1, 2, 3, 4],
            &mut voice
        ))
        .unwrap(),
        Status::Exited
    );
    assert_eq!(runner.completed_commands(), 9);
    assert_eq!(
        runner.vm.as_ref().unwrap().read_register(Register::from_regular_register(33)),
        -1
    );
    assert_eq!(
        runner.vm.as_ref().unwrap().read_register(Register::from_regular_register(34)),
        255
    );
    println!(
        "Higurashi variable runner:9 real commands plus authored startup SGET at1f78f resolved by concrete memory storage; full32-to-signed16/byte commit feedback, WAIT ownership and single completion pass"
    );
}
