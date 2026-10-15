use super::*;

fn archive(flags: u32, name: &[u8]) -> Vec<u8> {
    let mut index = Vec::new();
    let entry_length = (20 + name.len() + 1 + 3) & !3;
    for word in [entry_length as u16, 7, 1, 2] {
        index.extend(word.to_le_bytes());
    }
    let payload_offset = 32 + entry_length;
    let payload_length = if flags & 1 == 0 { 32u32 } else { 1032 };
    for word in [payload_offset as u32, 0, payload_length] {
        index.extend(word.to_le_bytes());
    }
    index.extend(name);
    index.resize(entry_length, 0);
    let mut data = b"TXA4".to_vec();
    for word in [
        2u32,
        payload_offset as u32 + payload_length,
        flags,
        1,
        payload_length,
        entry_length as u32,
        0,
    ] {
        data.extend(word.to_le_bytes());
    }
    data.extend(index);
    let first = [250, 1, 2, 255];
    let second = [5, 3, 4, 2];
    let mut payload = vec![0; payload_length as usize];
    if flags & 1 == 0 {
        payload[..4].copy_from_slice(&first);
        payload[16..20].copy_from_slice(&[11, 2, 2, 3]);
    } else {
        payload[..4].copy_from_slice(&first);
        payload[4..8].copy_from_slice(&second);
        payload[1028] = 1;
    }
    data.extend(payload);
    data
}

#[test]
fn gerokasu2_pixel_encoding_uses_bit_zero_and_names_use_selected_encoding() {
    for flags in [0, 1, 2, 3] {
        let data = archive(flags, "タイトル".as_bytes());
        let result = read_texture_archive_with_encoding(&data, TextEncoding::Utf8).unwrap();
        assert_eq!(result.textures[0].as_raw(), &[250, 1, 2, 255, 5, 3, 4, 2]);
        assert!(result.get_texture("タイトル").is_some());
        assert!(result.get_texture_by_vindex(7).is_some());
        let sjis = read_texture_archive(&archive(flags, &[0x93, 0xfa, 0x96, 0x7b])).unwrap();
        assert!(sjis.get_texture("日本").is_some());
    }
    assert!(read_texture_archive_with_encoding(&archive(2, &[0xff]), TextEncoding::Utf8).is_err());
}

#[test]
#[ignore = "requires supplied Gerokasu2 data.rom (SHIN_GEROKASU2_ROM)"]
fn gerokasu2_supplied_atlases() {
    use crate::format::rom::{IndexEntry, RomReader};
    use shin_primitives::stateless_reader::StatelessFile;
    use std::{fs::File, io::Read};
    let path = std::env::var_os("SHIN_GEROKASU2_ROM").expect("SHIN_GEROKASU2_ROM");
    let mut rom = RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
    let files = rom
        .traverse()
        .filter_map(|(path, entry)| match entry {
            IndexEntry::File(file) if path.ends_with(".txa") => Some((path, *file)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 14);
    let mut textures = 0;
    for (path, file) in files {
        let mut data = Vec::new();
        rom.open_file(file).unwrap().read_to_end(&mut data).unwrap();
        let flags = u32::from_le_bytes(data[12..16].try_into().unwrap());
        let atlas = read_texture_archive_with_encoding(&data, TextEncoding::Utf8)
            .unwrap_or_else(|error| panic!("{path}: {error:#}"));
        if path == "/title.txa" {
            assert_eq!(flags, 2);
            assert_eq!(atlas.textures.len(), 7);
        }
        textures += atlas.textures.len();
        let checksums = atlas
            .textures
            .iter()
            .map(|texture| crate::crc32::crc32(texture.as_raw(), 0))
            .collect::<Vec<_>>();
        println!(
            "{path} flags={flags} textures={} RGBA CRC32={checksums:x?}",
            atlas.textures.len()
        );
    }
    println!("Decoded all 14 unmodified Gerokasu2 atlases: {textures} textures");
}

#[test]
#[ignore = "requires Umineko ROM and guarded native dictionary oracle; CPU only"]
fn umineko_supplied_atlas_regression() {
    use crate::format::rom::RomReader;
    use shin_primitives::stateless_reader::StatelessFile;
    use std::{fs::File, io::Read};
    assert!(std::env::var_os("SHIN_REFERENCE_DICTIONARY_ORACLE").is_some());
    assert!(std::env::var_os("SHIN_REFERENCE_EXECUTABLE").is_some());
    assert_eq!(
        std::env::var("SHIN_REFERENCE_COMPARE_NATIVE_DICTIONARY").as_deref(),
        Ok("1")
    );
    let path = std::env::var_os("SHIN_REFERENCE_ROM").expect("SHIN_REFERENCE_ROM");
    let mut rom = RomReader::new(StatelessFile::new(File::open(path).unwrap()).unwrap()).unwrap();
    for (path, count) in [("/adv.txa", 6), ("/systex.txa", 4), ("/title.txa", 19)] {
        let file = rom.find_file(path).unwrap();
        let mut data = Vec::new();
        rom.open_file(file).unwrap().read_to_end(&mut data).unwrap();
        assert_eq!(u32::from_le_bytes(data[12..16].try_into().unwrap()), 1);
        // The existing per-texture reference hook compares every RGBA byte
        // against the original Umineko native dictionary function.
        let atlas = read_texture_archive(&data).unwrap();
        assert_eq!(atlas.textures.len(), count);
        assert_eq!(atlas.name_to_index.len(), count);
        println!("Umineko default decoder/native RGBA: {path}, {count} textures");
    }
    let (cases, pixels) =
        crate::format::picture::reference::finish_native_dictionary_comparison().unwrap();
    assert_eq!(cases, 29);
    println!("Umineko: {cases} textures, {pixels} native RGBA pixels compared byte for byte");
}
