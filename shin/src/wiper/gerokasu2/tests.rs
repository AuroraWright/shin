use super::*;
use shin_core::{
    format::{rom::RomReader, scenario::gerokasu2::MaskInfoItem, text::utf8::U16Utf8String},
    primitives::stateless_reader::StatelessFile,
};
use std::{
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

fn scenario() -> Gerokasu2Scenario {
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
    Gerokasu2Scenario::new(bytes.into()).unwrap()
}

struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    requests: usize,
    clocks: usize,
    bytes: usize,
}
impl Native {
    fn new(scenario: &Gerokasu2Scenario) -> Self {
        let mut child = Command::new(std::env::var_os("SHIN_GEROKASU2_TRANSITION_ORACLE").unwrap())
            .args([
                std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_MEDIUM_FONT").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_SYSTEM_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut header = b"GTR1".to_vec();
        header.extend((scenario.mask_info.len() as u32).to_le_bytes());
        for mask in &scenario.mask_info {
            let name = mask.name.as_str();
            header.extend((name.len() as u16).to_le_bytes());
            header.extend(name.as_bytes());
        }
        let mut input = child.stdin.take().unwrap();
        input.write_all(&header).unwrap();
        Self {
            input: Some(input),
            output: child.stdout.take().unwrap(),
            child,
            requests: 0,
            clocks: 0,
            bytes: 0,
        }
    }
    fn row(&mut self, op: u32, kind: u32, value: u32, params: [i32; 8]) {
        let mut words = vec![op, kind, value];
        words.extend(params.map(|p| p as u32));
        self.input
            .as_mut()
            .unwrap()
            .write_all(
                &words
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    fn compare(&mut self, expected: &[u8]) {
        let mut actual = vec![0; expected.len()];
        self.output.read_exact(&mut actual).unwrap();
        assert_eq!(actual, expected);
        self.bytes += expected.len();
    }
    fn request(
        &mut self,
        scenario: &Gerokasu2Scenario,
        kind: i32,
        duration: i32,
        params: [i32; 8],
    ) {
        self.row(0, kind as u32, (duration as f32).to_bits(), params);
        let request = TransitionRequest::resolve(scenario, kind, duration, params);
        let mut words = [0u32; 8];
        let mut path = String::new();
        let tag = match request {
            None => u32::MAX,
            Some(request) => {
                words[0] = request.duration.as_f32().to_bits();
                match request.kind {
                    TransitionKind::Default => 0,
                    TransitionKind::Mask {
                        path: p,
                        parameter,
                        flags,
                    } => {
                        path = p;
                        words[1] = parameter.0.to_bits();
                        words[2] = flags.bits() as u32;
                        1
                    }
                    TransitionKind::Scroll { incoming, outgoing } => {
                        words[1] = incoming as u32;
                        words[2] = outgoing as u32;
                        2
                    }
                    TransitionKind::Zoom { incoming, outgoing } => {
                        words[1] = (incoming as f32 * 0.001).to_bits();
                        words[2] = (outgoing as f32 * 0.001).to_bits();
                        3
                    }
                    TransitionKind::Turn(direction) => {
                        words[1] = direction as u32;
                        4
                    }
                    TransitionKind::Wave => {
                        words[1..5].copy_from_slice(&[0.5f32, 0.5, 1.0, 0.25].map(f32::to_bits));
                        5
                    }
                    TransitionKind::Scanline(exponent) => {
                        words[1] = (exponent as f32).to_bits();
                        6
                    }
                    TransitionKind::Ripple => {
                        words[1..7].copy_from_slice(
                            &[0.5f32, 0.5, 1.0, 0.25, 0.2, 60.0].map(f32::to_bits),
                        );
                        7
                    }
                    TransitionKind::Whirl(direction) => {
                        words[1] = direction as u32;
                        words[2] = 1.0f32.to_bits();
                        8
                    }
                    TransitionKind::Glass => 9,
                }
            }
        };
        let mut expected = b"GTR1".to_vec();
        expected.extend(tag.to_le_bytes());
        expected.extend(words.into_iter().flat_map(u32::to_le_bytes));
        expected.extend((path.len() as u32).to_le_bytes());
        expected.extend(path.as_bytes());
        self.compare(&expected);
        self.requests += 1;
    }
    fn clock(
        &mut self,
        op: u32,
        flags: u32,
        value: f32,
        state: &super::super::timed::TimedWiperState,
    ) {
        self.row(op, flags, value.to_bits(), [0; 8]);
        let mut expected = b"GTC1".to_vec();
        let [current, total] = state.reference_time_words();
        expected.extend(
            [
                current,
                total,
                state.is_running() as u32,
                state.get_progress().to_bits(),
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes),
        );
        self.compare(&expected);
        self.clocks += 1;
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        self.input.take();
        let status = self.child.wait().unwrap();
        if !std::thread::panicking() {
            assert!(status.success());
        }
    }
}

#[test]
#[ignore = "requires supplied ROM/executable/fonts and original transition oracle;CPU only"]
fn original_transition_factory_and_shared_clock_contract() {
    let mut scenario = scenario();
    let supplied = scenario.mask_info.len();
    for name in [String::new(), "仮面😀".repeat(80)] {
        scenario.mask_info.push(MaskInfoItem {
            name: U16Utf8String::new(name),
        });
    }
    let mut native = Native::new(&scenario);
    let values = [
        i32::MIN,
        -1001,
        -1,
        0,
        1,
        2,
        999,
        1000,
        1001,
        16_777_217,
        i32::MAX,
    ];
    for kind in [-1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, i32::MIN, i32::MAX] {
        for duration in [-1, 0, 1, 60, i32::MAX] {
            for a in values {
                let params = [a, a.wrapping_neg(), a.rotate_left(7), -7, 8, 9, 10, 11];
                native.request(&scenario, kind, duration, params);
            }
        }
    }
    for id in 0..scenario.mask_info.len() {
        native.request(&scenario, 1, 60, [id as i32, 500, -1, 0, 0, 0, 0, 0]);
    }
    for parameter in -32..=1032 {
        for flags in [i32::MIN, -1, 0, 1, 7, 15, 16, i32::MAX] {
            native.request(&scenario, 1, 60, [0, parameter, flags, 0, 0, 0, 0, 0]);
        }
    }
    for duration in [i32::MIN, -1, 0, 1, 2, 60, 1000, i32::MAX] {
        for flags in 0..256 {
            let mut state =
                super::super::timed::TimedWiperState::new(Ticks::from_f32(duration as f32));
            native.clock(1, 0, duration as f32, &state);
            native.clock(4, 0, 0.0, &state.clone());
            for delta in [0.0, -0.0, 0.125, 31.25, 100_000.0, -2.5, 60.0] {
                state.update(flags & 1 != 0, Ticks::from_f32(delta));
                native.clock(2, flags, delta, &state);
            }
            state.fast_forward();
            native.clock(3, 0, 0.0, &state);
            state.update(true, Ticks::from_f32(-1.0));
            native.clock(2, 1, -1.0, &state);
            native.clock(4, 0, 0.0, &state.clone());
        }
    }
    println!(
        "GEROKASU2_TRANSITION_NATIVE requests={} clocks={} bytes={} supplied_masks={supplied} original_complete_switch_UTF8_mask_bounds_paths_raw_flags_float_parameters_clock_ctor_copy_update_query_ff=verified",
        native.requests, native.clocks, native.bytes
    );
}

mod gpu;
