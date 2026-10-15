use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn snapshot(op: char, added: bool, history: &MessageReadHistory, id: u32) -> String {
    let mut words = vec![0u32; history.bits().size_in_words()];
    for id in history.bits().iter() {
        words[id >> 5] |= 1 << (id & 31);
    }
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for word in &words {
        for byte in word.to_le_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    }
    format!(
        "{op} {} {} {hash:016x} {}",
        u8::from(added),
        words.len(),
        u8::from(history.contains(id))
    )
}

#[test]
#[ignore = "executes original updated completion callsites/listener and full native bitset growth"]
fn higurashi_read_history_matches_native_completion_listener_and_shared_bitset() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for fragment in 0..5 {
        for vm in [false, true] {
            for listener in [false, true] {
                for id in [0, 1, 31, 32, 255, 256, 8191, 8192, 0x8000_0000, u32::MAX] {
                    for modes in [0, 1, 2, 3, 0x103] {
                        for flags in [0, 0x8000_0000, u32::MAX] {
                            for read in [0, 1, 255] {
                                for blocker in [0, 1] {
                                    profiles += 1;
                                    let mut history = MessageReadHistory::new();
                                    input.push_str("N\n");
                                    expected.push(snapshot('N', false, &history, 0));
                                    for _ in 0..2 {
                                        input.push_str(&format!("C {fragment} {} {} {id:x} {modes:x} {flags:x} {read} {blocker}\n", u8::from(vm), u8::from(listener)));
                                        history.record_completion(vm && listener, id);
                                        // Listener's ABI is void: ignored paths do not have a boolean return.
                                        expected.push(snapshot('C', false, &history, id));
                                    }
                                    input.push_str(&format!("Q {id:x}\n"));
                                    expected.push(snapshot('Q', false, &history, id));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Actual growth/copy/zero-extension and duplicate-return behavior, all32
    // bits, logical256-word boundaries, nonmonotonic insertion and packed-ID max.
    for fragment in 0..5 {
        input.push_str("N\n");
        let mut history = MessageReadHistory::new();
        expected.push(snapshot('N', false, &history, 0));
        let ids = (0..64).chain([
            8191,
            8192,
            16383,
            16384,
            32768,
            3,
            0x00ff_fffe,
            0x00ff_fffe,
            31,
        ]);
        for id in ids {
            input.push_str(&format!("C {fragment} 1 1 {id:x} 2 0 0 1\n"));
            history.record_completion(true, id);
            expected.push(snapshot('C', false, &history, id));
            input.push_str(&format!("A {id:x}\n"));
            let added = history.record_completion(true, id);
            expected.push(snapshot('A', added, &history, id));
        }
        for id in [65, 8190, 8193, 16382, 32769, 0x00ff_ffff, u32::MAX] {
            input.push_str(&format!("Q {id:x}\n"));
            expected.push(snapshot('Q', false, &history, id));
        }
        input.push_str("A 41\n");
        let added = history.record_completion(true, 65);
        expected.push(snapshot('A', added, &history, 65));
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_READ_HISTORY").unwrap())
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
    for (row, (line, expected)) in output.lines().zip(&expected).enumerate() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        assert_eq!(fields.len(), 8);
        assert_eq!(fields[..5].join(" "), *expected, "read-history row{row}");
        let words: usize = fields[2].parse().unwrap();
        let allocated: u32 = fields[5].parse().unwrap();
        let freed: u32 = fields[6].parse().unwrap();
        let copied: u32 = fields[7].parse().unwrap();
        assert_eq!(allocated - freed, u32::from(words != 0));
        assert_eq!(copied, freed);
    }
    println!(
        "Higurashi read history:{profiles} native listener profiles/{} snapshots match all5 original callsites, VM/listener/negative-ID gates, unchanged cached VM read byte, shared256-word growth, zero-extension/copy and duplicate returns",
        expected.len()
    );
}
