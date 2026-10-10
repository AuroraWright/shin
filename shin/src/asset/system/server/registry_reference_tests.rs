use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use crate::asset::system::locate::try_assets_directory;

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |value, byte| {
        (value ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[test]
#[ignore = "requires output from validate-native-resource-registry.py; no GPU or audio"]
fn reference_native_resource_registry_selection_and_paths() {
    let output = PathBuf::from(std::env::var_os("SHIN_NATIVE_REGISTRY_OUTPUT_DIR").unwrap());
    let queries = std::fs::read_to_string(output.join("queries.txt")).unwrap();
    let queries: Vec<_> = queries.lines().collect();
    let cases = std::fs::read_to_string(output.join("cases.tsv")).unwrap();
    let mut case_count = 0;
    let mut total = 0;
    for row in cases.lines() {
        let case: Vec<_> = row.split('\t').collect();
        assert_eq!(case.len(), 4);
        let name = case[0];
        let expected_mounts: usize = case[3].parse().unwrap();
        // Use the production directory probe, including its rejection of bad
        // ROM headers. locate_assets would search unrelated directories when
        // this isolated directory has no usable archives.
        let io = try_assets_directory(Path::new(case[1])).unwrap();
        assert_eq!(
            io.as_ref().map_or(0, |io| io.io.len()),
            expected_mounts,
            "{name}: mounted archives"
        );
        let trace = std::fs::read_to_string(case[2]).unwrap();
        let lines: Vec<_> = trace.lines().collect();
        assert_eq!(lines.len(), queries.len() + 2, "{name}");
        assert!(lines[0].starts_with(&format!("MOUNT {expected_mounts} ")));
        assert!(lines.last().unwrap().ends_with(" balanced"));
        for (query, line) in queries.iter().zip(&lines[1..lines.len() - 1]) {
            let native: Vec<_> = line.split_whitespace().collect();
            assert_eq!(native.len(), 6, "{name}: {query}");
            assert_eq!(native[0], "Q");
            let found = io.as_ref().map(|io| io.read_file(query));
            if native[1] == "0" {
                assert!(
                    found.is_none_or(|found| found.is_err()),
                    "{name}: unexpected asset {query}"
                );
            } else {
                assert_eq!(native[1], "1");
                let data = found
                    .unwrap()
                    .unwrap_or_else(|err| panic!("{name}: missing asset {query}: {err:#}"));
                let length: u64 = native[4].parse().unwrap();
                let mut cursor = data.cursor();
                assert_eq!(
                    cursor.seek(SeekFrom::End(0)).unwrap(),
                    length,
                    "{name}: {query}"
                );
                cursor.seek(SeekFrom::Start(0)).unwrap();
                let mut bytes = vec![0; length.min(4096) as usize];
                cursor.read_exact(&mut bytes).unwrap();
                assert_eq!(
                    fnv(&bytes),
                    u64::from_str_radix(native[5], 16).unwrap(),
                    "{name}: selected payload {query}"
                );
            }
            total += 1;
        }
        case_count += 1;
        println!(
            "Native registry comparison {name}: {} paths, {expected_mounts} archives",
            queries.len()
        );
    }
    assert_eq!(case_count, 9);
    assert_eq!(queries.len(), 16_417);
    println!("Native registry selection/path comparisons: {total}; no GPU/audio or save writes");
}
