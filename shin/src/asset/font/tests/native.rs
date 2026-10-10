use super::super::*;
use std::{collections::BTreeMap, fmt::Write, fs, path::Path, process::Command};

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, &byte| {
        (hash ^ byte as u64).wrapping_mul(0x100000001b3)
    })
}

#[test]
#[ignore = "requires supplied original/1.03 executables and ROM; CPU only"]
fn reference_native_font_decode_upload_and_cache_lifecycle() {
    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    use std::io::Read;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let supplied = root.join("umineko_switch_files");
    let out = std::env::var_os("SHIN_NATIVE_FONT_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("shin-native-font-{}", std::process::id()))
        });
    fs::create_dir_all(&out).unwrap();
    let helper = out.join("font-load");
    assert!(
        Command::new("clang")
            .args(["-O2", "-Wall", "-Wextra", "-Werror"])
            .arg(root.join("reverse-engineering/native-font-load.c"))
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    let mut rom = RomReader::new(
        StatelessFile::new(fs::File::open(supplied.join("data.rom")).unwrap()).unwrap(),
    )
    .unwrap();
    let names = ["newrodin-medium", "newrodin-bold", "system"];
    let mut fonts = Vec::new();
    for name in names {
        let path = format!("/{name}.fnt");
        let file = rom.find_file(&path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        fs::write(out.join(format!("{name}.fnt")), &bytes).unwrap();
        fonts.push(read_lazy_font(&mut Cursor::new(bytes)).unwrap());
    }
    let mut commands = String::new();
    let mut expected = Vec::new();
    for (index, font) in fonts.iter().enumerate() {
        // All distinct source glyphs, plus physical aliases checked separately
        // below. The native cache still acquires each codepoint independently.
        let mut unique = BTreeMap::new();
        for (cp, &id) in font.get_character_mapping().iter().enumerate() {
            unique.entry(id.0).or_insert(cp as u32);
        }
        for (&id, &cp) in &unique {
            writeln!(commands, "L {index} {cp} 0\nD 0").unwrap();
            let glyph = font.get_glyph(GlyphId(id)).unwrap().decompress();
            let info = glyph.get_info();
            let base = glyph.get_image(GlyphMipLevel::Level0);
            let mut pixels = Vec::new();
            for mip in [
                GlyphMipLevel::Level0,
                GlyphMipLevel::Level1,
                GlyphMipLevel::Level2,
                GlyphMipLevel::Level3,
            ] {
                pixels.extend_from_slice(glyph.get_image(mip).as_raw());
            }
            expected.push((
                index as u32,
                cp,
                vec![
                    font.get_character_table_crc(),
                    cp,
                    info.bearing_x as i32 as u32,
                    -(info.bearing_y as i32) as u32,
                    info.actual_width as u32,
                    info.actual_height as u32,
                    info.advance_width as u32,
                    font.get_ascent() as u32,
                    font.get_descent() as u32,
                    (info.actual_width as f32 / base.width() as f32).to_bits(),
                    (info.actual_height as f32 / base.height() as f32).to_bits(),
                    base.width(),
                    base.height(),
                    pixels.len() as u32,
                ],
                hash(&pixels),
            ));
        }
    }
    let input = out.join("corpus-input.txt");
    fs::write(&input, commands).unwrap();
    let mut traces = Vec::new();
    for version in ["uncompressed_executable", "uncompressed_executable_update"] {
        let trace = out.join(format!("{version}-corpus.trace"));
        let status = Command::new(&helper)
            .arg(supplied.join(version))
            .args(names.map(|n| out.join(format!("{n}.fnt"))))
            .stdin(fs::File::open(&input).unwrap())
            .stdout(fs::File::create(&trace).unwrap())
            .stderr(fs::File::create(out.join(format!("{version}-corpus.log"))).unwrap())
            .status()
            .unwrap();
        assert!(status.success(), "{version}");
        let output = fs::read_to_string(&trace).unwrap();
        let rows: Vec<_> = output.lines().collect();
        assert_eq!(rows.len(), expected.len() * 2);
        for ((index, cp, words, pixels), pair) in expected.iter().zip(rows.chunks_exact(2)) {
            let row: Vec<_> = pair[0].split_whitespace().collect();
            assert_eq!(row.len(), 22);
            assert_eq!(row[0], "G");
            assert_eq!(row[1].parse::<u32>().unwrap(), *index);
            assert_eq!(row[2].parse::<u32>().unwrap(), *cp);
            let native: Vec<_> = row[7..21]
                .iter()
                .map(|s| s.parse::<u32>().unwrap())
                .collect();
            assert_eq!(&native, words, "{version} font{index} U+{cp:04x}");
            assert_eq!(
                u64::from_str_radix(row[21], 16).unwrap(),
                *pixels,
                "{version} font{index} U+{cp:04x} mip pixels"
            );
            assert!(pair[1].starts_with("D "));
        }
        traces.push(output);
    }
    assert_eq!(
        traces[0], traces[1],
        "original/update corpus cache/decode/upload traces"
    );
    compare_cache_lifecycle(&helper, &supplied, &out, names, &fonts);
    println!(
        "Native glyph corpus:{} glyphs/version; exact metadata, UVs and four mip levels; original/update traces identical; output:{}",
        expected.len(),
        out.display()
    );
}

fn compare_cache_lifecycle(
    helper: &Path,
    supplied: &Path,
    out: &Path,
    names: [&str; 3],
    fonts: &[FontLazy],
) {
    use super::super::cache::Residency;
    let mut ops = vec![
        "L 0 65 0".to_owned(),
        "L 0 65 1".to_owned(),
        "L 1 65 2".to_owned(),
        "D 0".to_owned(),
        "D 1".to_owned(),
        "D 2".to_owned(),
        "L 0 65 0".to_owned(),
        "D 0".to_owned(),
        "F 0 60000".to_owned(),
        "L 0 60000 0".to_owned(),
        "D 0".to_owned(),
        "C".to_owned(),
    ];
    ops.extend((1..1057).map(|cp| format!("L 0 {cp} {cp}")));
    ops.push("C".to_owned());
    ops.extend((1..1057).map(|cp| format!("D {cp}")));
    ops.extend(
        [
            "L 0 1 0",
            "L 0 60001 1",
            "C",
            "D 0",
            "D 1",
            "A 65535",
            "L 2 50000 0",
            "C",
            "D 0",
        ]
        .map(str::to_owned),
    );
    let input = out.join("cache-input.txt");
    fs::write(&input, ops.join("\n") + "\n").unwrap();
    let mut traces = Vec::new();
    for version in ["uncompressed_executable", "uncompressed_executable_update"] {
        let result = Command::new(helper)
            .arg(supplied.join(version))
            .args(names.map(|n| out.join(format!("{n}.fnt"))))
            .stdin(fs::File::open(&input).unwrap())
            .output()
            .unwrap();
        fs::write(out.join(format!("{version}-cache.trace")), &result.stdout).unwrap();
        fs::write(out.join(format!("{version}-cache.log")), &result.stderr).unwrap();
        assert!(
            result.status.success(),
            "native cache: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let trace = String::from_utf8(result.stdout).unwrap();
        let rows: Vec<_> = trace.lines().collect();
        assert_eq!(rows.len(), ops.len());
        let mut residents: Residency<(u32, u32), ()> = Residency::default();
        let mut owners: BTreeMap<u32, ((u32, u32), Arc<()>)> = BTreeMap::new();
        let mut retired = 0;
        for (op, row) in ops.iter().zip(rows) {
            let args: Vec<_> = op.split_whitespace().collect();
            let vals: Vec<_> = row.split_whitespace().collect();
            let number = |i: usize| args[i].parse::<u32>().unwrap();
            match args[0] {
                "A" => residents.set_age(number(1)),
                "D" => {
                    drop(owners.remove(&number(1)).unwrap());
                    assert_eq!(
                        vals[1].parse::<usize>().unwrap(),
                        residents.snapshot().len()
                    );
                    assert_eq!(vals[2].parse::<usize>().unwrap(), retired);
                }
                "F" => {
                    residents.fail();
                    assert_eq!(
                        vals[1].parse::<usize>().unwrap(),
                        residents.snapshot().len()
                    );
                }
                "L" => {
                    let key = (
                        fonts[number(1) as usize].get_character_table_crc(),
                        number(2),
                    );
                    let before = residents.snapshot();
                    let new = !before.contains_key(&key);
                    let value = if new {
                        let value = Arc::new(());
                        retired += residents.insert(key, value.clone()).len();
                        value
                    } else {
                        let value = residents.test_owner(key);
                        residents.touch(key);
                        value
                    };
                    owners.insert(number(3), (key, value));
                    let snapshot = residents.snapshot();
                    assert_eq!(vals[3].parse::<usize>().unwrap(), usize::from(new), "{op}");
                    assert_eq!(vals[4].parse::<usize>().unwrap(), snapshot.len(), "{op}");
                    assert_eq!(vals[5].parse::<usize>().unwrap(), snapshot[&key].0, "{op}");
                    assert_eq!(vals[6].parse::<u32>().unwrap(), snapshot[&key].1, "{op}");
                }
                "C" => {
                    let values: Vec<_> = vals[2..]
                        .iter()
                        .map(|v| v.parse::<u32>().unwrap())
                        .collect();
                    let native: BTreeMap<_, _> = values
                        .chunks_exact(4)
                        .map(|v| ((v[0], v[1]), (v[2] as usize, v[3])))
                        .collect();
                    assert_eq!(
                        native,
                        residents.snapshot(),
                        "native complete resident keys, owners and ages"
                    );
                }
                _ => unreachable!(),
            }
        }
        assert_eq!(retired, 66);
        traces.push(trace);
    }
    assert_eq!(traces[0], traces[1]);
    println!(
        "Native cache lifecycle:{} operations/version; pinned pressure, warm reuse, cold failure,66 evictions and age renumbering match production residency policy",
        ops.len()
    );
}
