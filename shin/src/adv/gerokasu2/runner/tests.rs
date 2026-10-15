use super::*;
use std::{
    io::{Read, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

struct Native {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    snapshots: usize,
    words: usize,
}
impl Native {
    fn new() -> Self {
        let mut child = Command::new(std::env::var_os("SHIN_GEROKASU2_RUNNER_ORACLE").unwrap())
            .args([
                std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_NORMAL_FONT").unwrap(),
                std::env::var_os("SHIN_GEROKASU2_BOLD_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
            snapshots: 0,
            words: 0,
        }
    }
    fn send(&mut self, words: &[u32]) {
        let input = self.input.as_mut().unwrap();
        for n in words {
            input.write_all(&n.to_le_bytes()).unwrap();
        }
        input.flush().unwrap();
    }
    fn read(&mut self) -> ([u32; 7], Vec<u32>) {
        let mut magic = [0; 4];
        self.output.read_exact(&mut magic).unwrap();
        assert_eq!(&magic, b"GRF1");
        let mut get = || {
            let mut bytes = [0; 4];
            self.output.read_exact(&mut bytes).unwrap();
            u32::from_le_bytes(bytes)
        };
        let header: [u32; 8] = std::array::from_fn(|_| get());
        let events = (0..header[7]).map(|_| get()).collect::<Vec<_>>();
        self.snapshots += 1;
        self.words += 8 + events.len();
        (header[..7].try_into().unwrap(), events)
    }
    fn finish(mut self) {
        self.input.take();
        assert!(self.child.wait().unwrap().success());
        println!(
            "GEROKASU2_RUNNER_NATIVE snapshots={} words={} whole_ADV_frame_WAIT_poll_clock_root_voice_order_gates=verified",
            self.snapshots, self.words
        );
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn native_wait_and_complete_frame_command_clock_root_voice_order() {
    if std::env::var_os("SHIN_GEROKASU2_RUNNER_ORACLE").is_none() {
        return;
    }
    let mut native = Native::new();
    for amount in [i32::MIN, -12, 0, 1, 12, i32::MAX] {
        for allow in [0, 1, 2, 255] {
            for pattern in 0..8 {
                native.send(&[0, 2, 1, 10, allow, amount as u32, 0, 0, 2, 11, 0, 0, 0, 0]);
                let (header, events) = native.read();
                assert_eq!(header, [0, 0, 0, 0, 0, u32::MAX, 0]);
                assert!(events.is_empty());
                let mut clock = WaitClock::default();
                let mut started = false;
                let mut retired = false;
                let mut fetched = 0;
                let mut elapsed = 0.0f32;
                for step in 0..24 {
                    if step == 7 {
                        native.send(&[2]);
                        clock.interrupt();
                        let (h, e) = native.read();
                        assert_eq!(h[0], u32::from(clock.allow));
                        assert_eq!(h[1], clock.remaining.to_bits());
                        assert!(e.is_empty());
                    }
                    let paused = pattern & 1 != 0 && step % 3 == 0;
                    let changing = pattern & 2 != 0 && step % 4 == 1;
                    let animate = pattern & 4 == 0 || step % 2 == 0;
                    let skip = step >= 18;
                    let delta: f32 = [0.0, 0.25, 1.0, 6.0, -0.5][step % 5];
                    native.send(&[
                        1,
                        delta.to_bits(),
                        u32::from(paused),
                        u32::from(animate),
                        u32::from(changing),
                        0,
                        u32::from(step % 2 == 0),
                        u32::from(skip),
                    ]);
                    let mut expected = Vec::new();
                    if !paused && !changing {
                        expected.extend([1, fetched]);
                        if !started {
                            clock.install(amount, allow as u8);
                            started = true;
                            fetched = 1;
                            expected.extend([2, 10]);
                        }
                        if !retired {
                            expected.extend([3, 10]);
                            if clock.poll(skip) {
                                retired = true;
                                expected.extend([4, 10, 1, fetched, 2, 11, 3, 11]);
                                fetched = 2;
                            }
                        } else {
                            expected.extend([3, 11]);
                        }
                        if skip && animate {
                            expected.extend([7, clock.remaining.to_bits()]);
                            clock.clear_time();
                        }
                        clock.advance(Ticks::from_f32(delta));
                    }
                    if !paused {
                        expected.extend([5, clock.remaining.to_bits(), 6, u32::from(animate)]);
                    }
                    expected.extend([8, clock.remaining.to_bits()]);
                    elapsed = delta.mul_add(f32::from_bits(0x3c888889), elapsed);
                    let (header, events) = native.read();
                    assert_eq!(
                        header,
                        [
                            u32::from(clock.allow),
                            clock.remaining.to_bits(),
                            elapsed.to_bits(),
                            fetched,
                            u32::from(retired),
                            if !started {
                                u32::MAX
                            } else if retired {
                                11
                            } else {
                                10
                            },
                            u32::from(started && !retired)
                        ],
                        "amount{amount} allow{allow} pattern{pattern} step{step}"
                    );
                    assert_eq!(
                        events, expected,
                        "original full-frame ordering amount{amount} allow{allow} pattern{pattern} step{step}"
                    );
                }
            }
        }
    }
    // A ready command chain retires before the blocking sentinel in one frame.
    native.send(&[0, 3, 0, 1, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 2, 3, 0, 0, 0, 0]);
    native.read();
    native.send(&[1, 1.0f32.to_bits(), 0, 1, 0, 0, 0, 0]);
    let (header, events) = native.read();
    assert_eq!(header[3..6], [3, 2, 3]);
    assert_eq!(
        events[..22],
        [
            1, 0, 2, 1, 3, 1, 4, 1, 1, 1, 2, 2, 3, 2, 4, 2, 1, 2, 2, 3, 3, 3
        ]
    );
    native.finish();
}

#[cfg(test)]
mod gpu;
