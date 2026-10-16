use super::*;
use crate::format::{
    scenario::{
        instructions::{
            BinaryOperation, BinaryOperationType, Expression, ExpressionTerm, JumpCond,
            UnaryOperation,
        },
        types::{U8SmallList, U8SmallNumberList},
    },
    text::utf8::U16Utf8String,
};
use binrw::BinWrite;
use std::io::{Cursor, Read, Write};

fn reg(index: u16) -> Register {
    Register::from_regular_register(index)
}
fn arg(index: u16) -> Register {
    Register::from_argument(index)
}
fn constant(value: i32) -> NumberSpec {
    NumberSpec::constant(value)
}
fn number(register: Register) -> NumberSpec {
    NumberSpec::register(register)
}

fn prefix() -> Vec<u8> {
    let mut data = b"SNR ".to_vec();
    for word in [0u32, 0, 1, 1, 0, 0, 0, 0] {
        data.extend(word.to_le_bytes());
    }
    let mut tables = Vec::new();
    for index in 0..10 {
        data.extend((0x4c + tables.len() as u32).to_le_bytes());
        if index < 7 {
            tables.extend(4u32.to_le_bytes());
        }
        tables.extend(0u32.to_le_bytes());
    }
    data.extend(tables);
    let start = data.len() as u32;
    data[32..36].copy_from_slice(&start.to_le_bytes());
    data
}

#[test]
#[ignore = "requires original Ger VM oracle;raw audio operands and supplied prefix"]
fn gerokasu2_audio_words_preserve_all_native_integer_operands() {
    let mut native = NativeVm::new();
    for (opcode, count) in [
        (0x90, 4),
        (0x91, 1),
        (0x92, 2),
        (0x93, 1),
        (0x94, 1),
        (0x95, 7),
        (0x96, 2),
        (0x97, 1),
        (0x98, 3),
        (0x99, 3),
        (0x9a, 2),
        (0x9b, 5),
    ] {
        for base in [i32::MIN, -3000, -1, 0, 1, 2000, 9000, i32::MAX] {
            let mut data = prefix();
            data.push(opcode);
            let words: Vec<_> = (0..count).map(|i| base.wrapping_add(i)).collect();
            for i in 0..words.len() {
                spec(&mut data, number(reg(i as u16 + 1)));
            }
            end(&mut data);
            let mut vm = Gerokasu2Scripter::new(finish(data), 0, 42);
            let initial: Vec<_> = words
                .iter()
                .enumerate()
                .map(|(i, &v)| (i as u16 + 1, v))
                .collect();
            for &(i, v) in &initial {
                vm.ctx.write_register(reg(i), v);
            }
            native.reset(&vm, &initial);
            let Gerokasu2RuntimeCommand::Audio(command) =
                native.compare(&mut vm, CommandResult::None)
            else {
                panic!("audio variant")
            };
            assert_eq!(command.opcode(), opcode);
            assert_eq!(command.words(), words);
            native.compare(&mut vm, CommandResult::None);
        }
    }
    println!(
        "GEROKASU2_AUDIO_VM commands=96 native_yields={} raw_signed_fade_loop_count_volume_pan_speed_status_bits=verified",
        native.finish()
    );
}
fn finish(mut data: Vec<u8>) -> Arc<Gerokasu2Scenario> {
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    Arc::new(Gerokasu2Scenario::new(data.into()).unwrap())
}
fn emit(data: &mut Vec<u8>, instruction: Instruction) {
    let mut c = Cursor::new(Vec::new());
    instruction.write_le(&mut c).unwrap();
    data.extend(c.into_inner());
}
fn spec(data: &mut Vec<u8>, number: NumberSpec) {
    let mut c = Cursor::new(Vec::new());
    number.write_le(&mut c).unwrap();
    data.extend(c.into_inner());
}
fn text(data: &mut Vec<u8>, value: &str) {
    let mut c = Cursor::new(Vec::new());
    U16Utf8String::new(value).write_le(&mut c).unwrap();
    data.extend(c.into_inner());
}
fn end(data: &mut Vec<u8>) {
    data.extend([0, 0, 0]);
}
fn mov(data: &mut Vec<u8>, destination: Register, value: NumberSpec) {
    emit(
        data,
        Instruction::bo(BinaryOperation {
            ty: BinaryOperationType::MovRight,
            destination,
            left: number(destination),
            right: value,
        }),
    );
}

