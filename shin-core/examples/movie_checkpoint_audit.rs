//! Inspect all decoded authored Movie loads and their neighboring instructions.
//! This is a static audit; it does not establish branch reachability or replay.
use std::{fs::File, io::Read};

use shin_core::{
    format::{
        rom::RomReader,
        scenario::{
            Scenario,
            instruction_elements::UntypedNumberSpec,
            instructions::Instruction,
        },
    },
    primitives::stateless_reader::StatelessFile,
    vm::command::CompiletimeCommand as C,
};

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("Expected ROM path");
    let mut rom = RomReader::new(StatelessFile::new(File::open(path)?)?)?;
    let file = rom.find_file("/main.snr").expect("Missing main.snr");
    let mut bytes = Vec::new();
    rom.open_file(file)?.read_to_end(&mut bytes)?;
    let scenario = Scenario::new(bytes.into())?;
    // Supplied files end with a return opcode followed by alignment zeros.
    // Do not interpret those zeros as incomplete EXIT instructions.
    let code_end = scenario.raw().iter().rposition(|&b| b != 0).unwrap() + 1;
    let padding = scenario.raw().len() - code_end;
    anyhow::ensure!(padding <= 16 && scenario.raw()[code_end - 1] == 0x49,
                    "Unreviewed scenario tail");
    let mut reader = scenario.instruction_reader(scenario.entrypoint_address());
    let mut instructions = Vec::new();
    let mut movies = Vec::new();
    let mut dynamic = Vec::new();
    while (reader.position().0 as usize) < code_end {
        let address = reader.position().0;
        let instruction = reader.read().map_err(|error| {
            anyhow::anyhow!("Linear decode at{address:x}: {error:#}")
        })?;
        if let Instruction::Command(C::LAYERLOAD(load)) = &instruction {
            match load.layer_type.into_untyped() {
                UntypedNumberSpec::Constant(6) => movies.push(instructions.len()),
                UntypedNumberSpec::Register(_) => dynamic.push(instructions.len()),
                _ => {}
            }
        }
        instructions.push((address, instruction));
    }
    anyhow::ensure!(reader.position().0 as usize == code_end, "Instruction crosses padding boundary");
    println!(
        "S {} {:x} {:x} {} {} {} {}",
        scenario.raw().len(), scenario.entrypoint_address().0,
        reader.position().0, instructions.len(), movies.len(), dynamic.len(), padding
    );
    for (kind, indices) in [("MOVIE", movies), ("DYNAMIC", dynamic)] {
        for index in indices {
            println!("B {kind} {:x}", instructions[index].0);
            for (address, instruction) in &instructions[
                index.saturating_sub(12)..(index + 65).min(instructions.len())
            ] {
                println!("I {address:x} {instruction:?}");
            }
            println!("E {kind}");
        }
    }
    Ok(())
}
