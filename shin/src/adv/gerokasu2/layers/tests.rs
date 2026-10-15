use super::*;
use std::io::{Read, Write};

#[test]
fn gerokasu2_layer_policy_preserves_selection_and_rejects_invalid_storage() {
    let mut state = Gerokasu2Layers::default();
    state.select(9, 8);
    assert_eq!(state.selected_range(), [8, 9]);
    let targets = state.load(-6, 9, 0, [0; 8]).unwrap();
    assert_eq!(
        targets.iter().map(|t| t.layer.raw()).collect::<Vec<_>>(),
        [8, 9]
    );
    state.control(-6, 0, i32::MAX, 0).unwrap();
    state.control(-6, 0, 1, 0x80).unwrap();
    for target in targets {
        assert_eq!(
            state
                .record(target.layerbank)
                .property(LayerProperty::TranslateX),
            i32::MIN
        );
        assert_eq!(state.record(target.layerbank).kind(), 9);
    }
    let old = state.reference_bytes();
    assert!(state.initialize(-5).is_err());
    assert!(state.load(256, 1, 0, [0; 8]).is_err());
    assert!(state.control(-1, 90, 1, 0).is_err());
    assert!(state.select_plane(4).is_err());
    assert_eq!(old, state.reference_bytes());
    // Signed sorting precedes narrowing, rather than clamping into storage.
    state.select(-1, 0x10001);
    assert_eq!(state.selected_range(), [u16::MAX, 1]);
    assert!(state.load(-6, 1, 0, [0; 8]).is_err());
    state.select(8, 9);
    state.set_mask(7, 123);
    state.clear_plane();
    assert_eq!(state.selected_range(), [8, 9]);
    assert_eq!(state.planes[0].mask, [-1, 123]);
    assert!(state.target(LayerId::new(8)).is_none());
}

#[derive(Clone, Copy)]
struct Request {
    operation: u32,
    arguments: [i32; 11],
}
fn request(operation: u32, words: &[i32]) -> Request {
    let mut arguments = [0; 11];
    arguments[..words.len()].copy_from_slice(words);
    Request {
        operation,
        arguments,
    }
}
struct NativeLayers {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::process::ChildStdout,
    operations: usize,
}
impl NativeLayers {
    fn new() -> Self {
        use std::process::{Command, Stdio};
        let mut child = Command::new(
            std::env::var_os("SHIN_GEROKASU2_LAYER_ORACLE").expect("SHIN_GEROKASU2_LAYER_ORACLE"),
        )
        .arg(std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").expect("SHIN_GEROKASU2_EXECUTABLE"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
            operations: 0,
        }
    }
    fn verify(&mut self, r: Request, state: &Gerokasu2Layers) {
        let input = self.input.as_mut().unwrap();
        input.write_all(&r.operation.to_le_bytes()).unwrap();
        for value in r.arguments {
            input.write_all(&value.to_le_bytes()).unwrap();
        }
        let mut header = [0; 4];
        self.output.read_exact(&mut header).unwrap();
        assert_eq!(&header, b"GLYS");
        let mut native = vec![0; 0x609c];
        self.output.read_exact(&mut native).unwrap();
        let rust = state.reference_bytes();
        if let Some(offset) = native.iter().zip(&rust).position(|(n, r)| n != r) {
            let word = offset & !3;
            panic!(
                "Layer operation{} opcode{} args{:?} mismatch at{offset:x}: native{:x?} rust{:x?}",
                self.operations,
                r.operation,
                r.arguments,
                &native[word..word + 4],
                &rust[word..word + 4]
            );
        }
        self.operations += 1;
    }
    fn step(&mut self, r: Request, state: &mut Gerokasu2Layers) {
        let a = r.arguments;
        match r.operation {
            0 => *state = Gerokasu2Layers::default(),
            1 => {
                state.initialize(a[0]).unwrap();
            }
            2 => {
                state
                    .load(a[0], a[1], a[2], a[3..11].try_into().unwrap())
                    .unwrap();
            }
            3 => {
                state.unload(a[0]).unwrap();
            }
            4 => {
                state.control(a[0], a[1], a[2], a[4]).unwrap();
            }
            5 => state.swap(a[0], a[1]),
            6 => state.select(a[0], a[1]),
            7 => state.select_plane(a[0]).unwrap(),
            8 => state.back_started = true,
            9 => {
                state.clear_plane();
            }
            10 => state.set_mask(a[0], a[1]),
            11 => state.clear_mask(),
            12 => {
                state.records[a[0] as usize].words[105] = a[1];
                state.records[a[0] as usize].words[106] = a[2];
            }
            13 => state.loads_with_init = a[0] as u32,
            14 => state.back_started = false,
            _ => unreachable!(),
        }
        self.verify(r, state);
    }
    fn finish(mut self) -> usize {
        self.input.take();
        let mut byte = [0; 1];
        assert_eq!(self.output.read(&mut byte).unwrap(), 0);
        assert!(self.child.wait().unwrap().success());
        self.operations
    }
}