#[test]
fn gerokasu2_vm_feedback_boundaries_utf8_and_game_specific_values() {
    let mut data = prefix();
    data.extend([0x81, 31, 0, 7]); // persistent query
    data.extend([0x85, 0x85, 0x55]); // raw style 0x555, no Umineko style conversion
    data.extend([0x86, 0, 0, 0, 1]);
    spec(&mut data, number(reg(31)));
    text(&mut data, "日本@rかわいい");
    data.extend([0x8b, 0xbf]); // register15
    data.extend([0xc1, 0, 9, 0, 0]); // layer0,type9,flags0,zero parameter mask
    end(&mut data);
    let mut vm = Gerokasu2Scripter::new(finish(data), 0, 42);
    assert!(matches!(
        vm.run(CommandResult::None, 100).unwrap(),
        Gerokasu2RuntimeCommand::Shared(RuntimeCommand::SGET(_))
    ));
    let (pc, seed) = (vm.next_position(), vm.random_state());
    assert!(vm.run(CommandResult::None, 100).is_err());
    assert!(vm.run(CommandResult::WriteMemory(reg(30), 1), 100).is_err());
    assert_eq!((vm.next_position(), vm.random_state()), (pc, seed));
    assert!(matches!(
        vm.run(CommandResult::WriteMemory(reg(31), 123), 100)
            .unwrap(),
        Gerokasu2RuntimeCommand::MessageInit { style: 0x555 }
    ));
    let Gerokasu2RuntimeCommand::MessageSet {
        msg_id,
        message_arg,
        text,
        auto_wait,
    } = vm.run(CommandResult::None, 100).unwrap()
    else {
        panic!("MSGSET");
    };
    assert_eq!(msg_id.0, u32::MAX);
    assert_eq!(message_arg, 123);
    assert_eq!(text, "日本@rかわいい");
    assert!(auto_wait);
    assert!(matches!(
        vm.run(CommandResult::None, 100).unwrap(),
        Gerokasu2RuntimeCommand::MessageFace { face: 0 }
    ));
    assert!(matches!(
        vm.run(CommandResult::None, 100).unwrap(),
        Gerokasu2RuntimeCommand::LayerLoad { layer_type: 9, .. }
    ));
    assert!(matches!(
        vm.run(CommandResult::None, 100).unwrap(),
        Gerokasu2RuntimeCommand::Shared(RuntimeCommand::EXIT(_))
    ));
    assert!(vm.is_halted());
    assert!(vm.run(CommandResult::None, 100).is_err());
}

#[test]
fn gerokasu2_vm_rejects_stack_errors_and_bounds_control_loops() {
    let mut data = prefix();
    let start = data.len() as u32;
    emit(
        &mut data,
        Instruction::j {
            target: CodeAddress(start),
        },
    );
    let mut vm = Gerokasu2Scripter::new(finish(data), 0, 1);
    assert!(
        vm.run(CommandResult::None, 10)
            .unwrap_err()
            .to_string()
            .contains("budget exhausted")
    );
    assert_eq!(vm.next_position(), CodeAddress(start));
    for instruction in [Instruction::retsub {}, Instruction::r#return {}] {
        let mut data = prefix();
        emit(&mut data, instruction);
        end(&mut data);
        assert!(
            Gerokasu2Scripter::new(finish(data), 0, 1)
                .run(CommandResult::None, 100)
                .unwrap_err()
                .to_string()
                .contains("underflow")
        );
    }
    let mut data = prefix();
    emit(
        &mut data,
        Instruction::j {
            target: CodeAddress(4),
        },
    );
    end(&mut data);
    assert!(
        Gerokasu2Scripter::new(finish(data), 0, 1)
            .run(CommandResult::None, 100)
            .is_err()
    );
    let mut data = prefix();
    emit(
        &mut data,
        Instruction::push {
            values: U8SmallNumberList::from_contents((0..65).map(|_| constant(1))),
        },
    );
    end(&mut data);
    assert!(
        Gerokasu2Scripter::new(finish(data), 0, 1)
            .run(CommandResult::None, 100)
            .unwrap_err()
            .to_string()
            .contains("overflow")
    );
}

