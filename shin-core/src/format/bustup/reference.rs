//! Complete supplied-ROM PIC/BUP/TXA decoding, including unselected expressions.
use super::*;
use crate::{
    format::{
        picture::PicBlock,
        rom::{IndexEntry, RomReader},
    },
    primitives::stateless_reader::StatelessFile,
};
use std::{collections::BTreeMap, io::Read};

struct AllBlocks;
struct AllPictureBlocks {
    blocks: usize,
    pixels: u64,
}
impl crate::format::picture::PictureBuilder for AllPictureBlocks {
    type Args = ();
    type Output = (usize, u64);
    fn new(_: (), _: u32, _: u32, _: i32, _: i32, _: u32) -> Self {
        Self {
            blocks: 0,
            pixels: 0,
        }
    }
    fn add_block(&mut self, _: u32, _: Vec<(u32, u32)>, block: PicBlock) -> Result<()> {
        self.blocks += 1;
        self.pixels += u64::from(block.data.width()) * u64::from(block.data.height());
        Ok(())
    }
    fn build(self) -> Result<Self::Output> {
        Ok((self.blocks, self.pixels))
    }
}

impl BustupBuilder for AllBlocks {
    type Args = ();
    type Skeleton<'a> = BustupSkeleton<'a>;
    type BlockType = u64;
    type Output = (usize, u64);

    fn new<'a>(_: &(), skeleton: BustupSkeleton<'a>) -> Self::Skeleton<'a> {
        skeleton
    }
    fn new_block(_: &(), _: u32, block: PicBlock) -> Result<u64> {
        // The normal decoder has already reconstructed every pixel. Discard
        // the image now to bound memory while retaining all block promises.
        Ok(u64::from(block.data.width()) * u64::from(block.data.height()))
    }
    fn build(
        _: &(),
        skeleton: BustupSkeleton<'_>,
        token: BustupBlockPromiseToken<'_, u64>,
    ) -> Result<Self::Output> {
        let mut blocks = 0;
        let mut pixels = 0;
        let mut consume = |promise: BustupBlockPromise<'_>| {
            pixels += *promise.get(&token);
            blocks += 1;
        };
        for block in skeleton.base_blocks {
            consume(block);
        }
        for expression in skeleton.expressions.into_values() {
            for block in expression
                .face1
                .into_iter()
                .chain(expression.face2)
                .chain(expression.mouth_blocks)
                .chain(expression.eye_blocks)
            {
                consume(block);
            }
        }
        Ok((blocks, pixels))
    }
}

