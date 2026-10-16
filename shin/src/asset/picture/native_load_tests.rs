use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use anyhow::Result;
use shin_core::{
    format::{
        picture::{PicBlock, PictureBuilder, read_picture},
        rom::RomReader,
    },
    primitives::stateless_reader::StatelessFile,
};

pub(super) struct DecodedPicture {
    header: [u32; 5],
    pub(super) blocks: BTreeMap<u32, (Vec<(u32, u32)>, PicBlock)>,
}

impl PictureBuilder for DecodedPicture {
    type Args = ();
    type Output = Self;
    fn new(_: (), width: u32, height: u32, x: i32, y: i32, id: u32) -> Self {
        Self {
            header: [
                x as u16 as u32 | ((y as u16 as u32) << 16),
                width | (height << 16),
                1,
                1.0f32.to_bits(),
                id,
            ],
            blocks: BTreeMap::new(),
        }
    }
    fn add_block(
        &mut self,
        offset: u32,
        positions: Vec<(u32, u32)>,
        block: PicBlock,
    ) -> Result<()> {
        assert!(self.blocks.insert(offset, (positions, block)).is_none());
        Ok(())
    }
    fn build(self) -> Result<Self> {
        Ok(self)
    }
}

struct NativeLoader {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    trace: std::fs::File,
    dir: PathBuf,
    case: usize,
}

