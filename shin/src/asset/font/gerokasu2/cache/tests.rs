use super::*;
use shin_core::format::font::gerokasu2::{
    Gerokasu2FontHeader, Gerokasu2GlyphInfo,
    texture::{GlyphTextureGeometry, PreparedGlyphTexture},
};
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
};

fn header(system: bool) -> Gerokasu2FontHeader {
    Gerokasu2FontHeader {
        version: 1,
        file_size: 0,
        flags: 0,
        page_count: 0,
        glyph_count: 0,
        metric_words: if system {
            [15, 0, 53, 7, 33, 0]
        } else {
            [20, 0, 70, 10, 41, 0]
        },
    }
}
fn info(identity: u32) -> Gerokasu2GlyphInfo {
    Gerokasu2GlyphInfo {
        bearing_x: -300,
        bearing_y: 500,
        width: 8,
        height: 4,
        advance_sixteenths: 205,
        word_10: 0,
        identity,
        mip_count: 1,
        decoded_size: 32,
    }
}
fn fixture(system: bool) -> Gerokasu2Font {
    // A/B, supplementary cat and U+203B all share descriptor100; C uses200.
    let mut data = vec![0u8; 32 + 3 * 514 + 2 * 24 + 2];
    data[..4].copy_from_slice(b"FNT6");
    data[4..8].copy_from_slice(&1u32.to_le_bytes());
    let len = data.len() as u32;
    data[8..12].copy_from_slice(&len.to_le_bytes());
    data[16..18].copy_from_slice(&3u16.to_le_bytes());
    data[18..20].copy_from_slice(&2u16.to_le_bytes());
    for (i, word) in header(system).metric_words.into_iter().enumerate() {
        data[20 + i * 2..22 + i * 2].copy_from_slice(&word.to_le_bytes());
    }
    for (i, tag) in [0u16, 0x20, 0x1f4].into_iter().enumerate() {
        let at = 32 + i * 514;
        data[at..at + 2].copy_from_slice(&tag.to_le_bytes());
        data[at + 2..at + 514].fill(0xff);
        let pairs: &[(usize, u16)] = match tag {
            0 => &[(65, 0), (66, 0), (67, 1)],
            0x20 => &[(0x3b, 0)],
            _ => &[(8, 0)],
        };
        for &(low, id) in pairs {
            data[at + 2 + low * 2..at + 4 + low * 2].copy_from_slice(&id.to_le_bytes());
        }
    }
    for (i, identity) in [100u32, 200].into_iter().enumerate() {
        let at = 32 + 3 * 514 + i * 24;
        data[at + 4..at + 6].copy_from_slice(&1u16.to_le_bytes());
        data[at + 6..at + 8].copy_from_slice(&1u16.to_le_bytes());
        data[at + 8..at + 10].copy_from_slice(&205u16.to_le_bytes());
        data[at + 12..at + 16].copy_from_slice(&identity.to_le_bytes());
        data[at + 20..at + 24].copy_from_slice(&(len - 2 + i as u32).to_le_bytes());
    }
    Gerokasu2Font::new(data.into()).unwrap()
}
fn prepare(font: &Gerokasu2Font, cp: char) -> anyhow::Result<PreparedGlyphTexture> {
    font.lookup(u32::from(cp))
        .decode()?
        .prepare_texture(u32::from(cp), font.header())
}