struct NativeVm {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::process::ChildStdout,
    yields: usize,
}
impl NativeVm {
    fn new() -> Self {
        use std::process::{Command, Stdio};
        let mut child = Command::new(
            std::env::var_os("SHIN_GEROKASU2_VM_ORACLE").expect("SHIN_GEROKASU2_VM_ORACLE"),
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
            yields: 0,
        }
    }
    fn words(&mut self, values: &[u32]) {
        for value in values {
            self.input
                .as_mut()
                .unwrap()
                .write_all(&value.to_le_bytes())
                .unwrap();
        }
    }
    fn reset(&mut self, vm: &Gerokasu2Scripter, initial: &[(u16, i32)]) {
        self.words(&[
            0,
            vm.scenario.raw().len() as u32,
            vm.scenario.header.code_offset,
            vm.random_state(),
            vm.read_register(reg(0)) as u32,
            vm.scenario.bustup_info.len() as u32,
        ]);
        self.input
            .as_mut()
            .unwrap()
            .write_all(vm.scenario.raw())
            .unwrap();
        for info in &vm.scenario.bustup_info {
            self.input
                .as_mut()
                .unwrap()
                .write_all(&info.lipsync_character_id.to_le_bytes())
                .unwrap();
        }
        self.words(&[initial.len() as u32]);
        for &(register, value) in initial {
            self.input
                .as_mut()
                .unwrap()
                .write_all(&register.to_le_bytes())
                .unwrap();
            self.input
                .as_mut()
                .unwrap()
                .write_all(&value.to_le_bytes())
                .unwrap();
        }
        let mut ack = [0; 4];
        self.output.read_exact(&mut ack).unwrap();
        assert_eq!(&ack, b"ACK1");
    }
    fn compare(
        &mut self,
        vm: &mut Gerokasu2Scripter,
        result: CommandResult,
    ) -> Gerokasu2RuntimeCommand {
        let feedback = match result {
            CommandResult::None => [0, 0, 0],
            CommandResult::WriteMemory(destination, value) => {
                let bits = match destination.repr() {
                    crate::format::scenario::instruction_elements::RegisterRepr::Regular(v) => v,
                    crate::format::scenario::instruction_elements::RegisterRepr::Argument(v) => {
                        0x1000 + v
                    }
                };
                [1, u32::from(bits), value as u32]
            }
        };
        self.words(&[1]);
        self.words(&feedback);
        let command = vm.run(result, 100000).unwrap();
        let mut header = [0u8; 60];
        self.output.read_exact(&mut header).unwrap();
        assert_eq!(&header[..4], b"NVM2");
        let get = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
        assert_eq!(
            (get(4), get(8), get(12), get(16), get(20)),
            (
                u32::from(vm.scenario.raw()[vm.position().0 as usize]),
                vm.next_position().0,
                vm.random_state(),
                vm.position().0,
                vm.previous_random_state()
            ),
            "yield {}",
            self.yields
        );
        let mut expected_text = "";
        let fields = match &command {
            Gerokasu2RuntimeCommand::Audio(c) => {
                let words = c.words();
                std::array::from_fn(|i| words.get(i).copied().unwrap_or(0) as u32)
            }
            Gerokasu2RuntimeCommand::Wait {
                allow_interrupt,
                amount,
            } => [u32::from(*allow_interrupt), *amount as u32, 0, 0],
            Gerokasu2RuntimeCommand::Shared(RuntimeCommand::EXIT(c)) => {
                [u32::from(c.arg1), c.arg2 as u32, 0, 0]
            }
            Gerokasu2RuntimeCommand::Shared(RuntimeCommand::SGET(c)) => [
                register_bits(vm.pending_destination.unwrap()),
                c.slot_number as u32,
                0,
                0,
            ],
            Gerokasu2RuntimeCommand::MessageInit { style } => [*style as u32, 0, 0, 0],
            Gerokasu2RuntimeCommand::MessageFace { face } => [*face as u32, 0, 0, 0],
            Gerokasu2RuntimeCommand::VoicePlay {
                name,
                volume,
                flags,
            } => {
                expected_text = name;
                [*volume as u32, *flags as u32, 0, 0]
            }
            Gerokasu2RuntimeCommand::VoiceWait { statuses } => [*statuses as u32, 0, 0, 0],
            Gerokasu2RuntimeCommand::MessageSet {
                msg_id,
                auto_wait,
                message_arg,
                text,
            } => {
                expected_text = text;
                [msg_id.0, u32::from(*auto_wait), *message_arg as u32, 0]
            }
            Gerokasu2RuntimeCommand::Selection {
                dest,
                choice_set_base,
                choice_index,
                visibility,
                title,
                ..
            } => {
                expected_text = title;
                [
                    *choice_set_base as u32,
                    *choice_index as u32,
                    register_bits(*dest),
                    *visibility as u32,
                ]
            }
            Gerokasu2RuntimeCommand::Shared(RuntimeCommand::QUIZ(c)) => [
                register_bits(vm.pending_destination.unwrap()),
                c.arg as u32,
                0,
                0,
            ],
            Gerokasu2RuntimeCommand::LayerInit { layer_id }
            | Gerokasu2RuntimeCommand::TransWait { layer_id } => [*layer_id as u32, 0, 0, 0],
            Gerokasu2RuntimeCommand::LayerUnload {
                layer_id,
                delay_time,
            } => [*layer_id as u32, *delay_time as u32, 0, 0],
            Gerokasu2RuntimeCommand::LayerSelect { start, end } => {
                [*start as u32, *end as u32, 0, 0]
            }
            Gerokasu2RuntimeCommand::MovieWait { layer_id, statuses } => {
                [*layer_id as u32, *statuses as u32, 0, 0]
            }
            Gerokasu2RuntimeCommand::LayerLoad {
                layer_id,
                layer_type,
                flags,
                ..
            } => [*layer_id as u32, *layer_type as u32, *flags as u32, 0],
            Gerokasu2RuntimeCommand::LayerCtrl {
                layer_id,
                property_id,
                ..
            } => [*layer_id as u32, *property_id as u32, 0, 0],
            Gerokasu2RuntimeCommand::LayerWait { layer_id, .. } => [*layer_id as u32, 0, 0, 0],
            Gerokasu2RuntimeCommand::Wipe {
                ty,
                flags,
                duration,
                ..
            }
            | Gerokasu2RuntimeCommand::TransitionSet {
                ty,
                flags,
                duration,
                ..
            } => [*ty as u32, *flags as u32, *duration as u32, 0],
            Gerokasu2RuntimeCommand::PlaneSelect { plane } => [*plane as u32, 0, 0, 0],
            Gerokasu2RuntimeCommand::MaskLoad {
                mask_id,
                flags,
                animation_permission_arg,
            } => [
                *mask_id as u32,
                *flags as u32,
                *animation_permission_arg as u32,
                0,
            ],
            Gerokasu2RuntimeCommand::Shared(RuntimeCommand::LAYERSWAP(c)) => {
                [c.arg1 as u32, c.arg2 as u32, 0, 0]
            }
            _ => [0; 4],
        };
        for (i, &value) in fields.iter().enumerate() {
            assert_eq!(
                get(24 + i * 4),
                value,
                "command field {i} at {}",
                vm.position()
            );
        }
        assert_eq!(get(40) as usize, expected_text.len());
        let mut text = vec![0; expected_text.len()];
        self.output.read_exact(&mut text).unwrap();
        assert_eq!(text, expected_text.as_bytes());
        let parameters = match &command {
            Gerokasu2RuntimeCommand::Audio(c) => c.words(),
            Gerokasu2RuntimeCommand::LayerLoad { params, .. }
            | Gerokasu2RuntimeCommand::LayerCtrl { params, .. }
            | Gerokasu2RuntimeCommand::Wipe { params, .. }
            | Gerokasu2RuntimeCommand::TransitionSet { params, .. } => vec![
                params.0, params.1, params.2, params.3, params.4, params.5, params.6, params.7,
            ],
            Gerokasu2RuntimeCommand::LayerWait { properties, .. } => properties.clone(),
            _ => vec![],
        };
        assert_eq!(
            get(52) as usize,
            parameters.len(),
            "parameter count at {}",
            vm.position()
        );
        let mut native_parameters = vec![0; parameters.len() * 4];
        self.output.read_exact(&mut native_parameters).unwrap();
        for (word, value) in native_parameters.chunks_exact(4).zip(parameters) {
            assert_eq!(
                i32::from_le_bytes(word.try_into().unwrap()),
                value,
                "parameter at {}",
                vm.position()
            );
        }
        let expected_choices: &[String] = match &command {
            Gerokasu2RuntimeCommand::Selection { variants, .. } => variants,
            _ => &[],
        };
        assert_eq!(
            get(56) as usize,
            expected_choices.len(),
            "choice count at {}",
            vm.position()
        );
        for label in expected_choices {
            let mut size = [0; 4];
            self.output.read_exact(&mut size).unwrap();
            assert_eq!(
                u32::from_le_bytes(size) as usize,
                label.len(),
                "choice length at {}",
                vm.position()
            );
            let mut bytes = vec![0; label.len()];
            self.output.read_exact(&mut bytes).unwrap();
            assert_eq!(bytes, label.as_bytes(), "UTF8 choice at {}", vm.position());
        }
        let mut registers = vec![0u8; 2048 * 4];
        self.output.read_exact(&mut registers).unwrap();
        for index in 0..2048 {
            assert_eq!(
                u32::from_le_bytes(registers[index * 4..index * 4 + 4].try_into().unwrap()),
                vm.read_register(reg(index as u16)) as u32,
                "register{index} at {}",
                vm.position()
            );
        }
        let depth = get(44) as usize;
        let top = get(48) as usize;
        let mut stack = vec![0u8; (depth + 256 - top) * 4];
        self.output.read_exact(&mut stack).unwrap();
        let words = stack
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect::<Vec<_>>();
        let mut native = vec![depth as u32];
        native.extend_from_slice(&words[..depth]);
        native.push(top as u32);
        native.extend_from_slice(&words[depth..]);
        let expected = vm
            .ctx
            .reference_stack_snapshot()
            .iter()
            .map(|&word| word as u32)
            .collect::<Vec<_>>();
        assert_eq!(native, expected, "stacks at {}", vm.position());
        self.yields += 1;
        command
    }
    fn finish(mut self) -> usize {
        self.input.take();
        let mut byte = [0u8; 1];
        assert_eq!(self.output.read(&mut byte).unwrap(), 0);
        assert!(self.child.wait().unwrap().success());
        self.yields
    }
}