impl NativeLoader {
    fn new(helper: &Path, exe: &Path, rom: &Path, dir: PathBuf) -> Self {
        std::fs::create_dir_all(&dir).unwrap();
        let mut command = Command::new(helper);
        command
            .arg(exe)
            .arg(rom)
            .env("SHIN_NATIVE_PICTURE_STREAM", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if std::env::var_os("SHIN_NATIVE_PICTURE_REVERSE_JOBS").is_some() {
            command.env("SHIN_NATIVE_PICTURE_REVERSE_JOBS", "1");
        }
        let mut child = command.spawn().unwrap();
        Self {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()),
            trace: std::fs::File::create(dir.join("trace.txt")).unwrap(),
            child,
            dir,
            case: 0,
        }
    }
    fn row(&mut self) -> Vec<String> {
        let mut row = String::new();
        assert_ne!(
            self.output.read_line(&mut row).unwrap(),
            0,
            "Native loader ended early in {} at case {}",
            self.dir.display(),
            self.case
        );
        self.trace.write_all(row.as_bytes()).unwrap();
        row.split_whitespace().map(str::to_owned).collect()
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        let mut bytes = vec![0; n];
        self.output.read_exact(&mut bytes).unwrap();
        bytes
    }
    fn compare(&mut self, path: &str, bytes: &[u8], decoded: &DecodedPicture) {
        writeln!(self.input.as_mut().unwrap(), "{path}").unwrap();
        self.input.as_mut().unwrap().flush().unwrap();
        let row = self.row();
        assert_eq!(&row[..3], ["P", &self.case.to_string(), path]);
        let values = row[3..]
            .iter()
            .map(|s| s.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(&values[..5], decoded.header, "{path}: native header");
        let count = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as usize;
        assert_eq!(values[5] as usize, count);
        assert_eq!(values[6] as usize, decoded.blocks.len());
        assert_eq!(values[7], values[6] + 2);
        let expected_positions = bytes[36..36 + count * 12]
            .chunks_exact(12)
            .flat_map(|desc| {
                let x = u16::from_le_bytes(desc[..2].try_into().unwrap()) as f32;
                let y = u16::from_le_bytes(desc[2..4].try_into().unwrap()) as f32;
                x.to_le_bytes()
                    .into_iter()
                    .chain(y.to_le_bytes())
                    .chain(desc[4..].iter().copied())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            self.bytes(count * 16),
            expected_positions,
            "{path}: ordered placements"
        );
        for (&offset, (positions, block)) in &decoded.blocks {
            let row = self.row();
            assert_eq!(row[1], self.case.to_string());
            assert_eq!(row[2], offset.to_string());
            let expected_group = bytes[36..36 + count * 12]
                .chunks_exact(12)
                .filter(|desc| u32::from_le_bytes(desc[4..8].try_into().unwrap()) == offset)
                .map(|desc| {
                    (
                        u16::from_le_bytes(desc[..2].try_into().unwrap()) as u32,
                        u16::from_le_bytes(desc[2..4].try_into().unwrap()) as u32,
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                *positions, expected_group,
                "{path}:{offset}: grouped placements"
            );
            if row[0] == "E" {
                assert!(block.opaque_rects.is_empty() && block.transparent_rects.is_empty());
                continue;
            }
            assert_eq!(row[0], "B");
            assert_eq!(row[3], block.data.width().to_string());
            assert_eq!(row[4], block.data.height().to_string());
            let (vertices, indices) = super::vertex_data(block);
            let actual_vertices = vertices
                .iter()
                .flat_map(|v| {
                    let position = v.position;
                    let texture_position = v.texture_position;
                    position
                        .to_array()
                        .into_iter()
                        .chain(texture_position.to_array())
                        .flat_map(f32::to_le_bytes)
                })
                .collect::<Vec<_>>();
            let actual_indices = indices
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>();
            assert_eq!(row[5], actual_vertices.len().to_string());
            assert_eq!(row[6], actual_indices.len().to_string());
            let native_vertices = self.bytes(actual_vertices.len());
            if native_vertices != actual_vertices {
                let at = native_vertices
                    .iter()
                    .zip(&actual_vertices)
                    .position(|(a, b)| a != b)
                    .unwrap()
                    / 4
                    * 4;
                panic!(
                    "{path}:{offset} vertex byte {at}: native {:08x} != port {:08x}",
                    u32::from_le_bytes(native_vertices[at..at + 4].try_into().unwrap()),
                    u32::from_le_bytes(actual_vertices[at..at + 4].try_into().unwrap())
                );
            }
            assert_eq!(
                self.bytes(actual_indices.len()),
                actual_indices,
                "{path}:{offset}: indices"
            );
            let native_pixels = self.bytes(block.data.as_raw().len());
            if let Some(at) = native_pixels
                .iter()
                .zip(block.data.as_raw())
                .position(|(a, b)| a != b)
            {
                std::fs::write(self.dir.join("mismatch-native.bin"), &native_pixels).unwrap();
                std::fs::write(self.dir.join("mismatch-port.bin"), block.data.as_raw()).unwrap();
                panic!(
                    "{} {path}:{offset} RGBA byte {at}: native {} != port {}; buffers in {}",
                    self.dir.display(),
                    native_pixels[at],
                    block.data.as_raw()[at],
                    self.dir.display()
                );
            }
        }
        assert_eq!(self.row(), ["D", &self.case.to_string(), path]);
        self.case += 1;
    }
    fn finish(&mut self) {
        self.input.take();
        assert!(self.child.wait().unwrap().success());
        println!(
            "{}: {} complete native picture loads/cache hits/destructions",
            self.dir.display(),
            self.case
        );
    }
}
impl Drop for NativeLoader {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[ignore = "requires original/1.03 native executables and supplied ROMs; CPU only"]
fn reference_native_complete_picture_loading() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let files = root.join("umineko_switch_files");
    let out = std::env::var_os("SHIN_NATIVE_PICTURE_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("shin-native-picture-load-{}", std::process::id()))
        });
    std::fs::create_dir_all(&out).unwrap();
    let helper = out.join("load");
    assert!(
        Command::new("clang")
            .args(["-O2", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-picture-load.c"))
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    let filter = std::env::var("SHIN_NATIVE_PICTURE_FILTER").ok();
    let mut total_blocks = 0usize;
    let mut total_pixels = 0u64;
    for archive in ["data.rom", "patch.rom"] {
        let rom_path = files.join(archive);
        let mut rom =
            RomReader::new(StatelessFile::new(std::fs::File::open(&rom_path).unwrap()).unwrap())
                .unwrap();
        let paths = rom
            .traverse()
            .map(|(path, _)| path)
            .filter(|path| path.ends_with(".pic"))
            .filter(|path| filter.as_ref().is_none_or(|f| path == f))
            .collect::<Vec<_>>();
        if paths.is_empty() {
            continue;
        }
        let mut loaders = vec![NativeLoader::new(
            &helper,
            &files.join("uncompressed_executable_update"),
            &rom_path,
            out.join(format!("1.03-{archive}")),
        )];
        if archive == "data.rom" {
            loaders.push(NativeLoader::new(
                &helper,
                &files.join("uncompressed_executable"),
                &rom_path,
                out.join(format!("original-{archive}")),
            ));
        }
        for (index, path) in paths.iter().enumerate() {
            let file = rom.find_file(path).unwrap();
            let mut bytes = Vec::new();
            rom.open_file(file)
                .unwrap()
                .read_to_end(&mut bytes)
                .unwrap();
            let mut picture = read_picture::<DecodedPicture>(&bytes, ()).unwrap();
            if std::env::var_os("SHIN_NATIVE_PICTURE_SIGNED_OFFSETS").is_some() {
                for (_, block) in picture.blocks.values_mut() {
                    block.offset_x = 0xfffe;
                    block.offset_y = 0x8000;
                }
            }
            for loader in &mut loaders {
                loader.compare(path, &bytes, &picture);
            }
            total_blocks += picture.blocks.len();
            total_pixels += picture
                .blocks
                .values()
                .map(|(_, b)| u64::from(b.data.width()) * u64::from(b.data.height()))
                .sum::<u64>();
            if index % 100 == 0 {
                println!(
                    "{archive}: compared {index}/{} picture constructors",
                    paths.len()
                );
            }
        }
        for loader in &mut loaders {
            loader.finish();
        }
    }
    assert_ne!(total_blocks, 0);
    println!(
        "Compared {total_blocks} unique picture blocks and {total_pixels} decoded RGBA texels. Evidence: {}",
        out.display()
    );
}
