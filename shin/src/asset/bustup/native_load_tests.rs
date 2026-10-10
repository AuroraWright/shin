use super::*;
use crate::asset::picture::vertex_data;
use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::Weak,
};

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
fn raw_id(id: BustupBlockId) -> u32 {
    u32::from_str_radix(&id.to_string()[6..], 16).unwrap()
}
struct DecodedBlock {
    block: PicBlock,
    vertices: Vec<u8>,
    indices: Vec<u8>,
    hashes: [u64; 3],
}
impl DecodedBlock {
    fn new(block: PicBlock) -> Self {
        let (vertices, indices) = vertex_data(&block);
        let vertices: Vec<_> = vertices
            .into_iter()
            .flat_map(|v| {
                let position = v.position;
                let texture_position = v.texture_position;
                position
                    .to_array()
                    .into_iter()
                    .chain(texture_position.to_array())
                    .flat_map(f32::to_le_bytes)
            })
            .collect();
        let indices: Vec<_> = indices.into_iter().flat_map(u16::to_le_bytes).collect();
        let hashes = [hash(&vertices), hash(&indices), hash(block.data.as_raw())];
        Self {
            block,
            vertices,
            indices,
            hashes,
        }
    }
}
enum Promise<'a> {
    Ready(Arc<DecodedBlock>),
    Decode(BustupBlockPromise<'a>),
    Shared(BustupBlockId),
    Empty,
}
type BlockCache = std::sync::Mutex<HashMap<BustupBlockId, Weak<DecodedBlock>>>;
struct SelectionArgs<'a> {
    cache: &'a BlockCache,
    expression: &'a str,
    disabled: bool,
}
struct SelectionSkeleton<'a> {
    header: [u32; 2],
    blocks: Vec<(u32, u32, BustupBlockId, Promise<'a>)>,
}
struct Selection {
    header: [u32; 2],
    blocks: Vec<(u32, u32, BustupBlockId, Option<Arc<DecodedBlock>>)>,
}
struct SelectionBuilder<'a>(PhantomData<&'a ()>);
impl<'b> BustupBuilder for SelectionBuilder<'b> {
    type Args = SelectionArgs<'b>;
    type Skeleton<'a> = SelectionSkeleton<'a>;
    type BlockType = Arc<DecodedBlock>;
    type Output = Selection;
    fn new<'a>(args: &Self::Args, skeleton: BustupSkeleton<'a>) -> Self::Skeleton<'a> {
        let header = [
            u32::from(skeleton.origin_x as u16) | (u32::from(skeleton.origin_y as u16) << 16),
            u32::from(skeleton.effective_width) | (u32::from(skeleton.effective_height) << 16),
        ];
        let mut entries = Vec::new();
        let mut requested = std::collections::HashSet::new();
        let mut add = |kind, index, promise: Option<BustupBlockPromise<'a>>| {
            let Some(promise) = promise else {
                entries.push((kind, index, BustupBlockId::new(0), Promise::Empty));
                return;
            };
            let id = promise.get_id();
            let state =
                if let Some(block) = args.cache.lock().unwrap().get(&id).and_then(Weak::upgrade) {
                    Promise::Ready(block)
                } else if requested.insert(id) {
                    Promise::Decode(promise)
                } else {
                    Promise::Shared(id)
                };
            entries.push((kind, index, id, state));
        };
        for (i, block) in skeleton.base_blocks.into_iter().enumerate() {
            add(0, i as u32, Some(block));
        }
        let expression = skeleton
            .expressions
            .into_iter()
            .find(|(name, _)| name == args.expression)
            .unwrap()
            .1;
        add(1, 0, expression.face1);
        add(2, 0, expression.face2);
        for (i, block) in animation_frames(expression.mouth_blocks, args.disabled).enumerate() {
            add(3, i as u32, Some(block));
        }
        for (i, block) in animation_frames(expression.eye_blocks, args.disabled).enumerate() {
            add(4, i as u32, Some(block));
        }
        SelectionSkeleton {
            header,
            blocks: entries,
        }
    }
    fn new_block(_: &Self::Args, _: u32, block: PicBlock) -> Result<Self::BlockType> {
        Ok(Arc::new(DecodedBlock::new(block)))
    }
    fn build(
        args: &Self::Args,
        skeleton: Self::Skeleton<'_>,
        token: BustupBlockPromiseToken<Self::BlockType>,
    ) -> Result<Selection> {
        let blocks = skeleton
            .blocks
            .into_iter()
            .map(|(kind, index, id, promise)| {
                let block = match promise {
                    Promise::Ready(block) => Some(block),
                    Promise::Decode(promise) => {
                        let block = promise.get(&token).clone();
                        args.cache
                            .lock()
                            .unwrap()
                            .insert(id, Arc::downgrade(&block));
                        Some(block)
                    }
                    Promise::Shared(id) => Some(
                        args.cache
                            .lock()
                            .unwrap()
                            .get(&id)
                            .and_then(Weak::upgrade)
                            .unwrap(),
                    ),
                    Promise::Empty => None,
                };
                (kind, index, id, block)
            })
            .collect();
        Ok(Selection {
            header: skeleton.header,
            blocks,
        })
    }
}

