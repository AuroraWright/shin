use super::*;
use crate::asset::system::{AssetServer, LayeredAssetIo};
use shin_core::{
    format::{picture::read_picture_header, rom::RomReader},
    primitives::stateless_reader::StatelessFile,
};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn context(device: &wgpu::Device, queue: &wgpu::Queue) -> AssetLoadContext {
    AssetLoadContext {
        game: crate::cli::Game::Umineko,
        higurashi_bustup_cache: Default::default(),
        wgpu_device: device.clone(),
        wgpu_queue: queue.clone(),
        bustup_cache: crate::asset::system::cache::AssetCache::new(),
        picture_cache: Default::default(),
        font_cache: Default::default(),
    }
}

#[test]
#[ignore = "requires supplied ROMs/executables and Metal; no window or audio"]
fn reference_native_picture_aliases_and_production_gpu_cache() {
    crate::init_test_task_pools();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let rom_path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
    let files = Path::new(&rom_path).parent().unwrap();
    let out = std::env::var_os("SHIN_PICTURE_CACHE_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("shin-picture-cache-{}", std::process::id()))
        });
    std::fs::create_dir_all(&out).unwrap();
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(&rom_path).unwrap()).unwrap())
            .unwrap();
    let paths: Vec<_> = rom
        .traverse()
        .map(|(path, _)| path)
        .filter(|path| path.ends_with(".pic"))
        .collect();
    assert_eq!(paths.len(), 2601);
    let mut groups = BTreeMap::<u32, Vec<String>>::new();
    for path in paths {
        let mut cursor = rom.open_file(rom.find_file(&path).unwrap()).unwrap();
        let size = cursor.seek(SeekFrom::End(0)).unwrap();
        cursor.seek(SeekFrom::Start(0)).unwrap();
        let mut prefix = [0; 36];
        cursor.read_exact(&mut prefix).unwrap();
        let header = read_picture_header_prefix(&prefix, size).unwrap();
        groups.entry(header.picture_id).or_default().push(path);
    }
    let mut patch = RomReader::new(
        StatelessFile::new(std::fs::File::open(files.join("patch.rom")).unwrap()).unwrap(),
    )
    .unwrap();
    let patch_paths: Vec<_> = patch
        .traverse()
        .map(|(p, _)| p)
        .filter(|p| p.ends_with(".pic"))
        .collect();
    assert_eq!(patch_paths.len(), 3);
    for path in patch_paths {
        let mut bytes = Vec::new();
        patch
            .open_file(patch.find_file(&path).unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert!(
            !groups.contains_key(&read_picture_header(&bytes).unwrap().picture_id),
            "Updated PIC collides with base cache ID: {path}"
        );
    }
    groups.retain(|_, aliases| aliases.len() > 1);
    assert_eq!(groups.len(), 32);
    assert_eq!(groups.values().map(Vec::len).sum::<usize>(), 76);
    let mut read = |path: &str| {
        let mut bytes = Vec::new();
        rom.open_file(rom.find_file(path).unwrap())
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    let mut compared_bytes = 0usize;
    for paths in groups.values() {
        let expected = read(&paths[0]);
        for path in &paths[1..] {
            let bytes = read(path);
            compared_bytes += bytes.len();
            assert_eq!(bytes, expected, "Same-ID supplied aliases differ: {path}");
        }
    }
    let group_text: String = groups
        .values()
        .map(|paths| paths.join(" ") + "\n")
        .collect();
    std::fs::write(out.join("groups.txt"), &group_text).unwrap();
    let helper = out.join("native-cache");
    assert!(
        Command::new("clang")
            .args(["-O0", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function"])
            .arg(root.join("reverse-engineering/native-picture-cache.c"))
            .arg("-o")
            .arg(&helper)
            .status()
            .unwrap()
            .success()
    );
    let mut native_jobs = 0usize;
    for (version, executable) in [
        ("original", "uncompressed_executable"),
        ("1.03", "uncompressed_executable_update"),
    ] {
        for metadata in [false, true] {
            let mut command = Command::new(&helper);
            command
                .arg(files.join(executable))
                .arg(&rom_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if metadata {
                command.env("SHIN_NATIVE_PICTURE_ALIAS_METADATA", "1");
            }
            let mut child = command.spawn().unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(group_text.as_bytes())
                .unwrap();
            let result = child.wait_with_output().unwrap();
            let label = format!("{version}-metadata-{}", u8::from(metadata));
            std::fs::write(out.join(format!("{label}.trace")), &result.stdout).unwrap();
            std::fs::write(out.join(format!("{label}.stderr")), &result.stderr).unwrap();
            assert!(
                result.status.success(),
                "{label}: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            let text = String::from_utf8(result.stdout).unwrap();
            assert_eq!(text.lines().count(), groups.len());
            for ((id, paths), (index, line)) in groups.iter().zip(text.lines().enumerate()) {
                let fields: Vec<_> = line.split_whitespace().collect();
                assert_eq!(fields[0], "A");
                let values: Vec<usize> = fields[1..].iter().map(|v| v.parse().unwrap()).collect();
                assert_eq!(
                    &values[..4],
                    &[index, *id as usize, paths.len(), paths.len() - 1]
                );
                assert_eq!(values[5], usize::from(metadata));
                native_jobs += values[4] * 2; // cold load and reload, once each
            }
        }
        let mut child = Command::new(&helper)
            .arg(files.join(executable))
            .arg(&rom_path)
            .env("SHIN_NATIVE_PICTURE_CACHE_MUTANT", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all((group_text.lines().next().unwrap().to_owned() + "\n").as_bytes())
            .unwrap();
        let mutant = child.wait_with_output().unwrap();
        std::fs::write(
            out.join(format!("{version}-key-mutant.stderr")),
            &mutant.stderr,
        )
        .unwrap();
        assert!(
            !mutant.status.success(),
            "Native ID mutation escaped alias cache checks"
        );
        assert!(
            String::from_utf8_lossy(&mutant.stderr)
                .contains("native different-filename picture alias"),
            "Mutant failed outside the alias contract"
        );
    }

    let instance = wgpu::Instance::default();
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_rom(&rom_path).unwrap();
    let server = AssetServer::new(io.into(), context(&device, &queue));
    let mut gpu_blocks = 0usize;
    shin_tasks::block_on(async {
        for paths in groups.values() {
            let mut pictures: Vec<_> =
                futures::future::join_all(paths.iter().map(|path| server.load::<Picture, _>(path)))
                    .await
                    .into_iter()
                    .map(Result::unwrap)
                    .collect();
            gpu_blocks += pictures[0].blocks.len();
            let weak = Arc::downgrade(&pictures[0].blocks);
            for (path, picture) in paths.iter().zip(&pictures) {
                assert!(
                    Arc::ptr_eq(&pictures[0].blocks, &picture.blocks),
                    "Alias uploaded twice: {path}"
                );
                assert_eq!(picture.label, format!("Pic[{path}]"));
            }
            drop(pictures.remove(0));
            assert!(
                weak.upgrade().is_some(),
                "First filename retirement released a retained alias"
            );
            drop(pictures);
            assert!(
                weak.upgrade().is_none(),
                "Unreferenced GPU blocks retained by cache"
            );
            let reload = server
                .load::<Picture, _>(&paths[paths.len() - 1])
                .await
                .unwrap();
            assert!(
                weak.upgrade().is_none(),
                "Cold reload resurrected retired ownership"
            );
            drop(reload);
        }
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU work must complete before readback or retirement");

    // Real production fixtures distinguish header-only cache hits from body
    // decoding, per-file metadata from shared placement data, and failure retry.
    let fixture = out.join("fixtures");
    std::fs::create_dir_all(&fixture).unwrap();
    let good = read("/picture/0.pic");
    assert!(read_picture_header(&good).unwrap().block_count > 0);
    // A complete header with no descriptors/body succeeds only while another
    // filename retains this picture ID's decoded storage.
    let mut changed = good[..36].to_vec();
    changed[12..14].copy_from_slice(&(-321i16).to_le_bytes());
    changed[14..16].copy_from_slice(&654i16.to_le_bytes());
    changed[16..18].copy_from_slice(&1234u16.to_le_bytes());
    changed[18..20].copy_from_slice(&567u16.to_le_bytes());
    for (name, bytes) in [
        ("first.pic", &good),
        ("bad1.pic", &changed),
        ("bad2.pic", &changed),
        ("retry.pic", &good),
    ] {
        std::fs::write(fixture.join(name), bytes).unwrap();
    }
    let mut short = good[..35].to_vec();
    std::fs::write(fixture.join("short.pic"), &short).unwrap();
    short = good.clone();
    short[8..12].copy_from_slice(&36u32.to_le_bytes());
    std::fs::write(fixture.join("wrong-size.pic"), &short).unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_dir(&fixture).unwrap();
    let local = AssetServer::new(io.into(), context(&device, &queue));
    shin_tasks::block_on(async {
        let first = local.load::<Picture, _>("/first.pic").await.unwrap();
        let alias = local.load::<Picture, _>("/bad1.pic").await.unwrap();
        assert!(
            Arc::ptr_eq(&first.blocks, &alias.blocks),
            "Header-only hit unexpectedly decoded the corrupted body"
        );
        assert_eq!(
            (
                alias.origin_x,
                alias.origin_y,
                alias.effective_width,
                alias.effective_height
            ),
            (-321, 654, 1234, 567)
        );
        assert_ne!(first.origin_x, alias.origin_x);
        assert!(
            local.load::<Picture, _>("/short.pic").await.is_err(),
            "Short fixed header must fail"
        );
        let size_alias = local.load::<Picture, _>("/wrong-size.pic").await.unwrap();
        assert!(
            Arc::ptr_eq(&first.blocks, &size_alias.blocks),
            "Declared size rejected a native header-only hit"
        );
        drop(size_alias);
        let weak = Arc::downgrade(&alias.blocks);
        drop(first);
        assert!(weak.upgrade().is_some());
        drop(alias);
        assert!(weak.upgrade().is_none());
        let (a, b) = futures::join!(
            local.load::<Picture, _>("/bad1.pic"),
            local.load::<Picture, _>("/bad2.pic")
        );
        for failed in [a, b] {
            let error = failed.err().expect("Cold corrupt alias must fail");
            assert!(
                format!("{error:#}").contains("failed to fill whole buffer"),
                "{error:#}"
            );
        }
        let retry = local.load::<Picture, _>("/retry.pic").await.unwrap();
        let hit = local.load::<Picture, _>("/bad1.pic").await.unwrap();
        assert!(Arc::ptr_eq(&retry.blocks, &hit.blocks));

        let (other_device, other_queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();
        let mut io = LayeredAssetIo::new();
        io.try_with_dir(&fixture).unwrap();
        let other = AssetServer::new(io.into(), context(&other_device, &other_queue));
        let separate = other.load::<Picture, _>("/first.pic").await.unwrap();
        assert!(
            !Arc::ptr_eq(&retry.blocks, &separate.blocks),
            "GPU resource crossed device/context boundary"
        );
        other_device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU work must complete before readback or retirement");
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU work must complete before readback or retirement");
    let stats = format!(
        r#"{{"groups":32,"alias_files":76,"extra_aliases":44,
        "raw_alias_bytes_compared":{compared_bytes},"native_profiles":4,"native_cold_and_reload_jobs":{native_jobs},
        "production_unique_gpu_blocks":{gpu_blocks},"native_key_mutants_detected":2,
        "metadata_header_only_failure_retry_device_isolation":true}}"#
    );
    std::fs::write(out.join("stats.json"), &stats).unwrap();
    println!(
        "Picture alias cache checks passed: {stats}; evidence {}",
        out.display()
    );
}