#[test]
#[ignore = "requires supplied executable and guarded CPU layer-state oracle"]
fn gerokasu2_layer_state_native_operations_match_shared_allocator() {
    let mut native = NativeLayers::new();
    let mut state = Gerokasu2Layers::default();
    native.step(request(0, &[]), &mut state);
    for plane in 0..4 {
        native.step(request(7, &[plane]), &mut state);
        for id in [9, 8, 7, 255] {
            native.step(request(2, &[id, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8]), &mut state);
        }
        for special in [-1, -2, -3, -4] {
            for property in 0..90 {
                native.step(
                    request(4, &[special, property, i32::MAX, 15, 0]),
                    &mut state,
                );
                native.step(request(4, &[special, property, 1, 15, 0x80]), &mut state);
            }
            native.step(request(1, &[special]), &mut state);
        }
        native.step(request(6, &[9, 7]), &mut state);
        for property in 0..90 {
            native.step(request(4, &[-6, property, -1001, 0, 0]), &mut state);
            native.step(request(4, &[-6, property, i32::MIN, 0, 0x80]), &mut state);
        }
        native.step(request(1, &[-6]), &mut state);
        for range in [[8, 9], [7, 9], [0, 255], [255, 255]] {
            native.step(request(6, &range), &mut state);
            native.step(request(4, &[-6, 0, 7, 0, 0x80]), &mut state);
            native.step(request(4, &[-6, 0, 7, 0, 0x80]), &mut state); // cached/singleton path
        }
        native.step(request(5, &[8, 9]), &mut state);
        native.step(request(5, &[9, 42]), &mut state);
        native.step(request(5, &[255, 255]), &mut state);
        native.step(request(3, &[7]), &mut state);
        native.step(request(2, &[6, 2, 0]), &mut state);
        for flags in [0, 1, 2, 3, 0x102] {
            native.step(request(4, &[6, 0, 123, 0, 0]), &mut state);
            native.step(request(2, &[6, 1, flags]), &mut state);
        }
        let bank = state.target(LayerId::new(6)).unwrap().layerbank.raw();
        native.step(request(12, &[i32::from(bank), 0x11223301, 77]), &mut state);
        native.step(request(2, &[6, 9, 2]), &mut state);
        assert!(!state.record(LayerbankId::new(bank)).interaction_completed());
        assert_eq!(
            state.record(LayerbankId::new(bank)).interaction_result(),
            77
        );
        native.step(request(10, &[17, 0x12345678]), &mut state);
        native.step(request(11, &[]), &mut state);
        native.step(request(9, &[]), &mut state);
        assert!(state.target(LayerId::new(255)).is_none());
    }
    native.step(request(0, &[]), &mut state);
    native.step(request(6, &[0, 255]), &mut state);
    native.step(request(13, &[-1]), &mut state);
    native.step(request(2, &[-6, 1, 0]), &mut state); // ascending allocation and exhaustion
    assert_eq!(state.loads_with_init(), 47);
    assert!(state.target(LayerId::new(47)).is_some());
    assert!(state.target(LayerId::new(48)).is_none());
    native.step(request(3, &[-6]), &mut state);
    native.step(request(2, &[255, 2, 2]), &mut state); // freed slots retain raw stale words
    native.step(request(8, &[]), &mut state);
    native.step(request(14, &[]), &mut state);
    assert!(!state.back_started());
    for range in [[-1, 65537], [i32::MIN, i32::MAX], [65536, 65537]] {
        native.step(request(6, &range), &mut state);
        native.step(request(0, &[]), &mut state);
    }
    println!(
        "GEROKASU2_LAYER_STATE_SYNTHETIC operations={} complete_integer_bytes=24732 allocator_cache_and_48_records=verified",
        native.finish()
    );
}

fn command_request(command: &Gerokasu2RuntimeCommand) -> Option<Request> {
    use Gerokasu2RuntimeCommand as C;
    Some(match command {
        C::LayerInit { layer_id } => request(1, &[*layer_id]),
        C::LayerLoad {
            layer_id,
            layer_type,
            flags,
            params,
        } => request(
            2,
            &[
                *layer_id,
                *layer_type,
                *flags,
                params.0,
                params.1,
                params.2,
                params.3,
                params.4,
                params.5,
                params.6,
                params.7,
            ],
        ),
        C::LayerUnload {
            layer_id,
            delay_time,
        } => request(3, &[*layer_id, *delay_time]),
        C::LayerCtrl {
            layer_id,
            property_id,
            params,
        } => request(
            4,
            &[
                *layer_id,
                *property_id,
                params.0,
                params.1,
                params.2,
                params.3,
                params.4,
                params.5,
                params.6,
                params.7,
            ],
        ),
        C::LayerSelect { start, end } => request(6, &[*start, *end]),
        C::Shared(RuntimeCommand::LAYERSWAP(c)) => request(5, &[c.arg1, c.arg2]),
        C::PlaneSelect { plane } => request(7, &[*plane]),
        C::MaskLoad { mask_id, flags, .. } => request(10, &[*mask_id, *flags]),
        C::Shared(RuntimeCommand::MASKUNLOAD(_)) => request(11, &[]),
        C::Shared(RuntimeCommand::PAGEBACK(_)) => request(8, &[]),
        C::Wipe { .. } => request(14, &[]),
        C::Shared(RuntimeCommand::PLANECLEAR(_)) => request(9, &[]),
        _ => return None,
    })
}

