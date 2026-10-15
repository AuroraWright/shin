use super::*;

fn fixture(updated: bool) -> Vec<u8> {
    let mut data = vec![0; 0x50];
    data[..4].copy_from_slice(b"SNR ");
    data[12..16].copy_from_slice(&(if updated { 65u32 } else { 63 }).to_le_bytes());
    data[16..20].copy_from_slice(&(if updated { 133u32 } else { 129 }).to_le_bytes());
    for index in 0..11 {
        let offset = data.len() as u32;
        data[36 + index * 4..40 + index * 4].copy_from_slice(&offset.to_le_bytes());
        let mut payload = 0u32.to_le_bytes().to_vec();
        if index == 9 {
            payload = 1u32.to_le_bytes().to_vec();
            payload.extend_from_slice(&7u16.to_le_bytes());
            if updated {
                payload.extend_from_slice(&1006u16.to_le_bytes());
            }
            payload.extend_from_slice(b"\x0dtips/006.txa\0");
        }
        if !matches!(index, 7 | 8) {
            data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        }
        data.extend_from_slice(&payload);
        while data.len() % 4 != 0 {
            data.push(0);
        }
    }
    let code = data.len() as u32;
    data[32..36].copy_from_slice(&code.to_le_bytes());
    data.extend_from_slice(&[0, 0, 0, 0]);
    let size = data.len() as u32;
    data[4..8].copy_from_slice(&size.to_le_bytes());
    data
}

#[test]
fn tips_layout_depends_on_original_or_updated_header() {
    for updated in [false, true] {
        let data = fixture(updated);
        let scenario = HigurashiScenario::new(data.clone().into()).unwrap();
        assert_eq!(scenario.tips_info[0].id, 7);
        assert_eq!(scenario.tips_info[0].texture_path.as_str(), "tips/006.txa");
        assert_eq!(scenario.tips_info[0].updated_field, updated.then_some(1006));
        assert!(scenario.chart_data.is_empty());
        let error = super::super::Scenario::new(data.into()).err().unwrap();
        assert!(error.to_string().contains("HigurashiScenario"));
    }
}

