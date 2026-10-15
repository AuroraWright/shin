use super::*;
use std::{
    cell::RefCell,
    io::Write,
    process::{Command, Stdio},
};

pub(super) fn context(state: u32, vm: ReadState, value: f32, allow: u32) -> Context {
    let bit = |i: u32| state & (1u32 << i) != 0;
    Context {
        permission: bit(0),
        captured: bit(1),
        disabled: bit(2),
        system_locked: bit(3),
        vm: bit(4).then_some(vm),
        paused: bit(5),
        continuation_blocked: bit(6),
        overlay_transition_busy: bit(7),
        notification_busy: bit(8),
        animations_enabled: bit(9),
        overlay: bit(10).then_some(OverlayInput {
            menus: bit(11),
            hide_window: bit(12),
        }),
        hint_busy: bit(13),
        scene_jump_available: bit(14),
        wait_allow_interrupt: allow,
        overlay_transition_value: value,
    }
}

struct Reference {
    events: RefCell<String>,
    click: bool,
    interested: bool,
    wait: f32,
}
impl Services for Reference {
    fn request(&mut self, request: Request) {
        use Request::*;
        self.events.get_mut().push(match request {
            FlushInput => 'F',
            CueEnabled => 'E',
            CueDisabled => 'D',
            CueSceneJump => 'J',
            NextChapter => 'N',
            PreviousChapter => 'P',
            HintPulse => 'T',
            SceneJump => 'G',
            SystemMenu => 'S',
            MessageBrowser => 'B',
            PreviousMessage => 'R',
            HideWindow => 'H',
        });
    }
    fn click_message(&mut self) -> bool {
        self.events.get_mut().push('C');
        self.click
    }
    fn message_interested(&self) -> bool {
        self.events.borrow_mut().push('I');
        self.interested
    }
    fn interrupt_wait(&mut self) {
        self.wait = 0.0;
    }
}

#[test]
#[ignore = "executes updated native input/read branches against prepared service contracts"]
fn higurashi_input_and_read_decisions_match_updated_executable() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut add = |bits: u32,
                   modes,
                   pressed,
                   repeat,
                   vm: ReadState,
                   value: f32,
                   allow,
                   forced,
                   read_only,
                   held,
                   rapid| {
        let mut state = InputState {
            modes,
            temporary_indicator: 9,
        };
        let mut services = Reference {
            events: RefCell::default(),
            click: bits & (1 << 15) != 0,
            interested: bits & (1 << 16) != 0,
            wait: 12.0,
        };
        let c = context(bits, vm, value, allow);
        let enabled = process(
            &mut state,
            c,
            Buttons {
                held,
                pressed,
                repeat,
                rapid_repeat: rapid,
            },
            &mut services,
        );
        let suppression = suppress_transient_audio(c.vm, state.modes, forced, read_only);
        let requests = message_requests(c.vm, state.modes, forced, read_only);
        input.push_str(&format!("I {bits:x} {modes:x} {pressed:x} {repeat:x} {:x} {} {} {allow} 9 {held:x} {rapid:x} {:x} {} {}\n",
            vm.flags,vm.fast_forward_blocked,vm.is_read,value.to_bits(),u8::from(forced),u8::from(read_only)));
        let events = services.events.into_inner();
        expected.push(format!(
            "I {} {:08x} {} {:08x} {} {} {} {}",
            u8::from(enabled),
            state.modes,
            state.temporary_indicator,
            services.wait.to_bits(),
            if events.is_empty() { "-" } else { &events },
            u8::from(suppression),
            u8::from(requests.autoplay),
            u8::from(requests.fast_forward)
        ));
    };
    let base = 0x211;
    let mut profiles = vec![
        base,
        base | (1 << 15),
        base | (1 << 16),
        base | (1 << 15) | (1 << 16),
        base | (1 << 14),
        base | (1 << 13),
    ];
    for bit in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9] {
        profiles.push(base ^ (1 << bit));
    }
    for overlay in 0..4 {
        profiles.push(base | (1 << 10) | (overlay << 11));
    }
    let mut presses = vec![0, u32::MAX];
    for bit in 0..32 {
        presses.push(1 << bit);
    }
    for pair in [
        (10, 11),
        (10, 12),
        (11, 25),
        (9, 24),
        (24, 25),
        (6, 25),
        (15, 12),
        (15, 25),
        (6, 12),
    ] {
        presses.push((1 << pair.0) | (1 << pair.1));
    }
    for bits in profiles {
        for modes in [0, 1, 2, 3, 0x8000_0000] {
            for flags in [0, 0x8000_0000] {
                for &pressed in &presses {
                    for repeat in [0, 1 << 4, 1 << 8, (1 << 4) | (1 << 8)] {
                        add(
                            bits,
                            modes,
                            pressed,
                            repeat,
                            ReadState {
                                flags,
                                fast_forward_blocked: 0,
                                is_read: 0,
                            },
                            0.0,
                            1,
                            false,
                            true,
                            0,
                            u32::MAX,
                        );
                    }
                }
            }
        }
    }
    // Full native suppression/autoplay eligibility including nonboolean bytes,
    // upper mode bits and VM absence. No input can mutate these profiles.
    for present in [false, true] {
        for flags in [0, 1, 0x8000_0000, u32::MAX] {
            for blocked in [0, 1, 2, 255] {
                for read in [0, 1, 2, 255] {
                    for modes in [0, 1, 2, 3, 0x100, 0x103] {
                        for forced in [false, true] {
                            for read_only in [false, true] {
                                let bits = 0x201 | if present { 1 << 4 } else { 0 };
                                add(
                                    bits,
                                    modes,
                                    0,
                                    0,
                                    ReadState {
                                        flags,
                                        fast_forward_blocked: blocked,
                                        is_read: read,
                                    },
                                    0.0,
                                    2,
                                    forced,
                                    read_only,
                                    u32::MAX,
                                    0,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    for value in [-1.0, 0.0, 1.0, f32::NAN] {
        for allow in [0, 1, 2, u32::MAX] {
            for pressed in [1 << 6, 1 << 24, (1 << 6) | (1 << 24)] {
                add(
                    base,
                    0,
                    pressed,
                    0,
                    ReadState {
                        flags: 0x8000_0000,
                        fast_forward_blocked: 0,
                        is_read: 1,
                    },
                    value,
                    allow,
                    false,
                    false,
                    0,
                    0,
                );
            }
        }
    }
    drop(add);
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_INPUT").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.lines().count(), expected.len());
    for (row, (actual, expected)) in output.lines().zip(&expected).enumerate() {
        assert_eq!(actual, expected, "native input row{row}");
    }
    println!(
        "Higurashi input:{} native profiles match ordered toggles/cancellation, modal/transition gates, chapter/history/menu/hidden-window requests, message-first confirm, exact WAIT interrupt, transient suppression and post-poll autoplay/skip",
        expected.len()
    );
}
