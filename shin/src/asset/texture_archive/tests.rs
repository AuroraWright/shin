use super::decode_archive;
use crate::cli::Game;

/// A literal differential TXA4 row with an independently authored name and pixel.
fn archive(name: &[u8]) -> Vec<u8> {
    let entry_size = (20 + name.len() + 1 + 3) & !3;
    let payload_offset = 32 + entry_size;
    let mut bytes = b"TXA4".to_vec();
    for word in [
        2,
        (payload_offset + 16) as u32,
        2,
        1,
        16,
        entry_size as u32,
        0,
    ] {
        bytes.extend(word.to_le_bytes());
    }
    for word in [entry_size as u16, 7, 1, 1] {
        bytes.extend(word.to_le_bytes());
    }
    for word in [payload_offset as u32, 0, 16] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(name);
    bytes.resize(payload_offset, 0);
    bytes.extend([0x12, 0x34, 0x56, 0xff]);
    bytes.resize(payload_offset + 16, 0);
    bytes
}

#[test]
fn atlas_asset_decodes_names_for_the_selected_game_and_preserves_pixels() {
    for (game, name) in [
        (Game::Gerokasu2, "日本".as_bytes()),
        (Game::Umineko, &[0x93, 0xfa, 0x96, 0x7b]),
        (Game::Higurashi, &[0x93, 0xfa, 0x96, 0x7b]),
    ] {
        let decoded = decode_archive(&archive(name), game).unwrap();
        assert_eq!(decoded.name_to_index.len(), 1);
        assert_eq!(decoded.name_to_index["日本"], 0);
        assert_eq!(decoded.vindex_to_index[&7], 0);
        assert_eq!(
            decoded.get_texture("日本").unwrap().as_raw(),
            &[0x12, 0x34, 0x56, 0xff]
        );
    }
    assert!(decode_archive(&archive(&[0xff]), Game::Gerokasu2).is_err());
}

#[test]
#[ignore = "requires supplied Gerokasu2 ROM; CPU only (SHIN_GEROKASU2_ROM)"]
fn atlas_asset_decodes_all_supplied_gerokasu2_archives_through_game_dispatch() {
    use shin_core::{
        format::rom::{IndexEntry, RomReader},
        primitives::stateless_reader::StatelessFile,
    };
    use std::io::Read;

    let path = std::env::var_os("SHIN_GEROKASU2_ROM").expect("SHIN_GEROKASU2_ROM");
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap()).unwrap();
    let files = rom
        .traverse()
        .filter_map(|(path, entry)| match entry {
            IndexEntry::File(file) if path.ends_with(".txa") => Some((path, *file)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 14);
    let mut textures = 0;
    for (path, entry) in files {
        let mut bytes = Vec::new();
        rom.open_file(entry)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let decoded = decode_archive(&bytes, Game::Gerokasu2)
            .unwrap_or_else(|error| panic!("{path}: {error:#}"));
        assert_eq!(
            decoded.name_to_index.len(),
            decoded.textures.len(),
            "{path}"
        );
        if path == "/title.txa" {
            assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 2);
            assert_eq!(decoded.textures.len(), 7);
        }
        textures += decoded.textures.len();
    }
    println!(
        "GEROKASU2_ATLAS_ASSET_DISPATCH archives=14 textures={textures} UTF8_names_original_title_flags=verified CPU_only=true"
    );
}