#[test]
fn rejects_invalid_table_spans_sizes_and_counts() {
    let valid = fixture(true);
    for (offset, word) in [
        (36, 0x54u32),
        (40, u32::MAX),
        (0x50, u32::MAX),
        (0x54, u32::MAX),
    ] {
        let mut data = valid.clone();
        data[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        assert!(
            HigurashiScenario::new(data.into()).is_err(),
            "offset {offset:x}"
        );
    }
    let mut data = valid;
    data[12..16].copy_from_slice(&66u32.to_le_bytes());
    assert!(HigurashiScenario::new(data.into()).is_err());
}

#[test]
#[ignore = "requires supplied Higurashi ROMs (SHIN_HIGURASHI_ASSETS)"]
fn supplied_original_and_updated_metadata() {
    use crate::{
        format::rom::{IndexEntry, RomReader},
        primitives::stateless_reader::StatelessFile,
    };
    use std::{io::Read, path::PathBuf};
    let root = PathBuf::from(std::env::var_os("SHIN_HIGURASHI_ASSETS").unwrap());
    for (name, file_count, counts) in [
        (
            "data.rom",
            120440,
            [18, 1314, 2082, 159, 361, 12, 73, 158, 158, 188],
        ),
        (
            "patch.rom",
            127,
            [39, 1960, 2667, 254, 563, 13, 85, 158, 158, 315],
        ),
    ] {
        let mut rom = RomReader::new(
            StatelessFile::new(std::fs::File::open(root.join(name)).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            rom.traverse()
                .filter(|(_, entry)| matches!(entry, IndexEntry::File(_)))
                .count(),
            file_count
        );
        let mut data = Vec::new();
        rom.open_file(rom.find_file("/main.snr").unwrap())
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        let scenario = HigurashiScenario::new(data.into()).unwrap();
        assert_eq!(
            [
                scenario.mask_info.len(),
                scenario.picture_info.len(),
                scenario.bustup_info.len(),
                scenario.bgm_info.len(),
                scenario.se_info.len(),
                scenario.movie_info.len(),
                scenario.voice_mapping_info.len(),
                scenario.picture_box_info.len(),
                scenario.music_box_info.len(),
                scenario.tips_info.len()
            ],
            counts
        );
        assert_eq!(scenario.chart_record_count, 1449);
        assert_eq!(scenario.chart_data.len(), 18760);
        assert_eq!(scenario.mask_info[0].name.as_str(), "1");
        assert_eq!(scenario.bustup_info[0].emotion.as_str(), "DefA1");
        assert_eq!(scenario.movie_info[0].raw_fields, [87, -1]);
        assert_eq!(scenario.tips_info[0].texture_path.as_str(), "tips/000.txa");
        println!(
            "{name}: {file_count} archive files; all ten typed metadata tables consumed; 1449 chart records retained"
        );
    }
    let rom = RomReader::new(
        StatelessFile::new(std::fs::File::open(root.join("append.rom")).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        rom.traverse()
            .filter(|(_, entry)| matches!(entry, IndexEntry::File(_)))
            .count(),
        5181
    );
}

#[test]
#[ignore = "requires supplied Umineko ROMs (SHIN_REFERENCE_ROM)"]
fn supplied_umineko_metadata_and_entry_remain_compatible() {
    use crate::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    use std::{io::Read, path::PathBuf};
    let base = PathBuf::from(std::env::var_os("SHIN_REFERENCE_ROM").unwrap());
    for path in [base.clone(), base.parent().unwrap().join("patch.rom")] {
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(&path).unwrap()).unwrap())
                .unwrap();
        let mut data = Vec::new();
        rom.open_file(rom.find_file("/main.snr").unwrap())
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        let scenario = super::super::Scenario::new(data.into()).unwrap();
        assert!(scenario.info_tables().picture_info.len() > 1000);
        assert!(!scenario.info_tables().tips_info.is_empty());
        assert!(matches!(
            scenario
                .instruction_reader(scenario.entrypoint_address())
                .read()
                .unwrap(),
            super::super::instructions::Instruction::jc { .. }
        ));
        println!(
            "{}: existing Umineko metadata and entry decoded",
            path.display()
        );
    }
}

#[test]
#[ignore = "requires supplied Higurashi archives (SHIN_HIGURASHI_ASSETS)"]
fn supplied_higurashi_shared_bootstrap_decoders() {
    use crate::{
        format::{
            font::{GlyphMipLevel, read_lazy_font},
            rom::RomReader,
            texture_archive::read_texture_archive,
        },
        primitives::stateless_reader::StatelessFile,
    };
    use std::{io::Read, path::PathBuf};
    let root = PathBuf::from(std::env::var_os("SHIN_HIGURASHI_ASSETS").unwrap());
    let mut base = RomReader::new(
        StatelessFile::new(std::fs::File::open(root.join("data.rom")).unwrap()).unwrap(),
    )
    .unwrap();
    let mut patch = RomReader::new(
        StatelessFile::new(std::fs::File::open(root.join("patch.rom")).unwrap()).unwrap(),
    )
    .unwrap();
    for name in [
        "/title.txa",
        "/adv.txa",
        "/msgtex.txa",
        "/effects.txa",
        "/systex.txa",
    ] {
        let (rom, file) = match patch.find_file(name) {
            Ok(file) => (&mut patch, file),
            Err(_) => {
                let file = base.find_file(name).unwrap();
                (&mut base, file)
            }
        };
        let mut data = Vec::new();
        rom.open_file(file).unwrap().read_to_end(&mut data).unwrap();
        let archive = read_texture_archive(&data).unwrap();
        assert!(!archive.textures.is_empty());
        assert!(
            archive
                .textures
                .iter()
                .all(|image| image.width() > 0 && image.height() > 0)
        );
        println!(
            "{name}: {} textures decoded with shared TXA reader",
            archive.textures.len()
        );
    }
    let font_file = base.find_file("/newrodin.fnt").unwrap();
    let mut font_data = base.open_file(font_file).unwrap();
    let font = read_lazy_font(&mut font_data).unwrap();
    assert!(font.get_ascent() > 0);
    for character in ['あ', 'ひ', 'ぐ', 'ら', 'し'] {
        let glyph = font
            .try_get_glyph_for_character(character as u16)
            .unwrap()
            .decompress();
        assert!(
            glyph
                .get_image(GlyphMipLevel::Level0)
                .as_raw()
                .iter()
                .any(|&value| value != 0)
        );
    }
    println!("/newrodin.fnt: shared FNT reader and five Japanese glyphs decoded");
}