#[test]
#[ignore = "requires supplied executable and guarded CPU VM oracle"]
fn gerokasu2_vm_native_layer_arrays_transitions_masks_and_choice_labels() {
    let mut native = NativeVm::new();
    let mut programs = 0;
    let initial = [
        (1, i32::MIN),
        (2, -1001),
        (3, -1),
        (4, 0),
        (5, 1),
        (6, 1001),
        (7, i32::MAX),
        (8, 0x12345678),
        (9, 0x40000002),
    ];
    for mask in 0u8..=255 {
        let mut data = prefix();
        for (opcode, scalars) in [
            (0xc1, vec![constant(-6), constant(9), number(reg(9))]),
            (0xc3, vec![constant(-6), constant(89)]),
            (0x8e, vec![constant(17), constant(i32::MIN), number(reg(2))]),
            (0xc9, vec![constant(-1), constant(-1), number(reg(7))]),
        ] {
            data.push(opcode);
            for scalar in scalars {
                spec(&mut data, scalar);
            }
            data.push(mask);
            for index in 0..8 {
                if mask & (1 << index) != 0 {
                    spec(&mut data, number(reg(index + 1)));
                }
            }
        }
        end(&mut data);
        compare_program(&mut native, finish(data), 0x12345678, &initial);
        programs += 1;
    }
    for count in [0, 1, 31, 32] {
        let mut data = prefix();
        data.push(0xc4);
        spec(&mut data, constant(-6));
        data.push(count);
        for index in 0..count {
            spec(&mut data, constant(i32::from(index) * 3 % 90));
        }
        end(&mut data);
        compare_program(&mut native, finish(data), 42, &[]);
        programs += 1;
    }
    for count in 0..=12 {
        let mut data = prefix();
        data.push(0x8d);
        data.extend(0u16.to_le_bytes());
        data.extend(u16::MAX.to_le_bytes());
        data.extend(31u16.to_le_bytes());
        spec(&mut data, number(reg(7)));
        text(&mut data, "選択@r🐈");
        let mut labels = Vec::new();
        for index in 0..count {
            labels.extend(format!("選択{index}：レモン@rかわいい🐈").as_bytes());
            labels.push(0);
        }
        labels.push(0);
        data.extend((labels.len() as u16).to_le_bytes());
        data.extend(labels);
        end(&mut data);
        compare_program(&mut native, finish(data), 42, &initial);
        programs += 1;
    }
    let mut data = prefix();
    for plane in [
        constant(0),
        constant(1),
        constant(2),
        constant(3),
        constant(-1),
        number(reg(7)),
    ] {
        data.push(0xcc);
        spec(&mut data, plane);
    }
    for mask in [-1, 0, 65535] {
        data.push(0xce);
        spec(&mut data, constant(mask));
        spec(&mut data, constant(i32::MIN));
        spec(&mut data, number(reg(7)));
    }
    data.extend([0xcf, 0xcb, 0xcd]);
    data.push(0xc5);
    spec(&mut data, constant(255));
    spec(&mut data, constant(0));
    end(&mut data);
    compare_program(&mut native, finish(data), 42, &initial);
    programs += 1;
    println!(
        "GEROKASU2_VM_PAYLOADS programs={programs} native_yields={} mask_values=256 layer_load_ctrl_wipe_transset_words=verified wait_counts=0,1,31,32 choice_counts=0..12 UTF8_labels=verified raw_planes_masks_flags=verified",
        native.finish()
    );
}
fn register_bits(register: Register) -> u32 {
    match register.repr() {
        crate::format::scenario::instruction_elements::RegisterRepr::Regular(index) => {
            u32::from(index)
        }
        crate::format::scenario::instruction_elements::RegisterRepr::Argument(index) => {
            0x1000 + u32::from(index)
        }
    }
}

