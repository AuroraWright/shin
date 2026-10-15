use super::tests::{hash, pattern, put};
use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[derive(Clone)]
struct RawPayload {
    core: Vec<u8>,
    adv: Vec<u8>,
    extra: Vec<u8>,
    cursor: usize,
}
fn vector_hash(lengths: [usize; 2]) -> u64 {
    let mut bytes = Vec::new();
    for (length, step) in [(lengths[0], 17), (lengths[1], 19), (6, 23), (11, 29)] {
        bytes.extend((length as u32).to_le_bytes());
        bytes.extend((0..length).map(|i| (i * step + length) as u8));
    }
    hash(&bytes)
}
#[test]
#[ignore = "original1104c/109bc clone and112cc lookup/restore/pruning with complete native copies and GameData vectors"]
fn higurashi_checkpoint_forks_match_native_clone_lookup_pruning_and_source_ownership() {
    let patterns: &[&[u32]] = &[
        &[],
        &[0],
        &[0, 200],
        &[0, 200, 400, 600, 800],
        &[0, 2000, 2000, 2000, 2000],
        &[u32::MAX, u32::MAX, u32::MAX, u32::MAX],
        &[0, u32::MAX, 600, 800, 1000],
        &[400, 0, 600, 600, u32::MAX],
    ];
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut profiles = 0;
    for active in 0..2 {
        for seed in [0u32, 42, u32::MAX] {
            for lengths in [[0, 0], [1, 7], [1024, 2048], [4096, 1023]] {
                for &boundaries in patterns {
                    profiles += 1;
                    input.push_str(&format!(
                        "N {active} {seed:x} {} {}\n",
                        lengths[0], lengths[1]
                    ));
                    let mut archive = SparseCheckpoints::new();
                    for (i, &boundary) in boundaries.iter().enumerate() {
                        let seed = seed.wrapping_add(i as u32);
                        let extra = [0, 1, 3, 16][i % 4];
                        // Native335d4 is an unchecked cursor assignment. A
                        // restored position may exceed the current vector.
                        let cursor = [0, 1, 8193, 32768][i % 4];
                        input.push_str(&format!("C {boundary:x} {seed:x} {extra} {cursor}\n"));
                        archive
                            .capture_if_needed(boundary, || {
                                let mut core = pattern(0x2518, seed);
                                let mut adv = pattern(0x5ed0, seed ^ 0x9e3779b9);
                                put(&mut core, 0, 0x55667788);
                                put(&mut core, 4, seed);
                                put(&mut core, 8, 0x55667788);
                                put(&mut core, 12, seed);
                                put(&mut adv, 0, boundary);
                                Ok(RawPayload {
                                    core,
                                    adv,
                                    extra: pattern(extra * 4, seed ^ 0x13579bdf),
                                    cursor,
                                })
                            })
                            .unwrap();
                    }
                    let source: Vec<_> = archive
                        .entries()
                        .iter()
                        .map(|c| (c.boundary, hash(&c.state.core), hash(&c.state.adv)))
                        .collect();
                    let targets = [
                        0,
                        1,
                        199,
                        200,
                        201,
                        399,
                        400,
                        401,
                        600,
                        800,
                        1000,
                        2000,
                        u32::MAX - 1,
                        u32::MAX,
                    ];
                    for saved in targets.into_iter().chain(boundaries.iter().copied()) {
                        let replay = saved.wrapping_mul(17).wrapping_add(seed);
                        input.push_str(&format!("F {saved:x} {replay:x}\n"));
                        let fork = archive.fork_for_restore(saved);
                        let retained: Vec<_> =
                            fork.archive.entries().iter().map(|c| c.boundary).collect();
                        expected.push((
                            active,
                            saved,
                            replay,
                            fork.restored,
                            retained,
                            vector_hash(lengths),
                        ));
                        assert_eq!(
                            source,
                            archive
                                .entries()
                                .iter()
                                .map(|c| (c.boundary, hash(&c.state.core), hash(&c.state.adv)))
                                .collect::<Vec<_>>()
                        );
                    }
                }
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_RESTORATION").unwrap())
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
    for (row, (actual, (active, saved, replay, restored, retained, vectors))) in
        output.lines().zip(&expected).enumerate()
    {
        let fields: Vec<_> = actual.split_whitespace().collect();
        assert_eq!(fields[0], "F");
        let decimal = |i: usize| fields[i].parse::<u64>().unwrap();
        let hex = |i: usize| u64::from_str_radix(fields[i], 16).unwrap();
        assert_eq!(
            (decimal(1), decimal(2), decimal(3)),
            (*active as u64, *saved as u64, *replay as u64),
            "pair row{row}"
        );
        assert_eq!(decimal(5), retained.len() as u64, "retained row{row}");
        assert_eq!(hex(11), *vectors, "GameData vectors row{row}");
        assert_eq!(
            fields[12..]
                .iter()
                .map(|v| v.parse::<u32>().unwrap())
                .collect::<Vec<_>>(),
            *retained
        );
        if let Some(entry) = restored {
            let p = &entry.state;
            assert_eq!(
                (decimal(4), hex(6), hex(7), decimal(8), hex(9), decimal(10)),
                (
                    entry.boundary as u64,
                    hash(&p.core),
                    hash(&p.adv),
                    (p.extra.len() / 4) as u64,
                    hash(&p.extra),
                    p.cursor as u64
                ),
                "payload row{row}"
            );
        } else {
            assert_eq!(
                (decimal(4), decimal(8), hex(9), decimal(10)),
                (0, 0, hash(&[]), 0),
                "initial state row{row}"
            );
        }
    }
    println!(
        "Higurashi checkpoint restoration:{profiles} profiles/{} native forks; original1104c/109bc/10cec initialization and deep GameData copy, newest exclusion, unsigned strict predecessor, duplicate/wrapping archives, full2518/5ed0/extra/cursor restore, inclusive pruning, source immutability and original native destruction match",
        expected.len()
    );
}
