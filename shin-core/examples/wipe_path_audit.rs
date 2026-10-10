//! Inventory wipe selectors/copy triggers over the complete supplied code section.
//! Encoded occurrences do not establish live reachability or active-wipe timing.
use binrw::{BinRead, BinWrite};
use shin_core::{
    format::scenario::{
        Scenario, instruction_elements::UntypedNumberSpec as N, instructions::Instruction,
    },
    vm::command::CompiletimeCommand as C,
};
use std::{collections::BTreeMap, io::Cursor};
fn arguments<T: for<'a> BinWrite<Args<'a> = ()>>(value: &T) -> anyhow::Result<[N; 8]> {
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
    let path = std::env::args()
        .nth(1)
        .expect("Expected extracted main.snr");
    let scenario = Scenario::new(std::fs::read(path)?.into())?;
    let code_end = scenario.raw().iter().rposition(|&b| b != 0).unwrap() + 1;
    let padding = scenario.raw().len() - code_end;
    anyhow::ensure!(
        padding <= 16 && scenario.raw()[code_end - 1] == 0x49,
        "Unreviewed scenario tail"
    );
    let mut reader = scenario.instruction_reader(scenario.entrypoint_address());
    let mut instructions = 0u64;
    let mut counts = BTreeMap::<String, u64>::new();
    let mut types = BTreeMap::<String, u64>::new();
    let mut dynamic = BTreeMap::<String, u64>::new();
    while (reader.position().0 as usize) < code_end {
        let pc = reader.position().0;
        let instruction = reader
            .read()
            .map_err(|e| anyhow::anyhow!("Decode at{pc:x}: {e:#}"))?;
        instructions += 1;
        let event = match instruction {
            Instruction::Command(C::WIPE(w)) => Some((
                "WIPE",
                w.ty.into_untyped(),
                w.flags.into_untyped(),
                w.duration.into_untyped(),
                arguments(&w.params)?,
            )),
            Instruction::Command(C::TRANSSET(w)) => Some((
                "TRANSSET",
                w.ty.into_untyped(),
                w.flags.into_untyped(),
                w.duration.into_untyped(),
                arguments(&w.params)?,
            )),
            Instruction::Command(C::PAGEBACK(_)) => {
                *counts.entry("PAGEBACK".into()).or_default() += 1;
                println!("P {pc:x}");
                None
            }
            Instruction::Command(C::WIPEWAIT(_)) => {
                *counts.entry("WIPEWAIT".into()).or_default() += 1;
                println!("WAIT {pc:x}");
                None
            }
            Instruction::Command(C::TRANSWAIT(w)) => {
                *counts.entry("TRANSWAIT".into()).or_default() += 1;
                println!("TWAIT {pc:x} {:?}", w.layer_id.into_untyped());
                None
            }
            _ => None,
        };
        if let Some((name, ty, flags, duration, params)) = event {
            *counts.entry(name.into()).or_default() += 1;
            match ty {
                N::Constant(kind) => *types.entry(format!("{name}:{kind}")).or_default() += 1,
                N::Register(_) => *dynamic.entry(name.into()).or_default() += 1,
            };
            println!("W {pc:x} {name} {ty:?} {flags:?} {duration:?} {params:?}");
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
    println!("COUNTS {counts:?}");
    println!("TYPES {types:?}");
    println!("DYNAMIC {dynamic:?}");
    Ok(())
}