fn compare_program(
    native: &mut NativeVm,
    scenario: Arc<Gerokasu2Scenario>,
    seed: u32,
    initial: &[(u16, i32)],
) {
    let mut vm = Gerokasu2Scripter::new(scenario, 0, seed);
    for &(index, value) in initial {
        vm.ctx.write_register(reg(index), value);
    }
    native.reset(&vm, initial);
    let mut result = CommandResult::None;
    for _ in 0..1000 {
        native.compare(&mut vm, result);
        if vm.is_halted() {
            return;
        }
        result = vm
            .pending_destination()
            .map(|register| CommandResult::WriteMemory(register, 7))
            .unwrap_or(CommandResult::None);
    }
    panic!("Synthetic program did not terminate");
}

#[test]
#[ignore = "requires supplied executable and guarded CPU VM oracle"]
fn gerokasu2_vm_native_wait_byte_and_signed_duration() {
    let mut native = NativeVm::new();
    for amount in [i32::MIN, -12, 0, 1, 12, i32::MAX] {
        let mut data = prefix();
        for allow in 0..=255u8 {
            data.extend([0x83, allow]);
            spec(&mut data, number(reg(1)));
        }
        end(&mut data);
        compare_program(&mut native, finish(data), 42, &[(1, amount)]);
    }
    println!(
        "GEROKASU2_VM_WAIT programs=6 native_yields={} all_raw_allow_bytes_signed_duration=verified",
        native.finish()
    );
}

#[test]
#[ignore = "requires supplied executable and guarded CPU VM oracle"]
fn gerokasu2_vm_native_voice_operands_remain_raw() {
    let mut native = NativeVm::new();
    let values = [i32::MIN, -1, 0, 1, 700, 1000, 1001, i32::MAX];
    for volume in values {
        let mut data = prefix();
        let flags = [i32::MIN, -1, 0, 1, 2, 3, i32::MAX];
        let mut initial = vec![(1, volume)];
        for (index, flag) in flags.iter().enumerate() {
            let register = 10 + index as u16;
            initial.push((register, *flag));
            data.push(0x9c);
            text(&mut data, "声/Alice_0001");
            spec(&mut data, number(reg(1)));
            spec(&mut data, number(reg(register)));
            data.push(0x9e);
            spec(&mut data, number(reg(register)));
        }
        data.push(0x9d);
        end(&mut data);
        compare_program(&mut native, finish(data), 42, &initial);
    }
    println!(
        "GEROKASU2_VM_VOICE programs=8 native_yields={} raw_volume_flags_status_UTF8=verified",
        native.finish()
    );
}

