use super::*;
use bytes::Bytes;

fn reader(data: &[u8], variant: HigurashiVariant) -> HigurashiInstructionReader {
    HigurashiInstructionReader::new(Bytes::copy_from_slice(data), variant, 0, CodeAddress(0))
        .unwrap()
}

#[test]
fn updated_message_operands_and_choice_lengths_preserve_alignment() {
    let mut r = reader(
        &[
            0x85, 1, 2, 3, 0x8b, 0x86, 5, 0, 0, 1, 7, 3, 0, b'A', b'B', 0, 0x88,
        ],
        HigurashiVariant::Updated,
    );
    assert!(matches!(
        r.read().unwrap(),
        HigurashiInstruction::Command(HigurashiCommand::MSGINIT {
            updated_options: Some(_),
            ..
        })
    ));
    assert_eq!(r.position().0, 4);
    r.read().unwrap();
    assert_eq!(r.position().0, 5);
    let HigurashiInstruction::Command(HigurashiCommand::MSGSET {
        msg_id,
        argument,
        text,
        ..
    }) = r.read().unwrap()
    else {
        panic!()
    };
    assert_eq!(msg_id.0, 5);
    assert_eq!(argument, NumberSpec::constant(7));
    assert_eq!(text.0, "AB");
    assert_eq!(r.position().0, 16);
    r.read().unwrap();
    assert_eq!(r.position().0, 17);
    let mut original = reader(&[0x85, 1, 0x8b, 2, 0x88], HigurashiVariant::Original);
    original.read().unwrap();
    assert_eq!(original.position().0, 2);
    original.read().unwrap();
    assert_eq!(original.position().0, 4);
    let mut original = reader(&[0x8b, 0x83, 0xe8, 0x88], HigurashiVariant::Original);
    let HigurashiInstruction::Command(HigurashiCommand::MSGFACE { original_argument }) =
        original.read().unwrap()
    else {
        panic!()
    };
    assert_eq!(original_argument, Some(NumberSpec::constant(1000)));
    assert_eq!(original.position().0, 3);
    let mut choices = reader(
        &[
            0x8d, 1, 0, 2, 0, 0, 0, 3, 2, b'T', 0, 5, b'A', 0, b'B', 0, 0, 0x88,
        ],
        HigurashiVariant::Updated,
    );
    let HigurashiInstruction::Command(HigurashiCommand::SELECT { variants, .. }) =
        choices.read().unwrap()
    else {
        panic!()
    };
    assert_eq!(variants.0, ["A", "B"]);
    assert_eq!(choices.position().0, 17);
}

#[test]
fn jump_tables_use_one_byte_counts_and_expression_high_bit_terminators() {
    let mut r = reader(
        &[0x4a, 0, 1, 8, 0, 0, 0, 0x88, 0, 0, 0],
        HigurashiVariant::Updated,
    );
    let instruction = r.read().unwrap();
    assert_eq!(r.position().0, 7);
    assert_eq!(
        instruction.successors(r.position()),
        [CodeAddress(8), CodeAddress(7)]
    );
    let mut expression = reader(
        &[0x42, 0, 0, 0, 1, 0x20, 0x80, 0x88],
        HigurashiVariant::Updated,
    );
    expression.read().unwrap();
    assert_eq!(expression.position().0, 7);
}

#[test]
fn default_umineko_reader_keeps_its_operand_layouts() {
    let data = Bytes::from_static(&[0x85, 1, 2, 3]);
    let mut umineko = crate::format::scenario::InstructionReader::new(data, CodeAddress(0));
    assert!(matches!(umineko.read().unwrap(), Instruction::Command(_)));
    assert_eq!(umineko.position().0, 2);
    // Umineko voice names use u16 lengths; Higurashi uses u8 lengths.
    let data = Bytes::from_static(&[0x9c, 2, 0, b'A', 0, 1, 2]);
    let mut umineko = crate::format::scenario::InstructionReader::new(data.clone(), CodeAddress(0));
    umineko.read().unwrap();
    assert_eq!(umineko.position().0, 7);
    assert!(reader(&data, HigurashiVariant::Updated).read().is_err());
}

#[test]
fn malformed_operands_and_control_flow_return_errors_without_consuming() {
    for data in [
        vec![0x86, 1],
        vec![0x8c, 2, 0, b'A', b'B'],
        vec![0x9c, 0],
        vec![0x8d, 0, 0, 0, 0, 0, 0, 0, 1, 0, 3, b'A', 0, b'B'],
        vec![0x80],
    ] {
        let mut r = reader(&data, HigurashiVariant::Updated);
        assert!(r.read().is_err());
        assert_eq!(r.position().0, 0);
    }
    let mut r = reader(&[0x47, 2, 0, 0, 0], HigurashiVariant::Updated);
    assert!(
        r.reachable_spans()
            .unwrap_err()
            .to_string()
            .contains("operand")
    );
    let mut r = reader(&[0x47, 0xff, 0xff, 0xff, 0xff], HigurashiVariant::Updated);
    assert!(r.reachable_spans().is_err());
}

#[test]
#[ignore = "requires the supplied scenario via SHIN_HIGURASHI_SCENARIO"]
fn supplied_scenario_reachable_bytecode() {
    let data =
        Bytes::from(std::fs::read(std::env::var("SHIN_HIGURASHI_SCENARIO").unwrap()).unwrap());
    let scenario = super::super::HigurashiScenario::new(data).unwrap();
    let mut r = scenario
        .instruction_reader(CodeAddress(scenario.header.code_offset))
        .unwrap();
    let spans = r.reachable_spans().unwrap();
    assert!(spans.len() > 100_000);
    println!(
        "{:?}: {} instructions, {} bytes",
        scenario.variant,
        spans.len(),
        spans.iter().map(|(a, b)| b.0 - a.0).sum::<u32>()
    );
}