#[test]
fn aliases_and_cross_font_requests_share_producers_and_first_source_geometry() {
    let cache = Gerokasu2GlyphCache::<PreparedGlyphTexture>::default();
    let font = fixture(false);
    let other = fixture(true);
    let mut batch = request_glyphs(&font, &cache, &['A', 'B', 'C', '🐈', '\0']);
    assert_eq!(
        batch
            .requests
            .iter()
            .map(|r| (r.identity, r.codepoint))
            .collect::<Vec<_>>(),
        [(100, 'A'), (200, 'C')]
    );
    let shared = request_glyphs(&other, &cache, &['B']);
    assert!(shared.requests.is_empty());
    let independent = batch.requests.pop().unwrap();
    cache.complete_load(independent, || prepare(&font, 'C'));
    assert_eq!(
        batch.handles[2].try_wait_ref().unwrap().geometry.codepoint,
        u32::from('C')
    );
    let first = batch.requests.pop().unwrap();
    cache.complete_load(first, || prepare(&font, 'A'));
    let first = futures::executor::block_on(batch.handles[0].clone().wait_async()).unwrap();
    for i in [1, 3, 4] {
        let alias = futures::executor::block_on(batch.handles[i].clone().wait_async()).unwrap();
        assert!(Arc::ptr_eq(&first, &alias));
    }
    let other = futures::executor::block_on(shared.handles[0].clone().wait_async()).unwrap();
    assert!(Arc::ptr_eq(&first, &other));
    assert_eq!(other.geometry.font_metrics, [70, 10, 41]);
    assert_eq!(other.geometry.codepoint, u32::from('A'));
    let weak = Arc::downgrade(&first);
    drop(first);
    drop(other);
    drop(batch);
    drop(shared);
    assert!(
        weak.upgrade().is_some(),
        "Unowned glyph stays resident below pressure"
    );
    let warm = request_glyphs(&font, &cache, &['B']);
    assert!(warm.requests.is_empty());
    drop(warm);
    drop(cache);
    assert!(
        weak.upgrade().is_none(),
        "Context retirement releases residency"
    );
}

#[test]
fn failed_panicked_and_cancelled_workers_wake_aliases_and_preserve_retry_generations() {
    let font = fixture(false);
    for kind in 0..3 {
        let cache = Gerokasu2GlyphCache::<PreparedGlyphTexture>::default();
        let mut batch = request_glyphs(&font, &cache, &['A', 'B']);
        let request = batch.requests.pop().unwrap();
        match kind {
            0 => drop(request),
            1 => cache.complete_load(request, || anyhow::bail!("injected FNT6 decode error")),
            _ => cache.complete_load(request, || panic!("injected FNT6 upload panic")),
        }
        for handle in &batch.handles {
            assert!(handle.try_wait_ref().is_err());
        }
        let mut retry = request_glyphs(&font, &cache, &['B']);
        assert_eq!(retry.requests.len(), 1);
        cache.complete_load(retry.requests.pop().unwrap(), || prepare(&font, 'B'));
        assert_eq!(
            retry.handles[0].try_wait_ref().unwrap().geometry.codepoint,
            u32::from('B')
        );
        for handle in &batch.handles {
            assert!(handle.try_wait_ref().is_err());
        }
        assert!(request_glyphs(&font, &cache, &['A']).requests.is_empty());
        assert_eq!(cache.residents.lock().snapshot().len(), 1);
    }
}

#[test]
fn device_contexts_separate_native_key_spaces_and_gpu_font_metrics_stay_fractional() {
    use crate::asset::font::{FontGlyphCache, gerokasu2::GpuGerokasu2Font};
    use shin_core::layout::font::TextFontMetrics;
    let context = FontGlyphCache::default();
    let separate = FontGlyphCache::default();
    let a = GpuGerokasu2Font::with_cache(fixture(false), context.gerokasu2.clone()).unwrap();
    let b = GpuGerokasu2Font::with_cache(fixture(true), context.gerokasu2.clone()).unwrap();
    assert!(Arc::ptr_eq(&a.glyph_cache, &b.glyph_cache));
    assert!(!Arc::ptr_eq(&a.glyph_cache, &separate.gerokasu2));
    assert_eq!(Arc::new(a).metrics().glyph_advance('🐈'), Some(12.8125));
    assert_eq!(b.line_height(), 60.0);
    let empty = request_glyphs(&b.font, &b.glyph_cache, &[]);
    assert!(empty.handles.is_empty() && empty.requests.is_empty());
}

