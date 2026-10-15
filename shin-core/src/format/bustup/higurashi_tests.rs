use super::*;

fn word(data: &mut Vec<u8>, value: u32) {
    data.extend_from_slice(&value.to_le_bytes());
}

fn fragment(color: [u8; 4]) -> Vec<u8> {
    let mut data = Vec::new();
    for value in [3u16, 1, 0, 2, 0, 0, 1, 1] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    // Native69974 reads the complete u32 at byte16. This raw dictionary
    // fragment has no compressed payload; the rectangle begins at byte20.
    word(&mut data, 0);
    for value in [0u16, 0, 1, 1] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    data.extend_from_slice(&[0; 4]);
    data.extend_from_slice(&color);
    data.resize(1056, 0);
    data.extend_from_slice(&[0; 4]);
    data
}

fn fixture() -> Vec<u8> {
    let mut data = b"BUP4".to_vec();
    for value in [3u32, 8640, 0, 0x0001_0001, 1, 1, 1, 2, 42, 0, 0] {
        word(&mut data, value);
    }
    let location = |data: &mut Vec<u8>, index: Option<u32>| {
        word(data, index.map_or(0, |index| 160 + index * 1060));
        word(data, if index.is_some() { 1060 } else { 0 });
    };
    location(&mut data, Some(0));
    word(&mut data, 0);
    location(&mut data, None);
    word(&mut data, 1234);
    location(&mut data, Some(1));
    data.extend_from_slice(b"test\0\0\0\0\0\0\0\0\0\0\0\0");
    word(&mut data, 1);
    location(&mut data, Some(2));
    for index in [Some(3), None, Some(4), Some(5), Some(6), Some(7)] {
        location(&mut data, index);
    }
    assert_eq!(data.len(), 156);
    data.resize(160, 0);
    for index in 0..8 {
        data.extend_from_slice(&fragment([index + 1, 20, 30, 255]));
    }
    assert_eq!(data.len(), 8640);
    data
}

#[test]
fn higurashi_bustup_preserves_three_frames_null_slots_and_auxiliary_face() {
    let data = fixture();
    let metadata = HigurashiBustupMetadata::read(&data).unwrap();
    assert_eq!(metadata.header.bustup_id, BustupId::new(42));
    assert_eq!(metadata.auxiliary[1].identifier, 1234);
    assert_eq!(metadata.expressions[0].auxiliary_index, 1);
    let bustup = read_higurashi_bustup(&data).unwrap();
    // Rectangle ends are exclusive and may equal the fragment's dimensions.
    assert_eq!(bustup.base_image.as_raw(), &[1, 20, 30, 255]);
    let expression = &bustup.expressions["test"];
    assert_eq!(
        expression.face1.as_ref().unwrap().data.as_raw(),
        &[2, 20, 30, 255]
    );
    assert_eq!(
        expression.face2.as_ref().unwrap().data.as_raw(),
        &[3, 20, 30, 255]
    );
    assert_eq!(expression.eyes.len(), 3);
    assert!(expression.eyes[1].is_empty());
    assert_eq!(expression.eyes[2].data.as_raw(), &[5, 20, 30, 255]);
    assert_eq!(expression.mouths.len(), 3);
    for (index, block) in expression.mouths.iter().enumerate() {
        assert_eq!(block.data.get_pixel(0, 0).0, [6 + index as u8, 20, 30, 255]);
    }
}

#[test]
fn higurashi_bustup_checks_tables_locations_and_auxiliary_indices() {
    let data = fixture();
    for (offset, value) in [(24, u32::MAX), (48, u32::MAX), (96, 2), (64, 1)] {
        let mut invalid = data.clone();
        invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            HigurashiBustupMetadata::read(&invalid).is_err(),
            "offset {offset}"
        );
    }
    for length in [0, 16, 47] {
        assert!(HigurashiBustupMetadata::read(&data[..length]).is_err());
    }
    let mut invalid = data;
    // A rectangle outside the fragment is reported rather than indexed.
    invalid[160 + 24..160 + 26].copy_from_slice(&2u16.to_le_bytes());
    assert!(read_higurashi_bustup(&invalid).is_err());
}

#[test]
fn higurashi_bustup_selection_does_not_relax_umineko_v4_reader() {
    let data = fixture();
    assert!(super::super::read_bustup_header(&data).is_err());
    assert!(
        super::super::read_bustup::<super::super::default_builder::DefaultBustupBuilder>(&data, ())
            .is_err()
    );
    let mut v4 = data;
    v4[4..8].copy_from_slice(&4u32.to_le_bytes());
    assert!(HigurashiBustupMetadata::read(&v4).is_err());
}

#[test]
#[ignore = "requires SHIN_HIGURASHI_ASSETS; bounded metadata reads, no rendering/audio"]
fn supplied_higurashi_bustup_tables() {
    use crate::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    use std::io::Read;
    let inputs = std::env::var_os("SHIN_HIGURASHI_ASSETS").expect("SHIN_HIGURASHI_ASSETS");
    let mut totals = [0usize; 4];
    for (archive, expected_files) in [("data.rom", 239), ("append.rom", 122)] {
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(std::path::Path::new(&inputs).join(archive)).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let paths = rom
            .traverse()
            .filter_map(|(path, _)| {
                if path.ends_with(".bup") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(paths.len(), expected_files);
        for path in paths {
            let mut bytes = Vec::new();
            rom.open_file(rom.find_file(&path).unwrap())
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            let metadata = HigurashiBustupMetadata::read(&bytes)
                .unwrap_or_else(|error| panic!("{archive}:{path}: {error:#}"));
            totals[0] += 1;
            totals[1] += metadata.base_blocks.len();
            totals[2] += metadata.expressions.len();
            totals[3] += metadata
                .expressions
                .iter()
                .filter(|expression| expression.mouths.iter().all(|location| !location.is_null()))
                .count();
        }
    }
    assert_eq!(totals, [361, 6215, 2987, 2066]);
}