#[test]
#[ignore = "requires supplied executable and guarded CPU VM oracle"]
fn gerokasu2_vm_native_arithmetic_random_builtins_and_stacks() {
    let mut native = NativeVm::new();
    let seeds = [0, 1, 0x12345678, 0x80000000, u32::MAX];
    let boundary = [i32::MIN, -1001, -1, 0, 1, 1001, i32::MAX];
    let mut programs = 0;
    for seed in seeds {
        for left in boundary {
            for right in boundary {
                let mut data = prefix();
                for operation in 0..12 {
                    let ty = num_traits::FromPrimitive::from_u8(operation).unwrap();
                    emit(
                        &mut data,
                        Instruction::uo(UnaryOperation {
                            ty,
                            destination: reg(32 + operation as u16),
                            source: number(reg(1)),
                        }),
                    );
                }
                for operation in 0..18 {
                    let ty = num_traits::FromPrimitive::from_u8(operation).unwrap();
                    emit(
                        &mut data,
                        Instruction::bo(BinaryOperation {
                            ty,
                            destination: reg(64 + operation as u16),
                            left: number(reg(1)),
                            right: number(reg(17)),
                        }),
                    );
                }
                for index in 0..16 {
                    emit(
                        &mut data,
                        Instruction::rnd {
                            dest: reg(96 + index),
                            min: number(reg(1)),
                            max: number(reg(17)),
                        },
                    );
                }
                emit(
                    &mut data,
                    Instruction::exp {
                        dest: reg(31),
                        expr: Expression::new([
                            ExpressionTerm::Push(number(reg(1))),
                            ExpressionTerm::Push(number(reg(17))),
                            ExpressionTerm::Add,
                            ExpressionTerm::Push(constant(1000)),
                            ExpressionTerm::MultiplyReal,
                        ])
                        .unwrap(),
                    },
                );
                let terms = [
                    ExpressionTerm::Add,
                    ExpressionTerm::Subtract,
                    ExpressionTerm::Multiply,
                    ExpressionTerm::Divide,
                    ExpressionTerm::Modulo,
                    ExpressionTerm::ShiftLeft,
                    ExpressionTerm::ShiftRight,
                    ExpressionTerm::BitwiseAnd,
                    ExpressionTerm::BitwiseOr,
                    ExpressionTerm::BitwiseXor,
                    ExpressionTerm::Negate,
                    ExpressionTerm::BitwiseNot,
                    ExpressionTerm::Abs,
                    ExpressionTerm::CmpEqual,
                    ExpressionTerm::CmpNotEqual,
                    ExpressionTerm::CmpGreaterOrEqual,
                    ExpressionTerm::CmpGreater,
                    ExpressionTerm::CmpLessOrEqual,
                    ExpressionTerm::CmpLess,
                    ExpressionTerm::CmpZero,
                    ExpressionTerm::CmpNotZero,
                    ExpressionTerm::LogicalAnd,
                    ExpressionTerm::LogicalOr,
                    ExpressionTerm::Select,
                    ExpressionTerm::MultiplyReal,
                    ExpressionTerm::DivideReal,
                    ExpressionTerm::Sin,
                    ExpressionTerm::Cos,
                    ExpressionTerm::Tan,
                    ExpressionTerm::Min,
                    ExpressionTerm::Max,
                ];
                for (index, term) in terms.into_iter().enumerate() {
                    let mut expression = match term.argument_count() {
                        1 => vec![ExpressionTerm::Push(number(reg(1)))],
                        2 => vec![
                            ExpressionTerm::Push(number(reg(1))),
                            ExpressionTerm::Push(number(reg(17))),
                        ],
                        3 => vec![
                            ExpressionTerm::Push(number(reg(1))),
                            ExpressionTerm::Push(number(reg(17))),
                            ExpressionTerm::Push(number(reg(1))),
                        ],
                        _ => unreachable!(),
                    };
                    expression.push(term);
                    emit(
                        &mut data,
                        Instruction::exp {
                            dest: reg(128 + index as u16),
                            expr: Expression::new(expression).unwrap(),
                        },
                    );
                }
                for condition in 0..8 {
                    for is_negated in [false, true] {
                        let destination = reg(176 + condition * 2 + u16::from(is_negated));
                        let jump = data.len();
                        emit(
                            &mut data,
                            Instruction::jc {
                                cond: JumpCond {
                                    is_negated,
                                    condition: num_traits::FromPrimitive::from_u16(condition)
                                        .unwrap(),
                                },
                                left: number(reg(1)),
                                right: number(reg(17)),
                                target: CodeAddress(0),
                            },
                        );
                        let patch = data.len() - 4;
                        assert!(patch > jump);
                        mov(&mut data, destination, constant(1));
                        let target = data.len() as u32;
                        data[patch..patch + 4].copy_from_slice(&target.to_le_bytes());
                    }
                }
                data.extend([0x43]);
                spec(&mut data, number(reg(31)));
                data.extend(2u16.to_le_bytes());
                data.extend(31u16.to_le_bytes());
                data.extend(30u16.to_le_bytes());
                data.extend([0x45]);
                spec(&mut data, constant(-1));
                spec(&mut data, constant(1));
                data.extend(2u16.to_le_bytes());
                data.extend(31u16.to_le_bytes());
                data.extend(30u16.to_le_bytes());
                for (opcode, start) in [(0x51, 0), (0x52, 1), (0x51, -1)] {
                    data.push(opcode);
                    data.extend(29u16.to_le_bytes());
                    spec(&mut data, number(reg(1)));
                    spec(&mut data, constant(start));
                    data.extend(3u16.to_le_bytes());
                    for value in [number(reg(17)), number(reg(1)), constant(7)] {
                        let p = data.len();
                        spec(&mut data, value);
                        data.resize(p + 4, 0);
                    }
                }
                end(&mut data);
                compare_program(&mut native, finish(data), seed, &[(1, left), (17, right)]);
                programs += 1;
            }
        }
    }
    let mut data = prefix();
    let mut expression = vec![ExpressionTerm::Push(constant(1)); 32];
    expression.extend(std::iter::repeat_n(ExpressionTerm::Add, 31));
    emit(
        &mut data,
        Instruction::exp {
            dest: reg(31),
            expr: Expression::new(expression).unwrap(),
        },
    );
    end(&mut data);
    compare_program(&mut native, finish(data), 42, &[]);
    programs += 1;
    // Sixteen 15-argument frames fill the native 256-word argument bank.
    // Each child takes its parent's arguments before its frame is pushed.
    let mut data = prefix();
    let mut patch = data.len() + 1;
    data.extend([0x4f, 0, 0, 0, 0, 15]);
    for value in 0..15 {
        spec(&mut data, constant(value));
    }
    end(&mut data);
    for depth in 0..16 {
        let target = data.len() as u32;
        data[patch..patch + 4].copy_from_slice(&target.to_le_bytes());
        if depth < 15 {
            patch = data.len() + 1;
            data.extend([0x4f, 0, 0, 0, 0, 15]);
            for index in 0..15 {
                spec(&mut data, number(arg(index)));
            }
        } else {
            data.extend([0x81, 0, 0x10, 7]);
        }
        mov(&mut data, reg(31), number(arg(0)));
        data.push(0x50);
    }
    compare_program(&mut native, finish(data), 42, &[]);
    programs += 1;
    for depth in [1, 8, 64] {
        let mut data = prefix();
        let origin = data.len();
        data.extend([0x48, 0, 0, 0, 0]);
        end(&mut data);
        let mut patch = origin + 1;
        for index in 0..depth {
            let target = data.len() as u32;
            data[patch..patch + 4].copy_from_slice(&target.to_le_bytes());
            if index + 1 < depth {
                patch = data.len() + 1;
                data.extend([0x48, 0, 0, 0, 0]);
            }
            mov(&mut data, reg(31), constant(index));
            data.push(0x49);
        }
        compare_program(&mut native, finish(data), 123, &[]);
        programs += 1;
    }
    let mut data = prefix();
    let origin = data.len();
    data.extend([0x4b, 1, 2, 0]);
    let patch = data.len();
    data.extend([0; 8]);
    end(&mut data);
    let first = data.len() as u32;
    data.push(0x49);
    let second = data.len() as u32;
    mov(&mut data, reg(31), constant(42));
    data.push(0x49);
    data[patch..patch + 4].copy_from_slice(&first.to_le_bytes());
    data[patch + 4..patch + 8].copy_from_slice(&second.to_le_bytes());
    assert!(origin > 0);
    compare_program(&mut native, finish(data), 0, &[]);
    programs += 1;
    let mut data = prefix();
    let call = data.len();
    data.extend([0x4f, 0, 0, 0, 0, 2, 1, 2]);
    end(&mut data);
    let target = data.len() as u32;
    data[call + 1..call + 5].copy_from_slice(&target.to_le_bytes());
    emit(
        &mut data,
        Instruction::push {
            values: U8SmallNumberList::from_contents([number(arg(0)), number(arg(1))]),
        },
    );
    mov(&mut data, arg(0), constant(9));
    emit(
        &mut data,
        Instruction::pop {
            dest: U8SmallList::from_contents([arg(1), arg(0)]),
        },
    );
    mov(&mut data, reg(31), number(arg(0)));
    data.extend([0x81, 0, 0x10, 7, 0x50]);
    compare_program(&mut native, finish(data), 42, &[]);
    programs += 1;
    println!(
        "GEROKASU2_VM_SYNTHETIC programs={programs} native_yields={} full_registers=2048 stacks_seeds_PCs=verified",
        native.finish()
    );
}