struct Native {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::process::ChildStdout,
    cache: Gerokasu2GlyphCache<GlyphTextureGeometry>,
    owners: Vec<Option<Arc<GlyphTextureGeometry>>>,
    created: u32,
    cases: usize,
    record_bytes: usize,
}
impl Native {
    fn new() -> Self {
        let mut child = Command::new(
            std::env::var_os("SHIN_GEROKASU2_FONT_CACHE_ORACLE").expect("cache oracle"),
        )
        .arg(std::env::var_os("SHIN_GEROKASU2_EXECUTABLE").expect("executable"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
            cache: Gerokasu2GlyphCache::default(),
            owners: vec![None; 4096],
            created: 0,
            cases: 0,
            record_bytes: 0,
        }
    }
    fn operation(
        &mut self,
        op: u32,
        slot: u32,
        aux: u32,
        header: &Gerokasu2FontHeader,
        info: Gerokasu2GlyphInfo,
        cp: u32,
    ) {
        let mut row = Vec::new();
        for v in [op, slot, aux] {
            row.extend(v.to_le_bytes());
        }
        for v in header.native_owner_metrics() {
            row.extend(v.to_le_bytes());
        }
        row.extend(0u16.to_le_bytes());
        row.extend(cp.to_le_bytes());
        row.extend(info.bearing_x.to_le_bytes());
        row.extend(info.bearing_y.to_le_bytes());
        row.extend(info.width.to_le_bytes());
        row.extend(info.height.to_le_bytes());
        row.extend(info.advance_width().to_le_bytes());
        row.extend(info.mip_count.to_le_bytes());
        row.extend(0u16.to_le_bytes());
        row.extend(info.decoded_size.to_le_bytes());
        row.extend(info.identity.to_le_bytes());
        row.extend(0u32.to_le_bytes());
        assert_eq!(row.len(), 60);
        match op {
            0 => {
                self.owners.fill(None);
                self.cache = Gerokasu2GlyphCache::default();
                self.created = 0;
            }
            1 => {
                assert!(self.owners[slot as usize].is_none());
                let value = match self.cache.lookup(info.identity) {
                    CacheLookupResult::Loaded(value) => value,
                    CacheLookupResult::LoadRequired(producer) => {
                        let value = Arc::new(info.texture_geometry(cp, header));
                        self.cache
                            .finish_load(info.identity, producer, value.clone());
                        self.created += 1;
                        value
                    }
                    CacheLookupResult::Loading(_) => panic!("sequential native contract"),
                };
                self.owners[slot as usize] = Some(value);
            }
            2 => {
                assert!(self.owners[slot as usize].take().is_some());
            }
            3 => {
                assert!(self.owners[aux as usize].is_none());
                self.owners[aux as usize] = self.owners[slot as usize].clone();
            }
            4 => self.cache.residents.lock().set_age(slot),
            _ => panic!("operation"),
        }
        self.input.as_mut().unwrap().write_all(&row).unwrap();
        let mut prefix = [0u8; 20];
        if let Err(error) = self.output.read_exact(&mut prefix) {
            let status = self.child.wait().expect("native cache exit status");
            panic!(
                "Native cache stopped after case{}: op{op} U+{cp:x} identity{:08x}: {error}; {status}",
                self.cases, info.identity
            );
        }
        assert_eq!(&prefix[..4], b"GFC1");
        let words = prefix[4..]
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect::<Vec<_>>();
        let residents = self.cache.residents.lock();
        let state = residents.snapshot();
        assert_eq!(
            words,
            [
                residents.age(),
                state.len() as u32,
                self.created,
                self.created - state.len() as u32
            ],
            "native cache header after case{}",
            self.cases
        );
        let mut records = vec![0u8; state.len() * 60];
        self.output.read_exact(&mut records).unwrap();
        for ((&key, g), native) in residents.values().zip(records.chunks_exact(60)) {
            let &(refs, age) = &state[&key];
            let expected = [
                refs as u32,
                age,
                g.identity,
                g.codepoint,
                g.origin[0] as u32,
                g.origin[1] as u32,
                g.dimensions[0],
                g.dimensions[1],
                g.advance.to_bits(),
                g.font_metrics[0] as u32,
                g.font_metrics[1] as u32,
                g.font_metrics[2] as u32,
                g.scale[0].to_bits(),
                g.scale[1].to_bits(),
                g.extra_metric.to_bits(),
            ];
            let actual = native
                .chunks_exact(4)
                .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
                .collect::<Vec<_>>();
            assert_eq!(
                actual, expected,
                "native cache case{} identity{key:08x}",
                self.cases
            );
        }
        self.record_bytes += records.len();
        self.cases += 1;
    }
    fn simple(&mut self, op: u32, slot: u32, aux: u32) {
        self.operation(op, slot, aux, &header(false), info(0), 65);
    }
    fn load(&mut self, slot: u32, id: u32, cp: u32) {
        self.operation(1, slot, 0, &header(false), info(id), cp);
    }
    fn finish(mut self) -> (usize, usize) {
        self.input.take();
        assert_eq!(self.output.read(&mut [0; 1]).unwrap(), 0);
        assert!(self.child.wait().unwrap().success());
        (self.cases, self.record_bytes)
    }
}

#[test]
#[ignore = "requires supplied executable and guarded CPU font cache oracle"]
fn native_identity_alias_ownership_pressure_and_compaction_match_shared_residency() {
    let mut n = Native::new();
    n.simple(0, 0, 0);
    n.load(0, 7, 65);
    n.simple(3, 0, 1);
    let mut different = info(7);
    different.bearing_x = 123;
    different.width = 30;
    different.decoded_size = 120;
    n.operation(1, 2, 0, &header(true), different, 0x1f408);
    for slot in [0, 1, 2] {
        n.simple(2, slot, 0);
    }
    n.load(0, 7, 66);
    n.simple(2, 0, 0);
    for i in 0..1080 {
        n.load(100 + i, 0x10000 + i, 65 + i);
    }
    n.simple(3, 100, 2000);
    for i in 0..1080 {
        n.simple(2, 100 + i, 0);
    }
    n.load(0, 0x10001, 999);
    n.simple(2, 0, 0);
    n.load(0, 0x80000000, 0x1f408);
    n.simple(4, 65535, 0);
    n.load(1, 0x10000, 888);
    n.simple(2, 1, 0);
    n.load(1, 0x90000000, 0x203b);
    n.simple(2, 1, 0);
    n.simple(2, 0, 0);
    n.simple(2, 2000, 0);
    n.simple(0, 0, 0);
    let (cases, bytes) = n.finish();
    println!(
        "GEROKASU2_FONT_CACHE_SYNTHETIC operations={cases} compared_record_bytes={bytes} identity_refs_ages_geometry=verified"
    );
}

#[test]
#[ignore = "requires supplied ROM/executable and guarded CPU font cache oracle"]
fn all_supplied_font_identities_and_aliases_match_native_warm_cache_and_retirement() {
    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    let mut rom = RomReader::new(
        StatelessFile::new(
            std::fs::File::open(std::env::var_os("SHIN_GEROKASU2_ROM").unwrap()).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let mut n = Native::new();
    let mut mappings = 0;
    n.simple(0, 0, 0);
    for path in ["/newrodin-medium.fnt", "/system.fnt"] {
        let file = rom.find_file(path).unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let font = Gerokasu2Font::new(bytes.into()).unwrap();
        for (cp, _) in font.codepoints() {
            n.operation(1, 0, 0, font.header(), font.lookup(cp).info, cp);
            n.simple(2, 0, 0);
            mappings += 1;
        }
        for cp in [0, 0x10ffff, 0x110000, u32::MAX] {
            n.operation(1, 0, 0, font.header(), font.lookup(cp).info, cp);
            n.simple(2, 0, 0);
        }
    }
    n.simple(0, 0, 0);
    let (cases, bytes) = n.finish();
    println!(
        "GEROKASU2_FONT_CACHE_SUPPLIED mappings={mappings} operations={cases} compared_record_bytes={bytes} identity_refs_ages_geometry=verified"
    );
}
