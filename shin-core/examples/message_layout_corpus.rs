//! Extract every authored MSGSET without executing story or device commands.
use std::{fs::File, io::{self, Read, Write}, path::Path};

use anyhow::Context;
use shin_core::{
    format::{rom::RomReader, scenario::{Scenario, info::CharsSpriteSegment, instructions::Instruction}},
    primitives::stateless_reader::StatelessFile,
    vm::command::CompiletimeCommand,
};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().context("Expected SNR or ROM path")?;
    let bytes = if Path::new(&path).extension().is_some_and(|extension| extension == "rom") {
        let mut rom = RomReader::new(StatelessFile::new(File::open(path)?)?)?;
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file("/main.snr")?)?.read_to_end(&mut bytes)?;
        bytes
    } else {
        std::fs::read(path)?
    };
    let scenario = Scenario::new(bytes.into())?;
    if args.next().as_deref() == Some("--ui") {
        let mut output = io::BufWriter::new(io::stdout().lock());
        let mut count = 0;
        let mut emit = |name: String, text: &str| -> anyhow::Result<()> {
            write!(output, "{name} ")?;
            for byte in text.as_bytes() {
                write!(output, "{byte:02x}")?;
            }
            writeln!(output)?;
            count += 1;
            Ok(())
        };
        let tables = scenario.info_tables();
        for (index, bgm) in tables.bgm_info.iter().enumerate() {
            emit(format!("bgm.{index}"), bgm.display_name.as_str())?;
        }
        for (index, picture) in tables.picture_box_info.iter().enumerate() {
            emit(format!("picture.{index}"), picture.name.as_str())?;
        }
        for (index, character) in tables.chars_sprite_info.iter().enumerate() {
            for (state, segment) in character.segments.iter().enumerate() {
                if let CharsSpriteSegment::Texts { name, description } = segment {
                    emit(format!("character.{index}.{state}.name"), name.as_str())?;
                    emit(
                        format!("character.{index}.{state}.body"),
                        description.as_str(),
                    )?;
                }
            }
        }
        for (index, tip) in tables.tips_info.iter().enumerate() {
            emit(format!("tip.{index}.name"), tip.title.as_str())?;
            emit(format!("tip.{index}.body"), tip.content.as_str())?;
        }
        eprintln!("Authored UI strings: {count}");
        return Ok(());
    }
    let end = scenario
        .raw()
        .iter()
        .rposition(|v| *v != 0)
        .context("Empty scenario")?
        + 1;
    let mut reader = scenario.instruction_reader(scenario.entrypoint_address());
    let mut output = io::BufWriter::new(io::stdout().lock());
    let mut count = 0;
    while (reader.position().0 as usize) < end {
        let address = reader.position();
        let instruction = reader
            .read()
            .with_context(|| format!("Instruction {address:?}"))?;
        if let Instruction::Command(CompiletimeCommand::MSGSET(message)) = instruction {
            write!(output, "{:x} ", address.0)?;
            for byte in message.text.0.as_bytes() {
                write!(output, "{byte:02x}")?;
            }
            writeln!(output)?;
            count += 1;
        }
    }
    eprintln!("Authored MSGSET instructions: {count}");
    Ok(())
}
