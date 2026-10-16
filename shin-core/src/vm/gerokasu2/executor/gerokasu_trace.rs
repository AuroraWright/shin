//! CPU trace of the first Gerokasu commands (diagnostics; ignored test).
use std::io::Read;

use super::*;
use crate::format::rom::RomReader;

fn supplied_gerokasu() -> Arc<Gerokasu2Scenario> {
    use shin_primitives::stateless_reader::StatelessFile;
    let path = std::env::var_os("SHIN_GEROKASU_ROM").expect("SHIN_GEROKASU_ROM");
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap()).unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    Arc::new(Gerokasu2Scenario::new(bytes.into()).unwrap())
}

#[test]
#[ignore = "requires supplied Gerokasu data.rom (SHIN_GEROKASU_ROM); prints a command trace"]
fn gerokasu_trace_first_commands() {
    let scenario = supplied_gerokasu();
    let count: usize = std::env::var("SHIN_GEROKASU_TRACE_COUNT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(200);
    let mut vm = Gerokasu2Scripter::new(scenario, 0, 42);
    let mut result = CommandResult::None;
    for index in 0..count {
        let command = match vm.run(result, 10_000_000) {
            Ok(command) => command,
            Err(error) => {
                println!("T fault after {index}: {error:#}");
                break;
            }
        };
        println!("T {:06x} {command:?}", vm.position().0);
        result = match vm.pending_destination() {
            Some(register) => CommandResult::WriteMemory(register, 0),
            None => CommandResult::None,
        };
        if vm.is_halted() {
            println!("T halted");
            break;
        }
    }
}
