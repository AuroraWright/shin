use super::*;

fn targets(allocator: &mut LayerbankAllocator, plane: u8, from: u16, to: u16) -> Vec<(u16, u8)> {
    allocator
        .layers_in_range(PlaneId::new(plane), LayerId::new(from), LayerId::new(to))
        .iter()
        .map(|target| (target.layer.raw(), target.layerbank.raw()))
        .collect()
}

#[test]
fn singleton_callback_returns_once_without_overwriting_the_range_cache() {
    let mut allocator = LayerbankAllocator::new();
    allocator
        .alloc_layerbank(PlaneId::new(0), LayerId::new(3))
        .unwrap();
    assert_eq!(targets(&mut allocator, 0, 0, 8), [(3, 0)]);
    assert!(
        allocator
            .range_cache
            .is_hit(PlaneId::new(0), LayerId::new(0), LayerId::new(8))
    );
    assert_eq!(targets(&mut allocator, 0, 3, 3), [(3, 0)]);
    assert!(targets(&mut allocator, 1, 3, 3).is_empty());
    assert!(
        allocator
            .range_cache
            .is_hit(PlaneId::new(0), LayerId::new(0), LayerId::new(8))
    );
}

#[test]
fn range_order_matches_native_density_threshold_and_filters_other_planes() {
    let mut allocator = LayerbankAllocator::new();
    allocator
        .alloc_layerbank(PlaneId::new(0), LayerId::new(9))
        .unwrap();
    allocator
        .alloc_layerbank(PlaneId::new(0), LayerId::new(8))
        .unwrap();
    // allocated2 > span1: ascending logical IDs, not allocation order.
    assert_eq!(targets(&mut allocator, 0, 8, 9), [(8, 1), (9, 0)]);
    // allocated2 <= span2: ascending physical banks, not logical IDs.
    assert_eq!(targets(&mut allocator, 0, 7, 9), [(9, 0), (8, 1)]);
    allocator
        .alloc_layerbank(PlaneId::new(1), LayerId::new(8))
        .unwrap();
    // The threshold counts allocations in all planes, as Native20310 does.
    assert_eq!(targets(&mut allocator, 0, 7, 9), [(8, 1), (9, 0)]);
    assert_eq!(targets(&mut allocator, 1, 0, 255), [(8, 2)]);
}

#[test]
fn cached_ranges_follow_free_reuse_swap_empty_and_maximum_layer_ids() {
    let mut allocator = LayerbankAllocator::new();
    for layer in [9, 8, 7] {
        allocator
            .alloc_layerbank(PlaneId::new(0), LayerId::new(layer))
            .unwrap();
    }
    assert_eq!(targets(&mut allocator, 0, 0, 255), [(9, 0), (8, 1), (7, 2)]);
    assert_eq!(targets(&mut allocator, 0, 0, 255), [(9, 0), (8, 1), (7, 2)]);
    allocator.free_layerbank(PlaneId::new(0), LayerId::new(9));
    assert_eq!(targets(&mut allocator, 0, 0, 255), [(8, 1), (7, 2)]);
    allocator
        .alloc_layerbank(PlaneId::new(0), LayerId::new(6))
        .unwrap();
    assert_eq!(targets(&mut allocator, 0, 0, 255), [(6, 0), (8, 1), (7, 2)]);
    allocator.swap_layerbanks(PlaneId::new(0), LayerId::new(6), LayerId::new(8));
    assert_eq!(targets(&mut allocator, 0, 0, 255), [(8, 0), (6, 1), (7, 2)]);
    assert!(targets(&mut allocator, 0, 30, 4).is_empty());
    allocator
        .alloc_layerbank(PlaneId::new(0), LayerId::new(255))
        .unwrap();
    assert_eq!(targets(&mut allocator, 0, 254, 255), [(255, 3)]);
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_layer_range_singleton_order_and_cache_publication() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let executable = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0x2028c, 0x6b020068),
        (0x20290, 0x54000261),
        (0x202d0, 0xd63f0100),
        (0x202d4, 0x320003e0),
        (0x202d8, 0x1400005d),
        (0x20314, 0x6b08013f),
        (0x20318, 0x5400030d),
        (0x2033c, 0x3868694b),
        (0x20368, 0x54fffeab),
        (0x203c4, 0x39140928),
        (0x203f4, 0xb904f681),
        (0x203f8, 0xb904fa82),
        (0x203fc, 0xb904fe83),
        (0x20448, 0x54fffe4b),
    ] {
        assert_eq!(
            u32::from_le_bytes(executable[address + 256..address + 260].try_into().unwrap()),
            expected,
            "Native range lookup instruction {address:x}"
        );
    }
    // The singleton callback branches straight to the common epilogue,
    // bypassing the range-cache lookup/build and callback iteration.
    assert_eq!(0x202d8 + (0x1400005du32 & 0x03ffffff) * 4, 0x2044c);
}

