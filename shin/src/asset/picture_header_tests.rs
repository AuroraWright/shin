use super::*;
use crate::{
    asset::system::{AssetServer, LayeredAssetIo},
    layer::user::PictureLayer,
};
use glam::{Mat4, Vec2};
use shin_core::format::picture::read_picture;
use std::path::{Path, PathBuf};

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn metadata(picture: &Picture) -> [u32; 4] {
    [
        picture.origin_x as u16 as u32 | ((picture.origin_y as u16 as u32) << 16),
        picture.effective_width | (picture.effective_height << 16),
        picture.flags,
        picture.scale.to_bits(),
    ]
}

fn parent(profile: usize) -> Mat4 {
    let mut values = Mat4::IDENTITY.to_cols_array();
    match profile {
        0 => {}
        1 => {
            values[0] = 0.5;
            values[1] = 0.25;
            values[4] = -0.125;
            values[5] = 2.0;
            values[12] = 123.5;
            values[13] = -64.25;
        }
        2 => {
            values[0] = -2.0;
            values[1] = 0.125;
            values[3] = 0.0009765625;
            values[4] = 0.5;
            values[5] = -0.25;
            values[7] = -0.001953125;
            values[12] = -320.0;
            values[13] = 90.5;
        }
        _ => unreachable!(),
    }
    Mat4::from_cols_array(&values)
}

#[test]
#[ignore = "requires validate-native-picture-header.py output and Metal; no window or audio"]
fn reference_native_picture_headers_cache_metadata_and_scale() {
    run_cases("SHIN_PICTURE_HEADER_OUTPUT_DIR", 21, 864);
}

#[test]
#[ignore = "requires validate-native-picture-header.py --descriptor-profiles output and Metal; no window or audio"]
fn reference_native_picture_duplicate_descriptors() {
    run_cases("SHIN_PICTURE_DESCRIPTOR_OUTPUT_DIR", 7, 264);
}

#[test]
#[ignore = "requires validate-native-picture-file-span.py output and Metal; no window or audio"]
fn reference_native_picture_file_span_errors_and_cache_retry() {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os("SHIN_PICTURE_FILE_SPAN_OUTPUT_DIR").unwrap());
    let server = fixture_server(&output);
    let cases = std::fs::read_to_string(output.join("cases.tsv")).unwrap();
    std::fs::copy(
        output.join("valid-control.pic"),
        output.join("recovery.pic"),
    )
    .unwrap();
    let mut passed = 0;
    let mut failures = 0;
    shin_tasks::block_on(async {
        for row in cases.lines() {
            let fields: Vec<_> = row.split('\t').collect();
            assert_eq!(fields.len(), 4);
            let name = fields[0];
            let bytes = std::fs::read(fields[1]).unwrap();
            let native_failed = fields[2] == "3";
            let decoded = read_picture::<super::native_load_tests::DecodedPicture>(&bytes, ());
            assert_eq!(
                decoded.is_err(),
                native_failed,
                "{name}: native range outcome"
            );
            let path = format!("/{name}.pic");
            if native_failed {
                let alias = format!("/{name}-alias.pic");
                std::fs::write(output.join(format!("{name}-alias.pic")), &bytes).unwrap();
                let (first, second) = futures::join!(
                    server.load::<Picture, _>(&path),
                    server.load::<Picture, _>(&alias),
                );
                for failed in [first, second] {
                    let error = failed.err().expect("Invalid cold span published a picture");
                    if !name.starts_with("truncated-") {
                        assert!(
                            format!("{error:#}").contains("Picture block range error"),
                            "{name}: {error:#}"
                        );
                    }
                }
                let recovery = server.load::<Picture, _>("/recovery.pic").await.unwrap();
                let weak = Arc::downgrade(&recovery.blocks);
                // Once the same ID is retained, native construction reads only
                // the fixed header. Invalid body/descriptor spans are unused.
                if bytes.len() >= 36 {
                    let warm = server.load::<Picture, _>(&path).await.unwrap();
                    assert!(Arc::ptr_eq(&recovery.blocks, &warm.blocks));
                    drop(warm);
                } else {
                    assert!(server.load::<Picture, _>(&path).await.is_err());
                }
                drop(recovery);
                assert!(
                    weak.upgrade().is_none(),
                    "{name}: failed load retained an owner"
                );
                assert!(
                    server.load::<Picture, _>(&path).await.is_err(),
                    "{name}: expired alias bypassed cold validation"
                );
                failures += 1;
            } else {
                let loaded = server.load::<Picture, _>(&path).await.unwrap();
                let weak = Arc::downgrade(&loaded.blocks);
                let native = std::fs::read_to_string(fields[3]).unwrap();
                let blocks = native.lines().filter(|line| line.starts_with("B ")).count();
                assert_eq!(loaded.blocks.len(), blocks);
                drop(loaded);
                assert!(weak.upgrade().is_none());
            }
            passed += 1;
            println!("Native PIC span case{name}: cold failure={native_failed}");
        }
    });
    assert_eq!(passed, 10);
    assert_eq!(failures, 8);
    println!(
        "Native PIC span errors: {passed} cases, {failures} failures; paired failed aliases, recovery/warm alias and last-owner retirement pass; no window/audio or save writes"
    );
}

