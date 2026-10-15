use super::*;
use std::{sync::mpsc, time::Duration};

mod native;

pub(crate) fn fixture(corrupt_a: bool) -> Arc<GpuFontLazy> {
    Arc::new(
        GpuFontLazy::new(read_lazy_font(&mut Cursor::new(fixture_bytes(corrupt_a))).unwrap())
            .unwrap(),
    )
}

fn fixture_bytes(corrupt_a: bool) -> Vec<u8> {
    // Every BMP entry aliases B except A, which has the same metrics and a
    // separate bitmap. A is either transparent or deliberately malformed.
    let table_end = 16 + 65536 * 4;
    let b_offset = table_end as u32;
    let a_offset = b_offset + 95;
    let mut bytes = b"FNT4".to_vec();
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(7u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    for codepoint in 0..65536 {
        bytes.extend((if codepoint == 65 { a_offset } else { b_offset }).to_le_bytes());
    }
    bytes.extend([0, 7, 8, 8, 8, 0, 8, 8]);
    bytes.extend(0u16.to_le_bytes());
    bytes.extend([127; 85]);
    bytes.extend([0, 7, 8, 8, 8, 0, 8, 8]);
    if corrupt_a {
        bytes.extend(1u16.to_le_bytes());
        bytes.push(255);
    } else {
        bytes.extend(0u16.to_le_bytes());
        bytes.extend([0; 85]);
    }
    let length = bytes.len() as u32;
    bytes[8..12].copy_from_slice(&length.to_le_bytes());
    bytes
}

#[test]
fn gpu_font_rejects_zero_line_height_and_preserves_positive_metrics() {
    for (ascent, descent) in [(0u16, 0u16), (1, 0), (0, 1), (65535, 65535)] {
        let mut bytes = fixture_bytes(false);
        bytes[12..14].copy_from_slice(&ascent.to_le_bytes());
        bytes[14..16].copy_from_slice(&descent.to_le_bytes());
        let font = GpuFontLazy::new(read_lazy_font(&mut Cursor::new(bytes)).unwrap());
        if ascent == 0 && descent == 0 {
            assert!(font.err().unwrap().to_string().contains("line height"));
        } else {
            let font = font.unwrap();
            assert_eq!(font.get_ascent(), u32::from(ascent));
            assert_eq!(font.get_descent(), u32::from(descent));
        }
    }
}
fn gpu() -> (wgpu::Device, wgpu::Queue) {
    crate::init_test_task_pools();
    let instance = wgpu::Instance::default();
    let adapter =
        shin_tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    shin_tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap()
}

#[test]
#[ignore = "requires Metal GPU; no ROM, audio or working saves"]
fn glyph_batch_decoder_failure_wakes_aliases_without_cancelling_healthy_glyphs() {
    let (device, queue) = gpu();
    let font = fixture(true);
    let handles = font.clone().load_glyphs(device, queue, &['A', 'B', 'A']);
    assert_eq!(handles[0].id, handles[2].id);
    let (done, done_recv) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let states: Vec<_> = handles
            .iter()
            .map(|h| h.as_texture_source().is_ok())
            .collect();
        done.send(states).unwrap();
    });
    assert_eq!(
        done_recv.recv_timeout(Duration::from_secs(5)).unwrap(),
        [false, true, false]
    );
    thread.join().unwrap();
}

#[test]
#[ignore = "requires Metal GPU; no ROM, audio or working saves"]
fn failed_or_cancelled_glyph_generations_allow_retry_without_replacing_old_results() {
    let (device, queue) = gpu();
    for kind in 0..3 {
        let font = fixture(false);
        let id = font.font.get_character_mapping()[66];
        let key = GlyphKey {
            font: font.font.get_character_table_crc(),
            codepoint: 'B',
        };
        let producer = match font.glyph_cache.lookup(key) {
            crate::asset::system::cache::CacheLookupResult::LoadRequired(producer) => producer,
            _ => panic!("new font cache"),
        };
        let stale = GpuFontGlyphHandle {
            key,
            id,
            info: font.font.get_glyph(id).unwrap().get_info(),
            handle: producer.get_handle(),
        };
        match kind {
            0 => drop(producer),
            1 => font.complete_glyph_load(key, producer, || anyhow::bail!("injected upload error")),
            2 => font.complete_glyph_load(key, producer, || panic!("injected upload panic")),
            _ => unreachable!(),
        }
        assert!(stale.as_texture_source().is_err());
        let mut retry = font
            .clone()
            .load_glyphs(device.clone(), queue.clone(), &['B']);
        let retry = retry.remove(0);
        assert!(retry.as_texture_source().is_ok());
        assert!(stale.as_texture_source().is_err());
        let cached = font
            .clone()
            .load_glyphs(device.clone(), queue.clone(), &['B'])
            .remove(0);
        let first = shin_tasks::block_on(retry.handle.clone().wait_async()).unwrap();
        let second = shin_tasks::block_on(cached.handle.clone().wait_async()).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let weak = Arc::downgrade(&first);
        drop(first);
        drop(second);
        drop(retry);
        drop(cached);
        assert!(
            weak.upgrade().is_some(),
            "Native cache retains unowned GPU glyphs below eviction pressure"
        );
        drop(font);
        assert!(
            weak.upgrade().is_none(),
            "Retiring the device/font cache releases glyphs"
        );
    }
}