#[cfg(target_arch = "aarch64")]
#[test]
#[ignore = "requires supplied executable and compiled SHIN_REFERENCE_LAYER_ORACLE (native-layer-range.c)"]
fn reference_native_executed_allocator_and_range_trace_matches_rust() {
    use std::{fmt::Write as _, io::Write as _};
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let executable = std::path::Path::new(&rom)
        .parent()
        .unwrap()
        .join("uncompressed_executable");
    let oracle = std::env::var("SHIN_REFERENCE_LAYER_ORACLE").unwrap();
    let mut operations = vec![
        ('r', 0, 0, 255),
        ('a', 0, 9, 0),
        ('a', 0, 8, 0),
        ('r', 0, 7, 9),
        ('r', 0, 7, 9),
        ('r', 0, 8, 9),
        ('r', 0, 9, 9),
        ('r', 1, 9, 9),
        ('a', 1, 8, 0),
        ('r', 0, 7, 9),
        ('r', 1, 0, 255),
        ('s', 0, 8, 9),
        ('r', 0, 7, 9),
        ('f', 0, 9, 0),
        ('a', 0, 255, 0),
        ('r', 0, 254, 255),
        ('r', 0, 30, 4),
        ('r', 0, 255, 255),
    ];
    // Fill all48 banks, try overflow and duplicate allocation, then free and
    // reallocate through the original stack of free banks.
    for plane in 0..4 {
        for layer in 0..16 {
            operations.push(('a', plane, layer, 0));
        }
    }
    for plane in 0..4 {
        operations.push(('r', plane, 0, 255));
    }
    for plane in 0..4 {
        for layer in 0..16 {
            operations.push(('f', plane, layer, 0));
            operations.push(('r', plane, 0, 255));
        }
    }
    // Deterministic mixed operations exercise cache hits/misses, singleton,
    // reversed ranges, swaps with missing layers and reuse across planes.
    let mut random = 0x85f927b0a36e421du64;
    for _ in 0..2000 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let plane = ((random >> 32) % 4) as u8;
        let from = ((random >> 16) % 32) as u16;
        let to = ((random >> 24) % 32) as u16;
        let op = match random % 5 {
            0 => 'a',
            1 => 'f',
            2 => 's',
            _ => 'r',
        };
        operations.push((op, plane, from, to));
        if op == 'r' {
            operations.push((op, plane, from, to));
            operations.push(('r', plane, from, from));
            operations.push(('r', plane, 0, 255));
        }
    }
    let mut allocator = LayerbankAllocator::new();
    let mut input = String::new();
    let mut expected = Vec::new();
    for &(op, plane, from, to) in &operations {
        if op == 'a' || op == 'f' {
            writeln!(input, "{op} {plane} {from}").unwrap();
        } else {
            writeln!(input, "{op} {plane} {from} {to}").unwrap();
        }
        let p = PlaneId::new(plane);
        let f = LayerId::new(from);
        let mut line = String::new();
        match op {
            'a' => {
                let bank = allocator
                    .alloc_layerbank(p, f)
                    .map(|b| i32::from(b.raw()))
                    .unwrap_or(-1);
                write!(line, "a {bank}").unwrap();
            }
            'f' => {
                allocator.free_layerbank(p, f);
                line.push('f');
            }
            's' => {
                allocator.swap_layerbanks(p, f, LayerId::new(to));
                line.push('s');
            }
            'r' => {
                let actual = targets(&mut allocator, plane, from, to);
                let cache = &allocator.range_cache;
                let cache_plane = cache.plane.into_option().map(|p| p.raw()).unwrap_or(4);
                write!(
                    line,
                    "r {} {cache_plane} {} {} {}",
                    actual.len(),
                    cache.from.raw(),
                    cache.to.raw(),
                    cache.affected_layerbank_count
                )
                .unwrap();
                for (layer, bank) in actual {
                    write!(line, " {layer}:{bank}").unwrap();
                }
            }
            _ => unreachable!(),
        }
        expected.push(line);
    }
    // Use a file for stdin so neither the large input nor captured output can
    // deadlock on a full child pipe. The native process owns no external state.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let input_path = std::env::temp_dir().join(format!(
        "shin-layer-oracle-{}-{nonce}.txt",
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&input_path)
        .unwrap();
    file.write_all(input.as_bytes()).unwrap();
    drop(file);
    let output = std::process::Command::new(oracle)
        .arg(executable)
        .stdin(std::fs::File::open(&input_path).unwrap())
        .output()
        .unwrap();
    std::fs::remove_file(input_path).unwrap();
    assert!(
        output.status.success(),
        "Native oracle: {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let actual: Vec<_> = stdout.lines().collect();
    assert_eq!(actual.len(), expected.len());
    for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
        assert_eq!(
            *actual, expected,
            "Native/Rust operation{index}: {:?}",
            operations[index]
        );
    }
    eprintln!(
        "Independent native allocator/range: {} operations match Rust, including callback order and retained cache keys/count",
        operations.len()
    );
}
