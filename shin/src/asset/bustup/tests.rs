use super::*;

#[test]
fn native_descriptor_ranges_preserve_duplicates_and_empty_singleton_cases() {
    for (frames, animated, static_frames) in [
        (vec![], vec![], vec![]),
        (vec![40], vec![], vec![40]),
        (vec![40, 41], vec![41], vec![40]),
        (vec![40, 41, 41, 43], vec![41, 41, 43], vec![40]),
    ] {
        assert_eq!(
            animation_frames(frames.clone(), false).collect::<Vec<_>>(),
            animated
        );
        assert_eq!(
            animation_frames(frames, true).collect::<Vec<_>>(),
            static_frames
        );
    }
}
#[test]
fn native_character_metadata_is_signed_sixteen_bit() {
    for (id, expected) in [
        (0, 0),
        (19, 19),
        (32767, 32767),
        (32768, -32768),
        (65535, -1),
    ] {
        assert_eq!(CharacterId::from_metadata(id).raw(), expected);
    }
}

#[test]
#[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
fn reference_native_static_vs_animated_bustup_descriptor_ranges() {
    let rom = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let exe = std::fs::read(
        std::path::Path::new(&rom)
            .parent()
            .unwrap()
            .join("uncompressed_executable"),
    )
    .unwrap();
    for (address, expected) in [
        (0x20800, 0x3900d3ffu32),
        (0x20804, 0x79c022a8),
        (0x20808, 0xb90033e8),
        (0x210ec, 0x9100c3e3),
        (0x210f4, 0x94018733),
        (0x3adf4, 0x79c02008),
        (0x3adfc, 0x320003e8),
        (0x3ae00, 0x381cc3a8),
        (0x3548c, 0xb94002a8),
        (0x357d8, 0xf94003e8),
        (0x357dc, 0x39401108),
        (0x357f4, 0x7100011f),
        (0x357f8, 0x1a9f06da),
        (0x357fc, 0x5200011c),
        (0x35800, 0x4b1c0348),
        (0x35838, 0x6b1c035f),
        (0x358dc, 0xf94003e8),
        (0x358e0, 0x39401108),
        (0x358ec, 0x7100011f),
        (0x358f0, 0x52000116),
        (0x358f4, 0x1a9f06ba),
        (0x358f8, 0x4b160348),
        (0x35930, 0x6b16035f),
    ] {
        let actual = u32::from_le_bytes(exe[address + 256..address + 260].try_into().unwrap());
        assert_eq!(actual, expected, "native{address:x}");
    }
}

// Inspect every original descriptor without invoking the GPU builder's range
// helper or decoding unrelated expressions. Each promise still passes through
// the authoritative BUP parser, preserving authored duplicates and order.
struct LayoutBuilder;
struct Layout {
    expressions: IndexMap<String, (Vec<BustupBlockId>, Vec<BustupBlockId>)>,
}
impl BustupBuilder for LayoutBuilder {
    type Args = ();
    type Skeleton<'a> = Layout;
    type BlockType = ();
    type Output = Layout;
    fn new<'a>(_: &(), skeleton: BustupSkeleton<'a>) -> Layout {
        Layout {
            expressions: skeleton
                .expressions
                .into_iter()
                .map(|(name, expr)| {
                    (
                        name,
                        (
                            expr.mouth_blocks.into_iter().map(|p| p.get_id()).collect(),
                            expr.eye_blocks.into_iter().map(|p| p.get_id()).collect(),
                        ),
                    )
                })
                .collect(),
        }
    }
    fn new_block(_: &(), _: u32, _: PicBlock) -> Result<()> {
        panic!("Layout must not decode any blocks")
    }
    fn build(_: &(), layout: Layout, _: BustupBlockPromiseToken<()>) -> Result<Layout> {
        Ok(layout)
    }
}

#[test]
#[ignore = "requires supplied ROM and GPU (SHIN_REFERENCE_ROM)"]
fn reference_original_bustup_asset_ranges_cache_identity_and_argument_separation() {
    use crate::asset::system::{AssetServer, LayeredAssetIo};
    use shin_core::format::{rom::RomReader, scenario::Scenario};
    use shin_core::primitives::stateless_reader::StatelessFile;
    use std::io::Read;
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) = shin_tasks::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::SPIRV_SHADER_PASSTHROUGH,
            ..Default::default()
        },
        None,
    ))
    .unwrap();
    let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(&path).unwrap();
    let server = AssetServer::new(
        io.into(),
        AssetLoadContext {
            wgpu_device: device,
            wgpu_queue: queue,
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    );
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(path).unwrap()).unwrap()).unwrap();
    let mut read = |name: &str| {
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file(name).unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    let scenario = Scenario::new(read("/main.snr").into()).unwrap();
    let info = scenario
        .info_tables()
        .bustup_info
        .iter()
        .find(|i| i.lipsync_character_id == 19)
        .unwrap();
    let layout =
        shin_core::format::bustup::read_bustup::<LayoutBuilder>(&read(&info.path()), ()).unwrap();
    let (mouth_ids, eye_ids) = &layout.expressions[info.emotion.as_str()];
    assert!(
        mouth_ids.len() >= 4,
        "Original portrait must cover static and three animated mouths"
    );
    let mut loaded = Vec::new();
    for disable_animations in [false, true] {
        let args = BustupArgs {
            expression: info.emotion.to_string(),
            character_id: CharacterId::from_metadata(info.lipsync_character_id),
            disable_animations,
        };
        let asset =
            shin_tasks::block_on(server.load_with_args::<Bustup, _>(info.path(), args.clone()))
                .unwrap();
        let cached =
            shin_tasks::block_on(server.load_with_args::<Bustup, _>(info.path(), args)).unwrap();
        assert!(Arc::ptr_eq(&asset, &cached));
        for (ids, blocks) in [
            (mouth_ids, &asset.mouth_blocks),
            (eye_ids, &asset.eye_blocks),
        ] {
            let expected = if disable_animations {
                &ids[..ids.len().min(1)]
            } else {
                &ids[ids.len().min(1)..]
            };
            assert_eq!(blocks.len(), expected.len());
            for (&id, block) in expected.iter().zip(blocks) {
                match server.context().bustup_cache.lookup(id) {
                    CacheLookupResult::Loaded(expected) => assert!(Arc::ptr_eq(block, &expected)),
                    CacheLookupResult::Loading(_) => {
                        panic!("Awaited asset retained a loading block")
                    }
                    CacheLookupResult::LoadRequired(handle) => {
                        server
                            .context()
                            .bustup_cache
                            .finish_load(handle, block.clone());
                        panic!("Selected block does not match its original descriptor");
                    }
                }
            }
        }
        loaded.push(asset);
    }
    assert!(
        !Arc::ptr_eq(&loaded[0], &loaded[1]),
        "Static and animated assets must not alias"
    );
    for (a, b) in loaded[0].base_blocks.iter().zip(&loaded[1].base_blocks) {
        assert!(Arc::ptr_eq(a, b));
    }
    assert_eq!(loaded[0].character_id.raw(), 19);
    eprintln!(
        "Original BUP: {} mouth descriptors -> {} animated/{} static frames; {} eye descriptors; exact descriptor/cache identities and separate asset keys pass",
        mouth_ids.len(),
        loaded[0].mouth_blocks.len(),
        loaded[1].mouth_blocks.len(),
        eye_ids.len()
    );
}