#[test]
#[ignore = "requires supplied executable and guarded CPU VM oracle"]
fn gerokasu2_vm_native_utf8_commands_and_resource_queries() {
    let mut native = NativeVm::new();
    let mut data = prefix();
    data.extend([1, 0xe2, 0x84]); // native no-operand slots
    for (selector, index) in [
        (100, 0),
        (100, 1),
        (100, -1),
        (100, 2),
        (99, 0),
        (101, 0),
        (199, 0),
        (200, 0),
    ] {
        data.push(0x53);
        data.extend(31u16.to_le_bytes());
        spec(&mut data, constant(selector));
        spec(&mut data, constant(index));
        data.extend([0x8b, 0xc0, 31]);
    }
    data.extend([0x85, 5]); // preserve raw style for later native clamp
    data.extend([0x86, 1, 0, 0, 1]);
    spec(&mut data, constant(-1));
    text(&mut data, "かわいい@r日本");
    data.push(0x8d);
    data.extend(1u16.to_le_bytes());
    data.extend(0u16.to_le_bytes());
    data.extend(31u16.to_le_bytes());
    spec(&mut data, constant(-1));
    text(&mut data, "選択肢");
    let labels = b"yes\0no\0\0";
    data.extend((labels.len() as u16).to_le_bytes());
    data.extend(labels);
    data.extend([0xe0, 31, 0, 9]);
    end(&mut data);
    let mut scenario = Arc::try_unwrap(finish(data)).ok().unwrap();
    for lip in [u16::MAX, 4] {
        scenario
            .bustup_info
            .push(crate::format::scenario::gerokasu2::BustupInfoItem {
                name: U16Utf8String::new("日本"),
                emotion: U16Utf8String::new("通常"),
                lipsync_character_id: lip,
                raw_placement: [0, 0, 1000],
            });
    }
    compare_program(&mut native, Arc::new(scenario), 42, &[]);
    println!(
        "GEROKASU2_VM_COMMANDS native_yields={} UTF8_message_choice_and_ID_conversion=verified signed_query=verified",
        native.finish()
    );
}

