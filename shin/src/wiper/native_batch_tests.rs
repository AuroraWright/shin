//! Native virtual-comparison fixtures. Prepared payloads exercise the real
//! comparator, not factories or native NVN rendering. Mask resources are real
//! generated MSK assets so pointer identity uses the production Arc owners.
use super::*;
use std::{path::Path, sync::Arc};

pub(crate) fn reference_masks(device: &wgpu::Device, queue: &wgpu::Queue) -> [Arc<MaskTexture>; 2] {
    use crate::asset::system::{AssetLoadContext, LayeredAssetIo, cache::AssetCache};
    let root = std::env::temp_dir().join(format!("shin-wiper-batch-masks-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let (width, height) = (64u32, 64u32);
    let payload = 4 + width * height;
    let mut bytes = b"MSK4".to_vec();
    for word in [
        1,
        36 + payload + 32,
        0,
        width | (height << 16),
        36,
        payload,
        36 + payload,
        32,
    ] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend(0u32.to_le_bytes());
    for y in 0..height {
        for x in 0..width {
            bytes.push(((x * 13 + y * 7) % 256) as u8);
        }
    }
    for word in [1u32, 0, 0, 0, 0, 0] {
        bytes.extend(word.to_le_bytes());
    }
    for edge in [0u16, 0, (width - 1) as u16, (height - 1) as u16] {
        bytes.extend(edge.to_le_bytes());
    }
    // Distinct cache keys produce different native-style resource identities,
    // despite identical bytes. Repeated references to one Arc retain identity.
    for name in ["a.msk", "b.msk"] {
        std::fs::write(root.join(name), &bytes).unwrap();
    }
    let mut io = LayeredAssetIo::new();
    io.try_with_dir(&root).unwrap();
    let server = AssetServer::new(
        io.into(),
        AssetLoadContext {
            wgpu_device: device.clone(),
            wgpu_queue: queue.clone(),
            bustup_cache: AssetCache::new(),
            picture_cache: Default::default(),
            font_cache: Default::default(),
        },
    );
    let a = shin_tasks::block_on(server.load("/a.msk")).unwrap();
    let b = shin_tasks::block_on(server.load("/b.msk")).unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
    [a, b]
}

pub(crate) fn wipe(kind: u32, words: &[u32; 9], masks: &[Arc<MaskTexture>; 2]) -> AnyWiper {
    let duration = Ticks::from_f32(30.0);
    let mut result = match kind {
        0 => AnyWiper::Default(DefaultWiper::new(duration)),
        1 => AnyWiper::Scroll(scroll::ScrollWiper::new(duration, 0, 1)),
        2 => AnyWiper::Zoom(zoom::ZoomWiper::new(duration, 500, 1500)),
        3 => AnyWiper::Turn(turn::TurnWiper::new(duration, 0)),
        4 => AnyWiper::Wave(wave::WaveWiper::new(duration)),
        5 => AnyWiper::Ripple(ripple::RippleWiper::new(duration)),
        6 => AnyWiper::Scanline(scanline::ScanlineWiper::new(duration, 2)),
        7 => AnyWiper::Mask(MaskWiper::new(
            duration,
            masks[(words[3] - 1) as usize].clone(),
            MaskParam(0.5),
            MaskFlags::empty(),
        )),
        8 => AnyWiper::Whirl(whirl::WhirlWiper::new(duration, 1, 1.0)),
        9 => AnyWiper::Glass(glass::GlassWiper::with_seed(duration, words[2])),
        _ => unreachable!(),
    };
    match &mut result {
        AnyWiper::Default(w) => w.reference_apply_batch_words(words),
        AnyWiper::Scroll(w) => w.reference_apply_batch_words(words),
        AnyWiper::Zoom(w) => w.reference_apply_batch_words(words),
        AnyWiper::Turn(w) => w.reference_apply_batch_words(words),
        AnyWiper::Wave(w) => w.reference_apply_batch_words(words),
        AnyWiper::Ripple(w) => w.reference_apply_batch_words(words),
        AnyWiper::Scanline(w) => w.reference_apply_batch_words(words),
        AnyWiper::Mask(w) => w.reference_apply_batch_words(words),
        AnyWiper::Whirl(w) => w.reference_apply_batch_words(words),
        AnyWiper::Glass(w) => w.reference_apply_batch_words(words),
    }
    result
}

pub(crate) fn render_words(kind: u32, elapsed: f32) -> [u32; 9] {
    let b = f32::to_bits;
    let payload = match kind {
        0 => [0; 7],
        1 => [0, 1, 0, 0, 0, 0, 0],
        2 => [b(0.5), b(1.5), 0, 0, 0, 0, 0],
        3 => [1, 0, 0, 0, 0, 0, 0],
        4 => [b(0.5), b(0.5), b(1.0), b(0.25), 0, 0, 0],
        5 => [
            b(0.5),
            b(0.5),
            b(1.0),
            b(0.25),
            b(0.2),
            b(1.0 / 60.0),
            b(0.125),
        ],
        6 => [b(2.0), 0, 0, 0, 0, 0, 0],
        7 => [0, 1, 0, b(0.5), 0, 0, 0],
        8 => [1, b(1.0), 0, 0, 0, 0, 0],
        9 => [5489, b(2.0), b(0.35), b(0.0), 0, 0, 0],
        _ => unreachable!(),
    };
    [
        b(elapsed),
        b(30.0),
        payload[0],
        payload[1],
        payload[2],
        payload[3],
        payload[4],
        payload[5],
        payload[6],
    ]
}

#[test]
#[ignore = "requires Metal for generated Mask resources and saved native comparison fixtures; no window/audio"]
fn reference_all_native_wiper_batch_comparisons() {
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) =
        shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
            .unwrap();
    let masks = reference_masks(&device, &queue);
    let path = std::env::var("SHIN_NATIVE_WIPER_BATCH_FIXTURES").unwrap();
    let rows: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(Path::new(&path)).unwrap()).unwrap();
    assert_eq!(rows.len(), 3913);
    let mut mismatches = Vec::new();
    let mut old_default_only_mismatches = 0;
    for row in &rows {
        let kind = row["kind"].as_u64().unwrap() as u32;
        let other = row["other"].as_u64().unwrap() as u32;
        let mode = row["mode"].as_u64().unwrap() as u32;
        let expected = row["expected"].as_bool().unwrap();
        let words = |key: &str| -> [u32; 9] {
            row[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap()
        };
        let left = wipe(kind, &words("left"), &masks);
        let right = wipe(other, &words("right"), &masks);
        let result = match mode {
            0 => left.can_batch_with(&right),
            1 => left.can_batch_with(&left),
            2 => false,
            _ => unreachable!(),
        };
        if result != expected {
            mismatches.push(row["label"].as_str().unwrap().to_owned());
        }
        // The prior production restriction accepted only Default pairs.
        let prior = kind == 0 && (mode == 1 || other == 0) && result;
        old_default_only_mismatches += usize::from(prior != expected);
    }
    eprintln!(
        "Native wipe comparisons: {} profiles; {} port mismatches; prior Default-only restriction differs on {} profiles",
        rows.len(),
        mismatches.len(),
        old_default_only_mismatches
    );
    assert!(old_default_only_mismatches > 0);
    assert!(
        mismatches.is_empty(),
        "{} mismatches: {:?}",
        mismatches.len(),
        &mismatches[..mismatches.len().min(20)]
    );
}