#[test]
#[ignore = "requires Metal GPU; no ROM, audio or working saves"]
fn pending_glyph_aliases_share_the_producer_while_other_ids_finish() {
    let (device, queue) = gpu();
    let font = fixture(false);
    let id = font.font.get_character_mapping()[65];
    let key = GlyphKey {
        font: font.font.get_character_table_crc(),
        codepoint: 'A',
    };
    let producer = match font.glyph_cache.lookup(key) {
        crate::asset::system::cache::CacheLookupResult::LoadRequired(producer) => producer,
        _ => panic!("new font cache"),
    };
    let first = font
        .clone()
        .load_glyphs(device.clone(), queue.clone(), &['A'])
        .remove(0);
    let second = font
        .clone()
        .load_glyphs(device.clone(), queue.clone(), &['A'])
        .remove(0);
    let independent = font
        .clone()
        .load_glyphs(device.clone(), queue.clone(), &['B'])
        .remove(0);
    assert!(independent.as_texture_source().is_ok());
    let worker_font = font.clone();
    let worker = std::thread::spawn(move || {
        worker_font.complete_glyph_load(key, producer, || {
            Ok(GpuFontGlyph::load(
                &device,
                &queue,
                id,
                worker_font.font.get_glyph(id).unwrap().decompress(),
            ))
        })
    });
    let first = shin_tasks::block_on(first.handle.wait_async()).unwrap();
    let second = shin_tasks::block_on(second.handle.wait_async()).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    worker.join().unwrap();
}

#[test]
#[ignore = "requires Metal GPU; no ROM, audio or working saves"]
fn codepoint_identity_and_shared_font_residency_match_native_keys() {
    let (device, queue) = gpu();
    let font = fixture(false);
    let mut same = fixture(false);
    Arc::get_mut(&mut same).unwrap().glyph_cache = font.glyph_cache.clone();
    let a = font
        .clone()
        .load_glyphs(device.clone(), queue.clone(), &['B'])
        .remove(0);
    let b = same
        .clone()
        .load_glyphs(device.clone(), queue.clone(), &['B'])
        .remove(0);
    let alias = font
        .clone()
        .load_glyphs(device.clone(), queue.clone(), &['C'])
        .remove(0);
    assert_eq!(a.id, alias.id, "Synthetic B/C share one source bitmap");
    let a = shin_tasks::block_on(a.handle.wait_async()).unwrap();
    let b = shin_tasks::block_on(b.handle.wait_async()).unwrap();
    let alias = shin_tasks::block_on(alias.handle.wait_async()).unwrap();
    assert!(
        Arc::ptr_eq(&a, &b),
        "Same font identifier and codepoint share residency"
    );
    assert!(
        !Arc::ptr_eq(&a, &alias),
        "Native cache keeps aliased codepoints separate"
    );
    let weak = Arc::downgrade(&a);
    drop(a);
    drop(b);
    let warm = font.clone().load_glyphs(device, queue, &['B']).remove(0);
    let warm = shin_tasks::block_on(warm.handle.wait_async()).unwrap();
    assert!(
        Arc::ptr_eq(&weak.upgrade().unwrap(), &warm),
        "Unowned glyph stays warm"
    );
    let other_context = fixture(false);
    assert!(
        !Arc::ptr_eq(&font.glyph_cache, &other_context.glyph_cache),
        "Device contexts isolate GPU residents"
    );
}
