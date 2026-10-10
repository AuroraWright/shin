use super::*;

fn compare_failure(bytes: Bytes, position: u32) {
    let mut fast = InstructionReader::new(bytes.clone(), CodeAddress(position));
    let mut legacy = Cursor::new(bytes);
    legacy.set_position(u64::from(position));
    let actual = fast.read().unwrap_err();
    let expected = Instruction::read(&mut legacy).unwrap_err();
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    assert_eq!(u64::from(fast.position().0), legacy.position());
}

#[test]
fn opcode_dispatch_retains_unknown_and_truncated_input_errors() {
    for opcode in 0..=u8::MAX {
        // All commands except zero-argument ones are truncated here; the
        // unsupported opcodes must still fail through the original decoder.
        let bytes = Bytes::from(vec![opcode]);
        let mut legacy = Cursor::new(bytes.clone());
        if Instruction::read(&mut legacy).is_err() {
            compare_failure(bytes, 0);
        }
    }
    compare_failure(Bytes::new(), 0);
    compare_failure(Bytes::from_static(&[0x86, 0x80]), 0);
    compare_failure(Bytes::from_static(&[0, 0xc0]), 1);
}

#[test]
#[ignore = "requires original and1.03 supplied ROMs (SHIN_REFERENCE_ROM)"]
fn reference_complete_original_and_updated_instruction_decoder_parity() {
    use crate::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    use std::{collections::BTreeSet, io::Read, path::Path};
    let base = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    for path in [
        Path::new(&base).to_owned(),
        Path::new(&base).parent().unwrap().join("patch.rom"),
    ] {
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(&path).unwrap()).unwrap())
                .unwrap();
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/main.snr").unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let scenario = Scenario::new(bytes.into()).unwrap();
        let mut fast = scenario.instruction_reader(scenario.entrypoint_address());
        let mut legacy = Cursor::new(scenario.raw());
        legacy.set_position(u64::from(scenario.entrypoint_address().0));
        let mut instructions = 0;
        let mut opcodes = BTreeSet::new();
        let mut truncations = 0;
        let code_end = scenario.raw().iter().rposition(|byte| *byte != 0).unwrap() + 1;
        while (fast.position().0 as usize) < code_end {
            let start = fast.position();
            let opcode = scenario.raw()[start.0 as usize];
            let expected = Instruction::read(&mut legacy).unwrap();
            let actual = fast.read().unwrap();
            assert_eq!(actual, expected, "{} instruction{start:?}", path.display());
            assert_eq!(u64::from(fast.position().0), legacy.position());
            instructions += 1;
            if opcodes.insert(opcode) {
                // Keep absolute offsets because Pad4 depends on cursor position.
                let end = fast.position().0 as usize;
                for cut in start.0 as usize..end {
                    let bytes = Bytes::copy_from_slice(&scenario.raw()[..cut]);
                    let mut old = Cursor::new(bytes.clone());
                    old.set_position(u64::from(start.0));
                    let mut new = InstructionReader::new(bytes, start);
                    // SELECT's existing StringArray parser asserts on a length
                    // mismatch. Preserve that behavior without changing the
                    // field parser as part of this dispatch optimization.
                    let expected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        Instruction::read(&mut old)
                    }));
                    let actual =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| new.read()));
                    match (expected, actual) {
                        (Ok(Ok(expected)), Ok(Ok(actual))) => assert_eq!(actual, expected),
                        (Ok(Err(expected)), Ok(Err(actual))) => {
                            assert_eq!(format!("{actual:?}"), format!("{expected:?}"))
                        }
                        (Err(_), Err(_)) => {}
                        _ => panic!("different decode outcome at {start:?}, cut {cut}"),
                    }
                    assert_eq!(u64::from(new.position().0), old.position());
                    truncations += 1;
                }
            }
        }
        eprintln!(
            "{}: {instructions} complete instruction/position comparisons, {} opcodes and {truncations} truncated instruction inputs match legacy",
            path.display(),
            opcodes.len()
        );
    }
}
