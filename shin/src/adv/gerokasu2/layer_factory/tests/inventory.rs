//! Entire authored syntax and procedural artwork inventory, without a device.
use super::*;
use shin_core::{
    format::{
        scenario::{
            gerokasu2::bytecode::Instruction,
            instruction_elements::{CodeAddress, UntypedNumberSpec},
            instructions::Instruction as Shared,
        },
        text::TextEncoding,
        texture_archive::read_texture_archive_with_encoding,
    },
    vm::command::CompiletimeCommand,
};
#[test]
#[ignore = "requires supplied Ger ROM; complete layer syntax/artwork inventory, CPU only"]
fn ger_authored_drawable_kinds_and_procedural_artwork_inventory() {
    let scenario = scenario();
    let mut reader = scenario.instruction_reader(CodeAddress(scenario.header.code_offset));
    let mut kinds = std::collections::BTreeMap::new();
    let mut procedural = std::collections::BTreeMap::new();
    let mut instructions = 0usize;
    while (reader.position().0 as usize) < scenario.raw().len() {
        let tail = &scenario.raw()[reader.position().0 as usize..];
        if tail.len() < 16 && tail.iter().all(|&b| b == 0) {
            break;
        }
        let instruction = reader.read().unwrap();
        instructions += 1;
        if let Instruction::Shared(Shared::Command(CompiletimeCommand::LAYERLOAD(c))) = instruction
        {
            let kind = c.layer_type.into_untyped();
            let label = format!("{kind:?}");
            *kinds.entry(label.clone()).or_insert(0usize) += 1;
            if matches!(kind, UntypedNumberSpec::Constant(4 | 5 | 7 | 8)) {
                *procedural
                    .entry((label, format!("{:?}", c.params)))
                    .or_insert(0usize) += 1;
            }
        }
    }
    assert_eq!(instructions, 24187);
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let file = rom.find_file("/effects.txa").unwrap();
    let mut data = Vec::new();
    rom.open_file(file).unwrap().read_to_end(&mut data).unwrap();
    let archive = read_texture_archive_with_encoding(&data, TextEncoding::Utf8).unwrap();
    let mut textures: Vec<_> = archive
        .name_to_index
        .iter()
        .map(|(name, index)| {
            let texture = &archive.textures[*index];
            (name.clone(), texture.dimensions())
        })
        .collect();
    textures.sort();
    println!(
        "GEROKASU2_AUTHORED_DRAWABLE_INVENTORY instructions={instructions} kinds={kinds:?} procedural={procedural:?} artwork={textures:?}"
    );
}