#[test]
#[ignore = "requires supplied ROM (SHIN_REFERENCE_ROM); decodes all PIC/BUP/TXA payloads without GPU/audio"]
fn reference_rom_all_picture_bustup_blocks_and_texture_archives_decode() {
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var("SHIN_REFERENCE_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let files: Vec<_> = rom
        .traverse()
        .filter_map(|(path, entry)| match entry {
            IndexEntry::File(file) => Some((path, *file)),
            _ => None,
        })
        .collect();
    assert_eq!(files.len(), 109_791);
    let mut bustups = 0;
    let mut pictures = 0;
    let mut picture_blocks = 0;
    let mut picture_pixels = 0u64;
    let mut archives = 0;
    let mut textures = 0;
    let mut blocks = 0;
    let mut bustup_pixels = 0u64;
    let mut archive_pixels = 0u64;
    let mut flags = BTreeMap::<u16, usize>::new();
    let mut dictionary_archives = 0;
    let started = std::time::Instant::now();
    let mut next_progress = started;
    for (path, file) in files {
        if file.size() < 4 {
            continue;
        }
        let mut reader = rom.open_file(file).unwrap();
        let mut magic = [0; 4];
        reader.read_exact(&mut magic).unwrap();
        if magic != *b"BUP4" && magic != *b"TXA4" && magic != *b"PIC4" {
            continue;
        }
        let mut bytes = magic.to_vec();
        reader.read_to_end(&mut bytes).unwrap();
        if magic == *b"BUP4" {
            let mut cursor = std::io::Cursor::new(&bytes[..]);
            let header = BustupHeader::read(&mut cursor).unwrap();
            let base = with_offset_and_size(
                &mut cursor,
                header.base_block_descriptors_offset,
                header.base_block_descriptors_size,
                BaseBlockDescriptors::read,
            )
            .unwrap();
            let expressions = with_offset_and_size(
                &mut cursor,
                header.expression_descriptors_offset,
                header.expression_descriptors_size,
                ExpressionDescriptors::read,
            )
            .unwrap();
            let mut expected_blocks = 0;
            for descriptor in base
                .descriptors
                .iter()
                .chain(expressions.iter_additional_block_descs())
            {
                if descriptor.is_null() {
                    continue;
                }
                let start = descriptor.offset as usize;
                let data = bytes
                    .get(start..start.checked_add(descriptor.size as usize).unwrap())
                    .unwrap_or_else(|| panic!("Block outside {path}: {descriptor:?}"));
                if !data.is_empty() {
                    let flag = u16::from_le_bytes(data[..2].try_into().unwrap());
                    *flags.entry(flag).or_default() += 1;
                }
                expected_blocks += 1;
            }
            let (decoded, pixels) = read_bustup::<AllBlocks>(&bytes, ())
                .unwrap_or_else(|error| panic!("Decode every block of {path}: {error:#}"));
            assert_eq!(decoded, expected_blocks, "Unvisited block in {path}");
            blocks += decoded;
            bustup_pixels += pixels;
            bustups += 1;
        } else if magic == *b"PIC4" {
            let (decoded, pixels) =
                crate::format::picture::read_picture::<AllPictureBlocks>(&bytes, ())
                    .unwrap_or_else(|error| {
                        panic!("Decode every picture block of {path}: {error:#}")
                    });
            pictures += 1;
            picture_blocks += decoded;
            picture_pixels += pixels;
        } else {
            if u32::from_le_bytes(bytes[12..16].try_into().unwrap()) != 0 {
                dictionary_archives += 1;
            }
            let archive = crate::format::texture_archive::read_texture_archive(&bytes)
                .unwrap_or_else(|error| panic!("Decode all textures of {path}: {error:#}"));
            textures += archive.textures.len();
            archive_pixels += archive
                .textures
                .iter()
                .map(|image| u64::from(image.width()) * u64::from(image.height()))
                .sum::<u64>();
            archives += 1;
        }
        if std::time::Instant::now() >= next_progress {
            eprintln!(
                "All PIC/BUP/TXA decode: {pictures} pictures/{picture_blocks} blocks, {bustups} bustups/{blocks} blocks, {archives} archives/{textures} textures, {:.1}s",
                started.elapsed().as_secs_f64()
            );
            next_progress = std::time::Instant::now() + std::time::Duration::from_secs(10);
        }
    }
    assert_eq!((pictures, bustups, archives), (2601, 581, 372));
    assert_eq!(dictionary_archives, archives);
    assert!(
        flags.keys().all(|flags| flags & 2 != 0),
        "Differential BUP blocks: {flags:?}"
    );
    if let Some((native_blocks, native_pixels)) =
        crate::format::picture::reference::finish_native_dictionary_comparison()
    {
        assert_eq!(native_blocks, picture_blocks + blocks + textures);
        assert_eq!(
            native_pixels,
            picture_pixels + bustup_pixels + archive_pixels
        );
        println!(
            "Complete supplied-ROM native dictionary comparison: {native_blocks} decoded payloads, {native_pixels} exact RGBA pixels; native input/output/padding guards checked"
        );
    }
    if let Some((native_streams, native_bytes)) =
        crate::format::lz77::reference::finish_native_lz77_comparison()
    {
        assert!(native_streams > 0);
        assert!(native_bytes > 0);
        println!(
            "Complete supplied-ROM native LZ77 comparison: {native_streams} compressed image payloads, {native_bytes} exact decoded bytes; native input/output guards checked"
        );
    }
    println!(
        "Complete supplied-ROM PIC/BUP/TXA decode: {pictures} pictures, {picture_blocks} unique picture blocks, {picture_pixels} picture pixels; {bustups} bustups, {blocks} block references, {bustup_pixels} bustup pixels, flags={flags:?}; {archives} dictionary archives, {textures} textures, {archive_pixels} archive pixels; {:.3}s",
        started.elapsed().as_secs_f64()
    );
}
