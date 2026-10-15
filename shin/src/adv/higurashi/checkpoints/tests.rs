use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub(super) fn pattern(bytes: usize, mut seed: u32) -> Vec<u8> {
    (0..bytes / 4)
        .flat_map(|_| {
            seed = seed.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
            seed.to_le_bytes()
        })
        .collect()
}
pub(super) fn put(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
pub(super) fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |v, b| {
        (v ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
    })
}
struct Payload {
    core: Vec<u8>,
    adv: Vec<u8>,
    extra: Vec<u8>,
    route: u32,
}

#[test]
#[ignore = "original complete1185c memory capture and bounded native restore copies"]
fn higurashi_sparse_checkpoints_match_native_full_copies_commit_order_and_restore() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut captures = 0;
    for enabled in [false, true] {
        for extra_n in [0, 1, 3, 16] {
            for np in [0, 1, 8] {
                for nr in [0, 1, 8] {
                    for seed in [0, 42, u32::MAX] {
                        input.push_str("N\n");
                        let mut archive = SparseCheckpoints::new();
                        for (i, boundary) in
                            [0, 1, 199, 200, 201, 399, 400, 0, 2000, 2000, u32::MAX]
                                .into_iter()
                                .enumerate()
                        {
                            let seed = seed.wrapping_add(i as u32);
                            input.push_str(&format!(
                                "C {boundary:x} {seed:x} {extra_n} {} {np} {nr}\n",
                                u8::from(enabled)
                            ));
                            let added = archive
                                .capture_if_needed(boundary, || {
                                    let mut core = pattern(0x2518, seed);
                                    let mut adv = pattern(0x5ed0, seed ^ 0x9e3779b9);
                                    // Snapshot119bc stores original instruction PC/pre-PRNG in first8 bytes.
                                    put(&mut core, 0, 0x55667788);
                                    put(&mut core, 4, seed);
                                    put(&mut core, 8, 0x55667788);
                                    put(&mut core, 12, seed);
                                    put(&mut adv, 0, boundary);
                                    Ok(Payload {
                                        core,
                                        adv,
                                        extra: pattern(extra_n * 4, seed ^ 0x13579bdf),
                                        route: seed & 0x7fff,
                                    })
                                })
                                .unwrap();
                            captures += usize::from(added);
                            let p = &archive.entries().last().unwrap().state;
                            expected.push(format!(
                                "C {} {:016x} {:016x} {:016x} {} {} {} {} {}",
                                archive.entries().len(),
                                hash(&p.core),
                                hash(&p.adv),
                                hash(&p.extra),
                                p.route,
                                if enabled { np } else { 0 },
                                if enabled { nr } else { 0 },
                                if enabled { 0 } else { np },
                                if enabled { 0 } else { nr }
                            ));
                        }
                        // A rejected payload never changes existing immutable entries.
                        let count = archive.entries().len();
                        assert!(
                            archive
                                .capture_if_needed(u32::MAX, || anyhow::bail!(
                                    "snapshot capture unavailable"
                                ))
                                .is_err()
                        );
                        assert_eq!(archive.entries().len(), count);
                        assert!(
                            !archive
                                .capture_if_needed(0, || panic!("existing bucket recaptured"))
                                .unwrap()
                        );
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_CHECKPOINT").unwrap())
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
        assert_eq!(actual, expected, "sparse checkpoint row{row}");
    }
    println!(
        "Higurashi sparse checkpoints:{} native calls/{captures} captures match full2518/5ed0/vector/route copies and bounded restore; enabled/disabled commit queues,200-boundary buckets/gaps/wrap, immutable history and capture failure pass",
        expected.len()
    );
}
