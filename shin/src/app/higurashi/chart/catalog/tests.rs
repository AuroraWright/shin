use super::*;
use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
use std::{io::Read, path::PathBuf};

#[test]
fn higurashi_chart_catalog_matches_actual_updated_scenario_decoder() {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let assets = std::env::var_os("SHIN_HIGURASHI_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| project.join("higurashi"));
    if !assets.join("patch.rom").is_file() {
        eprintln!("skipping: Higurashi patch.rom is not available");
        return;
    }
    let mut archive = RomReader::new(
        StatelessFile::new(std::fs::File::open(assets.join("patch.rom")).unwrap()).unwrap(),
    )
    .unwrap();
    let mut source = Vec::new();
    archive
        .open_file(archive.find_file("/main.snr").unwrap())
        .unwrap()
        .read_to_end(&mut source)
        .unwrap();
    assert_eq!(source.len(), 26_104_192);
    let chart_start = u32::from_le_bytes(source[76..80].try_into().unwrap()) as usize;
    let scenario = HigurashiScenario::new(source.into()).unwrap();
    let catalog = Catalog::from_scenario(&scenario).unwrap();
    assert_eq!(catalog.declared_count, 1449);
    assert_eq!(catalog.records.len(), 1449);
    assert_eq!(scenario.chart_data.len(), 18_760);

    let capture = std::env::var_os("SHIN_HIGURASHI_CHART_CATALOG_CAPTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            project.join("test-fixtures/higurashi/chart-catalog-capture/output.txt")
        });
    let original = std::fs::read_to_string(capture).unwrap();
    let mut rows = original.lines();
    assert_eq!(rows.next(), Some("H 26104192 1449 1 1 1 1"));
    let mut kinds = [0usize; 6];
    let mut labels = 0;
    for (index, actual) in catalog.records.iter().enumerate() {
        let words: Vec<i64> = rows
            .next()
            .unwrap()
            .split_whitespace()
            .skip(1)
            .map(|word| word.parse().unwrap())
            .collect();
        assert_eq!(
            &words[..5],
            &[
                index as i64,
                actual.kind.into(),
                i64::from(actual.x as u16),
                i64::from(actual.y as u16),
                actual.value.into(),
            ],
            "Native Chart record{index}"
        );
        kinds[usize::from(actual.kind)] += 1;
        if let Some(label) = &actual.label {
            labels += 1;
            assert_eq!(words[5], i64::from(label.extra), "record{index} extra");
            assert_eq!(
                words[6] as usize,
                chart_start + 8 + label.source_offset,
                "record{index} actual retained source pointer"
            );
            assert_eq!(words[7] as usize, label.c_string_bytes().len());
            let encoded: Vec<i64> = label
                .c_string_bytes()
                .iter()
                .map(|b| i64::from(*b))
                .collect();
            assert_eq!(
                &words[8..],
                encoded.as_slice(),
                "record{index} source label"
            );
        } else {
            assert_eq!(&words[5..], &[-1, -1, 0], "record{index} absent label");
        }
    }
    assert_eq!(rows.next(), Some("X 0 0"));
    assert_eq!(rows.next(), None);
    assert_eq!(kinds, [449, 431, 506, 7, 51, 5]);
    assert_eq!(labels, 431);
    assert!(catalog.get(-1).is_none());
    assert!(catalog.get(1449).is_none());
    assert!(catalog.get(1450).is_none());
    println!(
        "Chart:1449 original typed records/431 source labels/all18760 payloadbytes and3 original accessor rejection cases"
    );
}
