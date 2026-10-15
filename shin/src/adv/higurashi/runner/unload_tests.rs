//! Real VM pumping around the original-compared captured unload owner.
use super::{
    tests::{Voice, policy},
    *,
};
use binrw::BinWrite;
use shin_core::format::scenario::instruction_elements::UntypedNumberSpec;

fn number(code: &mut Vec<u8>, value: i32) {
    let mut cursor = std::io::Cursor::new(Vec::new());
    UntypedNumberSpec::Constant(value)
        .write_le(&mut cursor)
        .unwrap();
    code.extend(cursor.into_inner());
}
struct Read;
impl ReadServices for Read {
    fn checkpoint(
        &mut self,
        _: super::super::read_state::Checkpoint,
        _: super::super::input::ReadState,
    ) -> Result<()> {
        panic!("unload/WAIT has no checkpoint prefix");
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        panic!("no message completion");
    }
}

#[test]
#[ignore = "actual updated VM unload/WAIT pump with supplied message resources on offscreen Metal; no OS audio/window/save"]
fn higurashi_runner_unload_completes_without_waiting_and_gates_retiring_updates() {
    let (_, _, assets) = super::super::message_glyphs::tests::gpu::gpu_assets();
    let assets = Arc::new(assets);
    let authored = shin_tasks::block_on(assets.load::<HigurashiScenario, _>("/main.snr")).unwrap();
    for native in [false, true] {
        for bank in 0..2 {
            let mut raw = authored.raw().to_vec();
            let entry = authored.header.code_offset as usize;
            let mut code = vec![0xc2];
            number(&mut code, 1);
            number(&mut code, 2);
            code.extend([0x83, 1]);
            number(&mut code, 64);
            code.extend([0, 0, 0]);
            raw[entry..entry + code.len()].copy_from_slice(&code);
            let scenario = Arc::new(HigurashiScenario::new(raw.into()).unwrap());
            let mut runner = shin_tasks::block_on(HigurashiRunner::load(
                assets.clone(),
                scenario.clone(),
                0,
                42,
                || [1; 4],
            ))
            .unwrap();
            let tile = |id| Cmd::LAYERLOAD {
                args: [id, 1, 0],
                params: (65535, -640, -360, 1280, 720, 0, 0, 0),
            };
            shin_tasks::block_on(runner.frame.scene.layers_mut().unwrap().dispatch(
                &tile(1),
                &assets,
                &scenario,
                || [1; 4],
            ))
            .unwrap();
            if bank == 1 {
                shin_tasks::block_on(runner.frame.scene.layers_mut().unwrap().dispatch(
                    &Cmd::LAYERBACK { argument: 1 },
                    &assets,
                    &scenario,
                    || [1; 4],
                ))
                .unwrap();
            }
            let target = runner.frame.scene.layers().state.target(1).unwrap();
            assert_eq!(usize::from(target.bank), bank);
            runner
                .frame
                .scene
                .layers_mut()
                .unwrap()
                .layer_mut(target)
                .unwrap()
                .properties
                .control(0, 64., Ticks::from_f32(2.), 0)
                .unwrap();
            let mut voice = Voice::default();
            let status = if native {
                shin_tasks::block_on(runner.pump_native(
                    policy(),
                    ReadPolicy {
                        modes: 0,
                        forced: false,
                        read_only: false,
                    },
                    &mut Read,
                    2,
                    100,
                    || panic!("unload consumes no constructor entropy"),
                    &mut voice,
                ))
                .unwrap()
            } else {
                shin_tasks::block_on(runner.pump(
                    policy(),
                    2,
                    100,
                    || panic!("unload consumes no constructor entropy"),
                    &mut voice,
                ))
                .unwrap()
            };
            assert_eq!(status, Status::Waiting);
            assert_eq!(runner.completed_commands(), 1);
            assert!(
                matches!(runner.pending.as_ref().unwrap().owner, Owner::Clock(_)),
                "VM continues to WAIT instead of waiting for layer retirement"
            );
            assert!(runner.frame.scene.layers().layer(target).is_none());
            let layers = runner.frame.scene.layers_mut().unwrap();
            let retained = layers.reference_group_entries(bank);
            assert_eq!(
                (retained.len(), retained[0].0, retained[0].1),
                (1, -1, 2f32.to_bits())
            );
            shin_tasks::block_on(layers.dispatch(&tile(77), &assets, &scenario, || [1; 4]))
                .unwrap();
            let replacement = layers.state.target(77).unwrap();
            assert_eq!(
                replacement.slot == target.slot,
                bank == 0,
                "free physical slot can be reused while the old owner remains retained"
            );
            // Original19578 restores existing groups in place. Until that
            // preservation is integrated, refuse an empty-retiree approximation
            // before consuming fresh seeds or changing the source owners.
            let snapshot = layers.state.clone();
            let prepared = shin_tasks::block_on(
                super::super::layers::HigurashiLayers::prepare_reconstruction(
                    &snapshot, &assets, &scenario, 0,
                ),
            )
            .unwrap();
            let result = layers.construct_reconstruction(
                prepared,
                || panic!("guard must precede entropy"),
                None,
            );
            assert!(
                result
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("in-place restoration of live retiring layers")
            );
            assert_eq!(layers.reference_group_entries(bank)[0].1, 2f32.to_bits());
            let mut paused = policy();
            paused.paused = true;
            runner.advance(Ticks::from_f32(128.), paused, &mut voice, None);
            let mut disabled = policy();
            disabled.animations_enabled = false;
            runner.advance(Ticks::from_f32(128.), disabled, &mut voice, None);
            assert_eq!(
                runner.frame.scene.layers().reference_group_entries(bank)[0].1,
                2f32.to_bits()
            );
            runner.advance(Ticks::from_f32(1.), policy(), &mut voice, None);
            let rows = runner.frame.scene.layers().reference_group_entries(bank);
            assert_eq!(rows[0].1, 1f32.to_bits());
            assert_eq!(rows[0].2.properties.values()[0], 32.);
            runner.advance(Ticks::from_f32(1.), policy(), &mut voice, None);
            assert!(
                runner
                    .frame
                    .scene
                    .layers()
                    .reference_group_entries(bank)
                    .iter()
                    .all(|row| row.0 >= 0)
            );
            assert!(runner.frame.scene.layers().layer(replacement).is_some());
        }
    }
    eprintln!(
        "Runner unload:4 actual VM/read-prefix front/back pipelines;immediate completion/WAIT continuation,retained updates,paused/disabled gates,physical slot reuse,and pre-entropy checked in-place restore boundary;offscreen Metal,no OS audio/window/user saves"
    );
}
