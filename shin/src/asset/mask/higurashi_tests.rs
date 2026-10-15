use shin_core::{
    format::{mask::read_higurashi_mask, rom::RomReader},
    primitives::stateless_reader::StatelessFile,
};
use std::{
    io::{BufReader, BufWriter, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
};
#[test]
#[ignore = "all supplied MSK3 files against updated native71e80 LZ77 and bounded header/stride reader"]
fn higurashi_masks_match_native_lz77_for_all_supplied_assets() {
    let assets = PathBuf::from(std::env::var("SHIN_HIGURASHI_ASSETS").unwrap());
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_MASK_LZ77").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = BufWriter::new(child.stdin.take().unwrap());
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut assets_count = 0;
    let mut compressed_count = 0;
    let mut pixels = 0;
    for archive in ["patch.rom", "append.rom", "data.rom"] {
        let mut rom = RomReader::new(
            StatelessFile::new(std::fs::File::open(assets.join(archive)).unwrap()).unwrap(),
        )
        .unwrap();
        let paths: Vec<_> = rom
            .traverse()
            .map(|(path, _)| path)
            .filter(|path| path.ends_with(".msk"))
            .collect();
        for path in paths {
            let entry = rom.find_file(&path).unwrap();
            let mut bytes = Vec::new();
            rom.open_file(entry)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            let image = read_higurashi_mask(&bytes).unwrap();
            let width = image.width() as usize;
            let height = image.height() as usize;
            let stride = (width + 15) & !15;
            let compressed = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
            let native = if compressed != 0 {
                compressed_count += 1;
                input.write_all(b"NLZ1").unwrap();
                input.write_all(&compressed.to_le_bytes()).unwrap();
                input.write_all(&12u32.to_le_bytes()).unwrap();
                input.write_all(&bytes[32..]).unwrap();
                input.flush().unwrap();
                let mut size = [0; 4];
                output.read_exact(&mut size).unwrap();
                assert_eq!(u32::from_le_bytes(size) as usize, stride * height);
                let mut raw = vec![0; stride * height];
                output.read_exact(&mut raw).unwrap();
                raw
            } else {
                bytes[32..].to_vec()
            };
            let packed: Vec<_> = native
                .chunks_exact(stride)
                .flat_map(|row| row[..width].iter().copied())
                .collect();
            assert_eq!(image.as_raw(), &packed, "{archive}:{path}");
            pixels += packed.len();
            assets_count += 1;
            assert!(
                shin_core::format::mask::read_mask(&bytes).is_err(),
                "Umineko still requires MSK4"
            );
            for cut in [0, 4, 12, 31, bytes.len() - 1] {
                assert!(read_higurashi_mask(&bytes[..cut]).is_err());
            }
        }
    }
    drop(input);
    assert!(child.wait().unwrap().success());
    assert_eq!(assets_count, 42);
    println!(
        "Higurashi masks:{assets_count} assets,{compressed_count} native compressed streams,{pixels} decoded texels"
    );
}
#[test]
fn higurashi_mask_padding_and_invalid_references_are_bounded() {
    for width in [1u16, 17] {
        let stride = (usize::from(width) + 15) & !15;
        let mut data = vec![0; 32 + 2 * stride];
        data[..4].copy_from_slice(b"MSK3");
        data[4..8].copy_from_slice(&1u32.to_le_bytes());
        let size = data.len() as u32;
        data[8..12].copy_from_slice(&size.to_le_bytes());
        data[12..14].copy_from_slice(&width.to_le_bytes());
        data[14..16].copy_from_slice(&2u16.to_le_bytes());
        for (i, b) in data[32..].iter_mut().enumerate() {
            *b = i as u8;
        }
        let image = read_higurashi_mask(&data).unwrap();
        assert_eq!(image.as_raw().len(), usize::from(width) * 2);
        assert_eq!(image.as_raw()[usize::from(width)], stride as u8);
        data[16..20].copy_from_slice(&3u32.to_le_bytes());
        data.truncate(35);
        data[32..].copy_from_slice(&[1, 0, 0]);
        data[8..12].copy_from_slice(&35u32.to_le_bytes());
        assert!(read_higurashi_mask(&data).is_err());
    }
}
