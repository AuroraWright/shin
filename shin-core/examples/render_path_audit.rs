//! Audit encoded render selectors across the complete supplied code section.
//! Counts are static occurrences, not proof that every branch executes.
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Cursor, Read},
};

use binrw::{BinRead, BinWrite};
use shin_core::{
    format::{
        rom::RomReader,
        scenario::{
            Scenario, instruction_elements::UntypedNumberSpec as N, instructions::Instruction,
        },
    },
    primitives::stateless_reader::StatelessFile,
    vm::command::CompiletimeCommand as C,
};

fn arguments<T: for<'a> BinWrite<Args<'a> = ()>>(value: &T) -> anyhow::Result<[N; 8]> {
    // Inspect original number specs rather than lowering registers to arbitrary
    // values. The public command tuple fields are intentionally opaque.
    let mut bytes = Cursor::new(Vec::new());
    value.write_le(&mut bytes)?;
    bytes.set_position(0);
    let mask = u8::read_le(&mut bytes)?;
    let mut result = [N::Constant(0); 8];
    for (i, spec) in result.iter_mut().enumerate() {
        if mask & (1 << i) != 0 {
            *spec = N::read_le(&mut bytes)?;
        }
    }
    anyhow::ensure!(
        bytes.position() == bytes.get_ref().len() as u64,
        "Parameter boundary"
    );
    Ok(result)
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("Expected ROM path");
    let mut rom = RomReader::new(StatelessFile::new(File::open(path)?)?)?;
    let mut bytes = Vec::new();
    rom.open_file(rom.find_file("/main.snr")?)?
        .read_to_end(&mut bytes)?;
    if let Some(dump) = std::env::args().nth(2) {
        std::fs::write(dump, &bytes)?;
    }
    let scenario = Scenario::new(bytes.into())?;
    let code_end = scenario.raw().iter().rposition(|&b| b != 0).unwrap() + 1;
    let padding = scenario.raw().len() - code_end;
    anyhow::ensure!(
        padding <= 16 && scenario.raw()[code_end - 1] == 0x49,
        "Unreviewed scenario tail"
    );
    let mut reader = scenario.instruction_reader(scenario.entrypoint_address());
    let mut instructions = 0u64;
    let mut loads = BTreeMap::<i32, u64>::new();
    let mut properties = BTreeMap::<i32, u64>::new();
    let mut controls = BTreeMap::<i32, u64>::new();
    let mut load_flags = BTreeMap::<i32, u64>::new();
    let mut dynamic_load_flags = 0u64;
    let mut dynamic_control_flags = 0u64;
    let mut selectors = BTreeMap::<i32, u64>::new();
    let (mut dynamic_load, mut dynamic_property, mut dynamic_shader) = (0u64, 0u64, 0u64);
    while (reader.position().0 as usize) < code_end {
        let pc = reader.position().0;
        let instruction = reader
            .read()
            .map_err(|e| anyhow::anyhow!("Decode at{pc:x}: {e:#}"))?;
        instructions += 1;
        match instruction {
            Instruction::Command(C::LAYERLOAD(load)) => {
                match load.flags.into_untyped() {
                    N::Constant(flags) => *load_flags.entry(flags).or_default() += 1,
                    N::Register(_) => {
                        dynamic_load_flags += 1;
                        println!("DLOAD_FLAGS {pc:x} {load:?}");
                    }
                }
                match load.layer_type.into_untyped() {
                    N::Constant(kind) => {
                        *loads.entry(kind).or_default() += 1;
                        if matches!(kind, 1 | 4 | 5 | 7 | 8 | 9) {
                            println!(
                                "L {pc:x} {:?} {kind} {:?}",
                                load.layer_id.into_untyped(),
                                arguments(&load.params)?
                            );
                        }
                    }
                    N::Register(_) => {
                        dynamic_load += 1;
                        println!("DLOAD {pc:x} {load:?}");
                    }
                }
            }
            Instruction::Command(C::LAYERCTRL(ctrl)) => match ctrl.property_id.into_untyped() {
                N::Constant(property) => {
                    *properties.entry(property).or_default() += 1;
                    let args = arguments(&ctrl.params)?;
                    match args[2] {
                        N::Constant(flags) => {
                            *controls.entry(flags).or_default() += 1;
                            if flags & 0x1000 != 0 {
                                println!(
                                    "PROTECTED {pc:x} {:?} {property} {args:?}",
                                    ctrl.layer_id.into_untyped()
                                );
                            }
                        }
                        N::Register(_) => {
                            dynamic_control_flags += 1;
                            println!("DFLAGS {pc:x} {ctrl:?}");
                        }
                    }
                    if matches!(property, 24 | 61) {
                        println!(
                            "C {pc:x} {:?} {property} {args:?}",
                            ctrl.layer_id.into_untyped()
                        );
                        if property == 24 {
                            match args[0] {
                                N::Constant(shader) => *selectors.entry(shader).or_default() += 1,
                                N::Register(_) => dynamic_shader += 1,
                            }
                        }
                    }
                }
                N::Register(_) => {
                    dynamic_property += 1;
                    println!("DPROP {pc:x} {ctrl:?}");
                }
            },
            _ => {}
        }
    }
    anyhow::ensure!(
        reader.position().0 as usize == code_end,
        "Instruction crosses code end"
    );
    println!(
        "S {} {:x} {:x} {instructions} {padding}",
        scenario.raw().len(),
        scenario.entrypoint_address().0,
        reader.position().0
    );
    println!("LOADS {loads:?}");
    println!("PROPERTIES {properties:?}");
    println!("SHADERS {selectors:?}");
    println!("CONTROL_FLAGS {controls:?}");
    println!("LOAD_FLAGS {load_flags:?}");
    println!("DYNAMIC_LOAD_FLAGS {dynamic_load_flags}");
    println!("DYNAMIC_CONTROL_FLAGS {dynamic_control_flags}");
    println!("DYNAMIC {dynamic_load} {dynamic_property} {dynamic_shader}");
    Ok(())
}