#[test]
#[ignore = "requires supplied ROM/executable and guarded CPU layer-state oracle"]
fn gerokasu2_layer_state_authored_opening_matches_native_consumers() {
    use shin_core::primitives::stateless_reader::StatelessFile;
    use shin_core::{
        format::{rom::RomReader, scenario::gerokasu2::Gerokasu2Scenario},
        vm::{command::CommandResult, gerokasu2::Gerokasu2Scripter},
    };
    use std::sync::Arc;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Arc::new(Gerokasu2Scenario::new(bytes.into()).unwrap());
    let mut native = NativeLayers::new();
    let mut kinds = std::collections::BTreeSet::new();
    let mut layer_commands = 0;
    for initial in [0, 1, 2, 3] {
        let mut state = Gerokasu2Layers::default();
        native.step(request(0, &[]), &mut state);
        let mut vm = Gerokasu2Scripter::new(scenario.clone(), initial, 0x12345678);
        let mut result = CommandResult::None;
        for _ in 0..512 {
            let command = vm.run(result, 100000).unwrap();
            let request = command_request(&command);
            assert_eq!(state.apply(&command).unwrap(), request.is_some());
            if let Some(request) = request {
                native.verify(request, &state);
                kinds.insert(scenario.raw()[vm.position().0 as usize]);
                layer_commands += 1;
            }
            if vm.is_halted() {
                break;
            }
            result = vm
                .pending_destination()
                .map(|register| CommandResult::WriteMemory(register, 0))
                .unwrap_or(CommandResult::None);
        }
    }
    println!(
        "GEROKASU2_LAYER_STATE_AUTHORED layer_commands={layer_commands} operations={} opcode_kinds={kinds:x?} full_integer_state=verified; same boot prefix, zero host completions, no draw effects",
        native.finish()
    );
}

#[test]
#[ignore = "requires supplied ROM/executable and guarded CPU layer-state oracle"]
fn gerokasu2_decoded_mask_plane_wipe_commands_match_native_integer_consumers() {
    use shin_core::{
        format::{rom::RomReader, scenario::gerokasu2::Gerokasu2Scenario},
        primitives::stateless_reader::StatelessFile,
        vm::{command::CommandResult, gerokasu2::Gerokasu2Scripter},
    };
    use std::sync::Arc;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let code = u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
    bytes.truncate(code); // Only the in-memory fixture's code is replaced.
    bytes.extend([
        0xcc, 2, // PLANESELECT2
        0xce, 13, 0x7f, 0xb0, // MASKLOAD13,flags-1,raw permission r0=MAX
        0xcf, // MASKUNLOAD retains flags
        0xcb, // PAGEBACK
        0x8e, 0, 0, 0, 0, // WIPE clears the integer back flag
        0xcc, 0, // PLANESELECT0
        0xce, 0x7f, 2, 0xe0, // MASKLOAD-1,flags2,raw permission MIN
        0xcd, // PLANECLEAR clears ID, retains flags
        0xce, 13, 0xb0, 1, // MASKLOAD13,flags r0=MAX
        0xcf, 0xcd, // MASKUNLOAD then PLANECLEAR
        0, 0, 0, // EXIT
    ]);
    let size = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    let mut vm = Gerokasu2Scripter::new(
        Arc::new(Gerokasu2Scenario::new(bytes.into()).unwrap()),
        i32::MAX,
        42,
    );
    let mut state = Gerokasu2Layers::default();
    let mut native = NativeLayers::new();
    native.step(request(0, &[]), &mut state);
    let mut count = 0;
    loop {
        let command = vm.run(CommandResult::None, 100).unwrap();
        if vm.is_halted() {
            break;
        }
        assert!(state.apply(&command).unwrap());
        native.verify(command_request(&command).unwrap(), &state);
        count += 1;
    }
    assert_eq!(state.current_plane().raw(), 0);
    assert!(!state.back_started());
    assert_eq!(state.planes[0].mask, [-1, i32::MAX]);
    assert_eq!(state.planes[2].mask, [-1, -1]);
    println!(
        "GEROKASU2_LAYER_STATE_MASK_BRIDGE commands={count} operations={} decoded_raw_words_to_integer_consumers=verified",
        native.finish()
    );
}
