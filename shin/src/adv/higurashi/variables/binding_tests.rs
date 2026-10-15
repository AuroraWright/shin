use super::*;
use shin_core::format::scenario::instruction_elements::Register;

#[test]
fn borrowed_progress_vector_returns_to_owner_after_codec_error() {
    let mut variables = HigurashiVariables::new(PersistData::new(), Vec::new());
    // Checked arithmetic error before buffer decoding/growth; no huge allocation.
    variables.position = usize::MAX;
    let mut data = vec![0xff];
    let original = data.as_ptr();
    assert!(
        variables
            .dispatch_in_progress(
                &mut data,
                &Cmd::SGET {
                    dest: Register::from_regular_register(31),
                    slot: 0
                },
                Execution::Replay {
                    boundary: 0,
                    target: 0
                }
            )
            .is_err()
    );
    assert_eq!(data, [0xff]);
    assert_eq!(data.as_ptr(), original);
    assert!(variables.route.is_empty());
    assert_eq!(variables.position, usize::MAX);
}