#[test]
#[ignore = "requires supplied Gerokasu2 ROM/executable and guarded CPU VM oracle"]
fn gerokasu2_vm_supplied_boot_prefix_matches_native_state() {
    use crate::format::rom::RomReader;
    use shin_primitives::stateless_reader::StatelessFile;
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(
                std::env::var_os("SHIN_GEROKASU2_ROM").expect("SHIN_GEROKASU2_ROM"),
            )
            .unwrap(),
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
    let mut native = NativeVm::new();
    let mut kinds = std::collections::BTreeSet::new();
    let mut extended_layer_addresses = std::collections::BTreeSet::new();
    for initial in [0, 1, 2, 3] {
        let mut vm = Gerokasu2Scripter::new(scenario.clone(), initial, 0x12345678);
        native.reset(&vm, &[]);
        let mut result = CommandResult::None;
        let mut commands = 0;
        let mut inventory = Vec::new();
        for _ in 0..512 {
            let command = native.compare(&mut vm, result);
            let layer_ids: &[i32] = match &command {
                Gerokasu2RuntimeCommand::LayerLoad { layer_id, .. }
                | Gerokasu2RuntimeCommand::LayerInit { layer_id }
                | Gerokasu2RuntimeCommand::LayerUnload { layer_id, .. }
                | Gerokasu2RuntimeCommand::LayerCtrl { layer_id, .. }
                | Gerokasu2RuntimeCommand::LayerWait { layer_id, .. }
                | Gerokasu2RuntimeCommand::MovieWait { layer_id, .. }
                | Gerokasu2RuntimeCommand::TransWait { layer_id } => std::slice::from_ref(layer_id),
                Gerokasu2RuntimeCommand::LayerSelect { start, end } => {
                    for value in [start, end] {
                        if !(-5..256).contains(value) {
                            extended_layer_addresses.insert(*value);
                        }
                    }
                    &[]
                }
                _ => &[],
            };
            for &value in layer_ids {
                if !(-5..256).contains(&value) {
                    extended_layer_addresses.insert(value);
                }
            }
            kinds.insert(scenario.raw()[vm.position().0 as usize]);
            commands += 1;
            if initial == 0 {
                inventory.push(format!("{:06x} {command:?}", vm.position().0));
            }
            if vm.is_halted() {
                break;
            }
            result = vm
                .pending_destination()
                .map(|register| CommandResult::WriteMemory(register, 0))
                .unwrap_or(CommandResult::None);
            drop(command);
        }
        println!(
            "GEROKASU2_VM_BOOT initial={initial} compared_commands={commands} last_PC={:x}",
            vm.position().0
        );
        if initial == 0 {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("gerokasu2/.local/audio/supplied-prefix.txt");
            std::fs::write(path, inventory.join("\n") + "\n").unwrap();
        }
    }
    println!(
        "GEROKASU2_VM_BOOT_TOTAL native_yields={} opcode_kinds={kinds:x?} extended_layer_addresses={extended_layer_addresses:?}; host completions zero, no ADV effects",
        native.finish()
    );
}

#[test]
fn gerokasu_property_ids_map_onto_shared_numbering() {
    // Blur X stays, blur Y moves to the appended slot, the rest shift down.
    assert_eq!(super::gerokasu_property_id(65), 65);
    assert_eq!(super::gerokasu_property_id(66), 66);
    assert_eq!(super::gerokasu_property_id(67), 90);
    // Mosaic, rain intensity and camera X/Y/Z.
    assert_eq!(super::gerokasu_property_id(68), 67);
    assert_eq!(super::gerokasu_property_id(74), 73);
    assert_eq!([86, 87, 88].map(super::gerokasu_property_id), [85, 86, 87]);
    assert_eq!(super::gerokasu_property_id(90), 89);
}