fn fixture_server(output: &Path) -> AssetServer {
    let instance = wgpu::Instance::default();
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let mut io = LayeredAssetIo::new();
    io.try_with_dir(output).unwrap();
    AssetServer::new(
        io.into(),
        AssetLoadContext {
            game: crate::cli::Game::Umineko,
            higurashi_bustup_cache: Default::default(),
            wgpu_device: device,
            wgpu_queue: queue,
            bustup_cache: crate::asset::system::cache::AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    )
}

fn run_cases(output_variable: &str, expected_cases: usize, expected_matrices: usize) {
    crate::init_test_task_pools();
    let output = PathBuf::from(std::env::var_os(output_variable).unwrap());
    let cases = std::fs::read_to_string(output.join("cases.tsv")).unwrap();
    let server = fixture_server(&output);
    let mut count = 0;
    let mut matrices = 0;
    let mut compared_pixels = 0;
    let mut largest_matrix_error = 0.0f32;
    shin_tasks::block_on(async {
        for row in cases.lines() {
            let fields: Vec<_> = row.split('\t').collect();
            assert_eq!(fields.len(), 4);
            let name = fields[0];
            let cold_bytes = std::fs::read(fields[1]).unwrap();
            let prefix = std::fs::read(fields[2]).unwrap();
            assert_eq!(prefix.len(), 36);
            let decoded =
                read_picture::<super::native_load_tests::DecodedPicture>(&cold_bytes, ()).unwrap();
            let trace = std::fs::read_to_string(fields[3]).unwrap();
            let cold_path = format!(
                "/{}",
                Path::new(fields[1]).file_name().unwrap().to_str().unwrap()
            );
            let hit_path = format!(
                "/{}",
                Path::new(fields[2]).file_name().unwrap().to_str().unwrap()
            );
            let cold = server.load::<Picture, _>(&cold_path).await.unwrap();
            let hit = server.load::<Picture, _>(&hit_path).await.unwrap();
            assert!(
                Arc::ptr_eq(&cold.blocks, &hit.blocks),
                "{name}: header-only alias lost shared blocks"
            );
            let weak = Arc::downgrade(&cold.blocks);
            let layer = PictureLayer::new(cold.clone(), None);
            let placements: Vec<_> = trace
                .lines()
                .filter(|row| row.starts_with("L "))
                .map(|row| {
                    let words: Vec<u32> = row
                        .split_whitespace()
                        .skip(1)
                        .map(|w| w.parse().unwrap())
                        .collect();
                    (
                        Vec2::new(f32::from_bits(words[0]), f32::from_bits(words[1])),
                        words[2],
                    )
                })
                .collect();
            let traversal: Vec<_> = (0..3)
                .map(|profile| layer.reference_placement_transforms(parent(profile)))
                .collect();
            for profile in &traversal {
                assert_eq!(profile.len(), placements.len(), "{name}: traversal length");
                for ((offset, _), (_, native_offset)) in profile.iter().zip(&placements) {
                    assert_eq!(offset, native_offset, "{name}: authored draw order");
                }
            }
            let mut current_profile = 0;
            let mut headers = 0;
            for row in trace.lines() {
                let words: Vec<_> = row.split_whitespace().collect();
                match words[0] {
                    "H" => {
                        let native: Vec<u32> =
                            words[1..].iter().map(|w| w.parse().unwrap()).collect();
                        let picture = if headers == 0 { &cold } else { &hit };
                        assert_eq!(
                            &metadata(picture)[..4],
                            &native[..4],
                            "{name}: per-file metadata"
                        );
                        let bytes = if headers == 0 { &cold_bytes } else { &prefix };
                        assert_eq!(
                            u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
                            native[4]
                        );
                        headers += 1;
                    }
                    "B" => {
                        let offset: u32 = words[2].parse().unwrap();
                        let (positions, block) = &decoded.blocks[&offset];
                        let native_positions: Vec<_> = placements
                            .iter()
                            .filter(|(_, off)| *off == offset)
                            .map(|(p, _)| (p.x as u32, p.y as u32))
                            .collect();
                        assert_eq!(*positions, native_positions, "{name}: grouped placements");
                        assert_eq!(block.data.width(), words[3].parse::<u32>().unwrap());
                        assert_eq!(block.data.height(), words[4].parse::<u32>().unwrap());
                        let (vertices, indices) = vertex_data(block);
                        let vertex_bytes: Vec<_> = vertices
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
                            .collect();
                        let index_bytes: Vec<_> =
                            indices.iter().flat_map(|i| i.to_le_bytes()).collect();
                        assert_eq!(
                            fnv(&vertex_bytes),
                            u64::from_str_radix(words[7], 16).unwrap(),
                            "{name}: vertex bytes"
                        );
                        assert_eq!(
                            fnv(&index_bytes),
                            u64::from_str_radix(words[8], 16).unwrap(),
                            "{name}: index bytes"
                        );
                        assert_eq!(
                            fnv(block.data.as_raw()),
                            u64::from_str_radix(words[9], 16).unwrap(),
                            "{name}: decoded pixels"
                        );
                        compared_pixels += block.data.as_raw().len() / 4;
                    }
                    "R" => current_profile = words[1].parse().unwrap(),
                    "T" => {
                        let index: usize = words[1].parse().unwrap();
                        let native: Vec<_> = words[2..]
                            .iter()
                            .map(|w| f32::from_bits(w.parse().unwrap()))
                            .collect();
                        let ours = traversal[current_profile][index].1.to_cols_array();
                        for (value, expected) in ours.iter().zip(native) {
                            let error = (value - expected).abs();
                            largest_matrix_error = largest_matrix_error.max(error);
                            assert_eq!(
                                value.to_bits(),
                                expected.to_bits(),
                                "{name}: matrix profile{current_profile} value{value} != native{expected}"
                            );
                        }
                        matrices += 1;
                    }
                    "P" | "L" | "D" => {}
                    _ => panic!("Unknown native trace row {row}"),
                }
            }
            assert_eq!(headers, 2);
            drop(layer);
            drop(cold);
            assert!(weak.upgrade().is_some());
            drop(hit);
            assert!(weak.upgrade().is_none());
            count += 1;
            println!(
                "Native PIC header/cache/scale case{name}: {} placements",
                placements.len()
            );
        }
    });
    assert_eq!(count, expected_cases);
    assert_eq!(matrices, expected_matrices);
    println!(
        "Native PIC headers: {count} cases, {matrices} placement matrices, {compared_pixels} pixels; maximum matrix error {largest_matrix_error}; no window/audio or save writes"
    );
}