struct NativeLoader {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    trace: std::fs::File,
    case: usize,
    version: &'static str,
    jobs: u64,
    uploaded: u64,
    pixels: u64,
    vertices: u64,
    indices: u64,
    references: u64,
}
impl NativeLoader {
    fn new(helper: &Path, exe: &Path, rom: &Path, out: &Path, version: &'static str) -> Self {
        let mut command = Command::new(helper);
        command
            .arg(exe)
            .arg(rom)
            .env("SHIN_NATIVE_BUSTUP_STREAM", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if std::env::var_os("SHIN_NATIVE_BUSTUP_REVERSE_JOBS").is_some() {
            command.env("SHIN_NATIVE_BUSTUP_REVERSE_JOBS", "1");
        }
        let mut child = command.spawn().unwrap();
        Self {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            trace: std::fs::File::create(out.join(format!("{version}.trace"))).unwrap(),
            case: 0,
            version,
            jobs: 0,
            uploaded: 0,
            pixels: 0,
            vertices: 0,
            indices: 0,
            references: 0,
        }
    }
    fn row(&mut self) -> Vec<String> {
        let mut row = String::new();
        assert_ne!(
            self.output.read_line(&mut row).unwrap(),
            0,
            "{} native BUP ended at case{}",
            self.version,
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
    fn compare(&mut self, path: &str, expression: &str, disabled: bool, selection: &Selection) {
        // Preserve authored empty names and trailing spaces across the native
        // line protocol. '-' represents empty; other names are UTF-8 hex.
        let expression_hex = if expression.is_empty() {
            "-".to_owned()
        } else {
            expression
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        writeln!(
            self.input.as_mut().unwrap(),
            "{path} {expression_hex} {}",
            u8::from(disabled)
        )
        .unwrap();
        self.input.as_mut().unwrap().flush().unwrap();
        let row = self.row();
        let label = format!("{}:{path}:{expression}:{disabled}", self.version);
        assert_eq!(
            &row[..5],
            [
                "U",
                &self.case.to_string(),
                path,
                &expression_hex,
                &u8::from(disabled).to_string()
            ],
            "{label}"
        );
        let values: Vec<u32> = row[5..].iter().map(|v| v.parse().unwrap()).collect();
        assert_eq!(&values[..2], selection.header, "{label}: header");
        assert_eq!(values[2], 1);
        assert_eq!(values[4], values[3] + 3, "{label}: native request reads");
        assert_eq!(
            values[6] as usize,
            selection.blocks.len(),
            "{label}: selected native references"
        );
        self.jobs += u64::from(values[3]);
        self.references += selection.blocks.len() as u64;
        for (kind, index, id, block) in &selection.blocks {
            let row = self.row();
            assert_eq!(
                &row[1..5],
                [
                    self.case.to_string(),
                    kind.to_string(),
                    index.to_string(),
                    raw_id(*id).to_string()
                ],
                "{label}: ordered selected block"
            );
            let renderable = block.as_ref().filter(|b| {
                !b.block.opaque_rects.is_empty() || !b.block.transparent_rects.is_empty()
            });
            let Some(block) = renderable else {
                assert_eq!(row[0], "E", "{label}: native null block");
                continue;
            };
            assert_eq!(row[0], "B", "{label}: expected renderable block");
            assert_eq!(row[5], block.block.data.width().to_string());
            assert_eq!(row[6], block.block.data.height().to_string());
            assert_eq!(row[7], block.vertices.len().to_string());
            assert_eq!(row[8], block.indices.len().to_string());
            for (actual, expected) in row[9..12].iter().zip(block.hashes) {
                assert_eq!(
                    u64::from_str_radix(actual, 16).unwrap(),
                    expected,
                    "{label}:{kind}:{index}: block bytes"
                );
            }
            match row[12].as_str() {
                "1" => {
                    assert_eq!(
                        self.bytes(block.vertices.len()),
                        block.vertices,
                        "{label}:{kind}:{index}: native vertices"
                    );
                    assert_eq!(
                        self.bytes(block.indices.len()),
                        block.indices,
                        "{label}:{kind}:{index}: native indices"
                    );
                    assert_eq!(
                        self.bytes(block.block.data.as_raw().len()),
                        *block.block.data.as_raw(),
                        "{label}:{kind}:{index}: native RGBA"
                    );
                    self.uploaded += 1;
                    self.pixels +=
                        u64::from(block.block.data.width()) * u64::from(block.block.data.height());
                    self.vertices += block.vertices.len() as u64;
                    self.indices += block.indices.len() as u64;
                }
                "0" => {}
                other => panic!("Unknown native allocation marker {other}"),
            }
        }
        assert_eq!(self.row()[..2], ["D", &self.case.to_string()]);
        self.case += 1;
    }
    fn finish(mut self) {
        self.input.take();
        assert_eq!(self.row(), vec!["Z".to_owned(), self.case.to_string()]);
        assert!(self.child.wait().unwrap().success());
        println!(
            "{} native BUP: {} cases,{} jobs,{} selected references,{} allocated GPU blocks,{} exact RGBA pixels,{} vertex bytes,{} index bytes",
            self.version,
            self.case,
            self.jobs,
            self.references,
            self.uploaded,
            self.pixels,
            self.vertices,
            self.indices
        );
    }
}

#[test]
#[ignore = "requires supplied original/1.03 executables and ROM; full native BUP CPU loading, no window/audio"]
fn reference_native_complete_bustup_loading() {
    struct Names;
    impl BustupBuilder for Names {
        type Args = ();
        type Skeleton<'a> = Vec<String>;
        type BlockType = ();
        type Output = Vec<String>;
        fn new<'a>(_: &(), s: BustupSkeleton<'a>) -> Vec<String> {
            s.expressions.into_keys().collect()
        }
        fn new_block(_: &(), _: u32, _: PicBlock) -> Result<()> {
            panic!("Names must not decode blocks")
        }
        fn build(_: &(), s: Vec<String>, _: BustupBlockPromiseToken<()>) -> Result<Vec<String>> {
            Ok(s)
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let rom_path = PathBuf::from(std::env::var("SHIN_REFERENCE_ROM").unwrap());
    let files = rom_path.parent().unwrap();
    let out = std::env::var_os("SHIN_NATIVE_BUSTUP_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("shin-native-bustup-{}", std::process::id()))
        });
    std::fs::create_dir_all(&out).unwrap();
    let helper = out.join("load");
    assert!(
        Command::new("clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-bustup-load.c"))
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(&rom_path).unwrap()).unwrap())
            .unwrap();
    let mut paths: Vec<_> = rom
        .traverse()
        .map(|(p, _)| p)
        .filter(|p| p.ends_with(".bup"))
        .collect();
    assert_eq!(paths.len(), 581);
    let patch = RomReader::new(
        StatelessFile::new(std::fs::File::open(files.join("patch.rom")).unwrap()).unwrap(),
    )
    .unwrap();
    assert!(
        !patch.traverse().any(|(p, _)| p.ends_with(".bup")),
        "Updated BUP replacements need their own native file reads"
    );
    let filter = std::env::var("SHIN_NATIVE_BUSTUP_FILTER").ok();
    if let Some(filter) = &filter {
        paths.retain(|p| p.contains(filter));
        assert!(!paths.is_empty());
    }
    let cache = BlockCache::default();
    let mut previous = None;
    let mut original = NativeLoader::new(
        &helper,
        &files.join("uncompressed_executable"),
        &rom_path,
        &out,
        "original",
    );
    let mut updated = NativeLoader::new(
        &helper,
        &files.join("uncompressed_executable_update"),
        &rom_path,
        &out,
        "1.03",
    );
    let start = std::time::Instant::now();
    let mut next_progress = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut expressions = 0;
    let mut cases = 0;
    for path in &paths {
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file(path).unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let names = shin_core::format::bustup::read_bustup::<Names>(&bytes, ()).unwrap();
        expressions += names.len();
        for name in names {
            for disabled in [false, true] {
                let selection = shin_core::format::bustup::read_bustup::<SelectionBuilder>(
                    &bytes,
                    SelectionArgs {
                        cache: &cache,
                        expression: &name,
                        disabled,
                    },
                )
                .unwrap();
                original.compare(path, &name, disabled, &selection);
                updated.compare(path, &name, disabled, &selection);
                previous = Some(selection);
                cases += 1;
                if std::time::Instant::now() >= next_progress {
                    eprintln!(
                        "Native BUP {cases} cases at{path}, {:.1}s",
                        start.elapsed().as_secs_f64()
                    );
                    next_progress = std::time::Instant::now() + std::time::Duration::from_secs(10);
                }
            }
        }
    }
    drop(previous);
    assert!(
        cache
            .lock()
            .unwrap()
            .values()
            .all(|weak| weak.upgrade().is_none()),
        "CPU cache retained unreferenced portrait data"
    );
    if filter.is_none() {
        assert_eq!(expressions, 8282);
        assert_eq!(cases, 16564);
    }
    original.finish();
    updated.finish();
    println!(
        "Native BUP complete: {} files,{expressions} expressions, {cases} animated/static cases; {:.3}s; evidence {}",
        paths.len(),
        start.elapsed().as_secs_f64(),
        out.display()
    );
}
