use super::*;
use crate::format::{picture::PicBlock, scenario::gerokasu2::Gerokasu2Scenario};
use std::{collections::BTreeMap, io::Read};

struct AllBlocks;
struct Stats {
    names: Vec<String>,
    blocks: usize,
    pixels: u64,
}
impl BustupBuilder for AllBlocks {
    type Args = ();
    type Skeleton<'a> = BustupSkeleton<'a>;
    type BlockType = u64;
    type Output = Stats;
    fn new<'a>(_: &(), skeleton: BustupSkeleton<'a>) -> Self::Skeleton<'a> {
        skeleton
    }
    fn new_block(_: &(), _: u32, block: PicBlock) -> Result<u64> {
        Ok(u64::from(block.data.width()) * u64::from(block.data.height()))
    }
    fn build(
        _: &(),
        skeleton: Self::Skeleton<'_>,
        token: BustupBlockPromiseToken<u64>,
    ) -> Result<Stats> {
        let mut result = Stats {
            names: skeleton.expressions.keys().cloned().collect(),
            blocks: 0,
            pixels: 0,
        };
        let mut consume = |block: BustupBlockPromise<'_>| {
            result.blocks += 1;
            result.pixels += *block.get(&token);
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
        Ok(result)
    }
}

fn empty_bustup(name: &[u8]) -> Vec<u8> {
    let mut expression = 1u32.to_le_bytes().to_vec();
    let size = (32 + name.len() + 1 + 3) & !3;
    expression.extend((size as u32).to_le_bytes());
    expression.extend([0; 28]);
    expression.extend(name);
    expression.push(0);
    expression.resize(4 + size, 0);
    let mut bytes = b"BUP4".to_vec();
    for word in [
        4u32,
        (56 + expression.len()) as u32,
        0,
        0x10001,
        1,
        7,
        0,
        52,
        4,
        0,
        56,
        expression.len() as u32,
    ] {
        bytes.extend(word.to_le_bytes());
    }
    assert_eq!(bytes.len(), 52);
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(expression);
    bytes
}

#[test]
fn gerokasu2_bustup_expression_encoding_is_explicit_and_preserves_legacy_names() {
    let sjis = empty_bustup(&[0x93, 0xfa, 0x96, 0x7b]);
    assert_eq!(read_bustup::<AllBlocks>(&sjis, ()).unwrap().names, ["日本"]);
    assert!(read_bustup_with_encoding::<AllBlocks>(&sjis, (), TextEncoding::Utf8).is_err());
    let utf8 = empty_bustup("通常_顔影2".as_bytes());
    assert_eq!(
        read_bustup_with_encoding::<AllBlocks>(&utf8, (), TextEncoding::Utf8)
            .unwrap()
            .names,
        ["通常_顔影2"]
    );
    assert!(
        read_bustup_with_encoding::<AllBlocks>(&empty_bustup(&[0xff]), (), TextEncoding::Utf8)
            .is_err()
    );
}

#[test]
#[ignore = "requires supplied Gerokasu2 ROM; all bustup blocks, CPU only"]
fn gerokasu2_all_bustup_blocks_decode_with_utf8_scenario_expressions() {
    use crate::format::rom::{IndexEntry, RomReader};
    use shin_primitives::stateless_reader::StatelessFile;
    let root = std::env::var_os("SHIN_GEROKASU2_ROM").expect("SHIN_GEROKASU2_ROM");
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(root).unwrap()).unwrap()).unwrap();
    let paths = rom
        .traverse()
        .filter_map(|(path, entry)| match entry {
            IndexEntry::File(file) if path.ends_with(".bup") => Some((path, *file)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut expressions = BTreeMap::new();
    let mut names = 0;
    let mut blocks = 0;
    let mut pixels = 0;
    for (path, file) in &paths {
        let mut bytes = Vec::new();
        rom.open_file(*file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let result = read_bustup_with_encoding::<AllBlocks>(&bytes, (), TextEncoding::Utf8)
            .unwrap_or_else(|e| panic!("{path}: {e:#}"));
        names += result.names.len();
        blocks += result.blocks;
        pixels += result.pixels;
        assert!(result.names.iter().all(|name| !name.is_ascii()));
        println!(
            "Gerokasu2 bustup decoded: {path}, {} expressions, {} blocks",
            result.names.len(),
            result.blocks
        );
        expressions.insert(path.clone(), result.names);
    }
    let mut bytes = Vec::new();
    let file = rom.find_file("/main.snr").unwrap();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Gerokasu2Scenario::new(bytes.into()).unwrap();
    for (index, item) in scenario.bustup_info.iter().enumerate() {
        let path = format!("/bustup/M/{}.bup", item.name.as_str()).to_ascii_lowercase();
        let names = expressions.get(&path).unwrap();
        assert!(
            names.iter().any(|name| name == item.emotion.as_str()),
            "missing expression for bustup {index}"
        );
    }
    assert_eq!(paths.len(), 46);
    assert_eq!(names, 645);
    assert_eq!(scenario.bustup_info.len(), 644);
    println!(
        "GEROKASU2_BUSTUPS files={} expressions={names} scenario_expressions={} blocks={blocks} pixels={pixels}",
        paths.len(),
        scenario.bustup_info.len()
    );
}
