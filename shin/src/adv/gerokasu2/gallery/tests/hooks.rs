use super::super::*;
use shin_core::format::text::utf8::U16Utf8String;

struct Metadata {
    pictures: Vec<PictureInfoItem>,
    bgms: Vec<BgmInfoItem>,
    movies: Vec<MovieInfoItem>,
}
impl Metadata {
    fn new(pictures: &[i16], bgms: &[i16], movies: &[(i16, i16)]) -> Self {
        Self {
            pictures: pictures
                .iter()
                .map(|&linked_cg_id| PictureInfoItem {
                    name: U16Utf8String::new("pic"),
                    linked_cg_id,
                })
                .collect(),
            bgms: bgms
                .iter()
                .map(|&linked_bgm_id| BgmInfoItem {
                    name: U16Utf8String::new("bgm"),
                    display_name: U16Utf8String::new("BGM"),
                    linked_bgm_id,
                })
                .collect(),
            movies: movies
                .iter()
                .map(|&(linked_picture_id, linked_bgm_id)| MovieInfoItem {
                    name: U16Utf8String::new("movie"),
                    linked_picture_id,
                    linked_bgm_id,
                    volume_source: 0,
                    transparency: 0,
                })
                .collect(),
        }
    }
    fn info(&self) -> GalleryMetadata<'_> {
        GalleryMetadata {
            pictures: &self.pictures,
            bgms: &self.bgms,
            movies: &self.movies,
        }
    }
}

#[test]
fn ger_publication_metadata_keeps_movie_cross_gallery_links_and_missing_items() {
    let data = Metadata::new(
        &[32, -32768],
        &[65, -1],
        &[(0, 0), (90, 80), (-1, -32768), (1, 1)],
    );
    let mut gallery = GalleryUnlocks::default();
    assert_eq!(
        loaded_layer_unlocks(data.info(), 2, 0),
        [GalleryUnlock::Picture(0), GalleryUnlock::Picture(32)]
    );
    assert_eq!(
        loaded_layer_unlocks(data.info(), 2, 1),
        [GalleryUnlock::Picture(1)]
    );
    let movie = loaded_layer_unlocks(data.info(), 6, 0);
    assert_eq!(
        movie,
        [
            GalleryUnlock::Music(65),
            GalleryUnlock::Music(0),
            GalleryUnlock::Music(32),
            GalleryUnlock::Picture(0),
            GalleryUnlock::Movie(0)
        ]
    );
    assert!(gallery.apply(movie.clone(), false));
    assert!(!gallery.apply(movie, false));
    assert!(gallery.cgs.contains(0));
    assert!(!gallery.cgs.contains(32));
    assert!(gallery.bgms.contains(0) && gallery.bgms.contains(32) && gallery.bgms.contains(65));
    assert!(gallery.movies.contains(0));
    assert_eq!(
        loaded_layer_unlocks(data.info(), 6, 1),
        [
            GalleryUnlock::Music(80),
            GalleryUnlock::Picture(90),
            GalleryUnlock::Movie(1)
        ]
    );
    assert_eq!(
        loaded_layer_unlocks(data.info(), 6, 2),
        [GalleryUnlock::Movie(2)]
    );
    assert_eq!(
        loaded_layer_unlocks(data.info(), 6, 3),
        [
            GalleryUnlock::Music(1),
            GalleryUnlock::Picture(1),
            GalleryUnlock::Movie(3)
        ]
    );
}

#[test]
fn ger_gallery_ignores_non_gallery_layers_invalid_metadata_and_silent_replay() {
    let data = Metadata::new(&[32767], &[32767], &[(0, 0)]);
    for kind in [-1, 0, 1, 3, 4, 5, 7, 8, 9, 10, i32::MIN, i32::MAX] {
        assert!(loaded_layer_unlocks(data.info(), kind, 0).is_empty());
    }
    for kind in [2, 6] {
        for id in [-1, i32::MIN, 1, 65535, i32::MAX] {
            assert!(loaded_layer_unlocks(data.info(), kind, id).is_empty());
        }
    }
    let mut gallery = GalleryUnlocks::default();
    assert!(!gallery.apply(loaded_layer_unlocks(data.info(), 6, 0), true));
    assert!(
        gallery.cgs.raw_words().is_empty()
            && gallery.bgms.raw_words().is_empty()
            && gallery.movies.raw_words().is_empty()
    );
    assert!(gallery.apply(loaded_layer_unlocks(data.info(), 6, 0), false));
    assert_eq!(gallery.bgms.size_in_words(), 1024);
    assert!(gallery.bgms.contains(32767));
}

#[test]
fn ger_loaded_global_physical_tails_survive_publication_and_repeated_links() {
    let mut gallery = GalleryUnlocks {
        cgs: IndexSet::from_raw_words(vec![0, 0, 0, 0]),
        bgms: IndexSet::from_raw_words(vec![0x80000000, 0, 0]),
        movies: IndexSet::from_raw_words(vec![1, 0]),
    };
    let data = Metadata::new(&[32], &[31], &[(0, 0)]);
    assert!(gallery.apply(loaded_layer_unlocks(data.info(), 6, 0), false));
    assert_eq!(gallery.cgs.raw_words(), &[1, 0, 0, 0]);
    assert_eq!(gallery.bgms.raw_words(), &[0x80000001, 1, 0]);
    assert_eq!(gallery.movies.raw_words(), &[1, 0]);
    assert!(!gallery.apply(loaded_layer_unlocks(data.info(), 6, 0), false));
}

struct Native {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    output: std::process::ChildStdout,
    operations: usize,
    bytes: usize,
}
impl Native {
    fn new(root: &std::path::Path) -> Self {
        use std::process::{Command, Stdio};
        let folder = root.join("gerokasu2/.local/gallery-hooks");
        std::fs::create_dir_all(&folder).unwrap();
        let binary = folder.join("native-gallery-hooks");
        let compiled = Command::new("clang")
            .args(["-std=c11", "-O2"])
            .arg(root.join("gerokasu2/native-gallery-hooks.c"))
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let mut child = Command::new(binary)
            .arg(root.join("gerokasu2/uncompressed_executable"))
            .arg(root.join("gerokasu2/.local/newrodin-medium.fnt"))
            .arg(root.join("gerokasu2/.local/system.fnt"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: child.stdout.take().unwrap(),
            child,
            operations: 0,
            bytes: 0,
        }
    }
    fn send(&mut self, words: &[u32]) {
        use std::io::Write;
        self.input
            .as_mut()
            .unwrap()
            .write_all(
                &words
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    fn compare(&mut self, gallery: &GalleryUnlocks) {
        use std::io::Read;
        let mut tag = [0; 4];
        self.output.read_exact(&mut tag).unwrap();
        assert_eq!(&tag, b"GAL2");
        self.bytes += 4;
        for set in [&gallery.cgs, &gallery.bgms, &gallery.movies] {
            self.output.read_exact(&mut tag).unwrap();
            let count = u32::from_le_bytes(tag) as usize;
            assert_eq!(
                count,
                set.size_in_words(),
                "native physical count, operation{}",
                self.operations
            );
            let mut bytes = vec![0; count * 4];
            self.output.read_exact(&mut bytes).unwrap();
            let words = bytes
                .chunks_exact(4)
                .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
                .collect::<Vec<_>>();
            assert_eq!(
                words,
                set.raw_words(),
                "native words, operation{}",
                self.operations
            );
            self.bytes += 4 + bytes.len();
        }
        self.operations += 1;
    }
    fn metadata(&mut self, info: GalleryMetadata<'_>, gallery: &GalleryUnlocks) {
        self.send(&[
            1,
            0,
            0,
            info.pictures.len() as u32,
            info.bgms.len() as u32,
            info.movies.len() as u32,
        ]);
        for picture in info.pictures {
            self.send(&[i32::from(picture.linked_cg_id) as u32]);
        }
        for bgm in info.bgms {
            self.send(&[i32::from(bgm.linked_bgm_id) as u32]);
        }
        for movie in info.movies {
            self.send(&[
                i32::from(movie.linked_picture_id) as u32,
                i32::from(movie.linked_bgm_id) as u32,
            ]);
        }
        self.compare(gallery);
    }
    fn reset(&mut self) -> GalleryUnlocks {
        self.send(&[0; 6]);
        let gallery = GalleryUnlocks::default();
        self.compare(&gallery);
        gallery
    }
    fn loaded(
        &mut self,
        gallery: &mut GalleryUnlocks,
        info: GalleryMetadata<'_>,
        kind: i32,
        id: i32,
    ) {
        self.send(&[2, kind as u32, id as u32, 0, 0, 0]);
        gallery.apply(loaded_layer_unlocks(info, kind, id), false);
        self.compare(gallery);
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        self.input.take();
        let status = self.child.wait().unwrap();
        if !std::thread::panicking() {
            assert!(status.success());
        }
    }
}

#[test]
#[ignore = "executes original Ger gallery callback and lookups with synthetic and supplied-ROM metadata; CPU only"]
fn ger_native_loaded_resource_gallery_hooks_match_all_metadata_branches() {
    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    use std::io::Read;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let mut native = Native::new(root);
    let links = [i16::MIN, -1, 0, 1, 4, 31, 32, 8191, 8192, i16::MAX];
    let ids = [
        i32::MIN,
        -1,
        0,
        1,
        2,
        4,
        5,
        6,
        31,
        32,
        8192,
        32767,
        65535,
        i32::MAX,
    ];
    for profile in 0..256 {
        let mut random = (profile as u32) ^ 0xdeadbeef;
        let mut next = || {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            links[((random >> 16) as usize) % links.len()]
        };
        let data = Metadata::new(
            &(0..profile % 7).map(|_| next()).collect::<Vec<_>>(),
            &(0..(profile / 7) % 7).map(|_| next()).collect::<Vec<_>>(),
            &(0..(profile / 49) % 7)
                .map(|_| (next(), next()))
                .collect::<Vec<_>>(),
        );
        let mut gallery = native.reset();
        native.metadata(data.info(), &gallery);
        for kind in [2, 6] {
            for id in ids {
                for _ in 0..2 {
                    native.loaded(&mut gallery, data.info(), kind, id);
                }
            }
        }
        for kind in [-1, 0, 1, 3, 4, 5, 7, 8, 9, 10, i32::MIN, i32::MAX] {
            native.loaded(&mut gallery, data.info(), kind, 0);
        }
    }
    let rom_path = std::env::var_os("SHIN_GEROKASU2_ROM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("gerokasu2/data.rom"));
    let mut rom =
        RomReader::new(StatelessFile::new(std::fs::File::open(rom_path).unwrap()).unwrap())
            .unwrap();
    let file = rom.find_file("/main.snr").unwrap();
    let mut bytes = Vec::new();
    rom.open_file(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    let scenario = Gerokasu2Scenario::new(bytes.into()).unwrap();
    let info = GalleryMetadata::from(&scenario);
    let mut gallery = native.reset();
    native.metadata(info, &gallery);
    for (kind, count) in [(2, info.pictures.len()), (6, info.movies.len())] {
        for id in 0..count {
            for _ in 0..2 {
                native.loaded(&mut gallery, info, kind, id as i32);
            }
        }
    }
    println!(
        "GEROKASU2_GALLERY_HOOKS_NATIVE profiles=256 authored_pictures={} authored_bgms={} authored_movies={} operations={} compared_bytes={} original_callback_lookups_growth_all_physical_words=verified SDK_resource_publication=excluded",
        info.pictures.len(),
        info.bgms.len(),
        info.movies.len(),
        native.operations,
        native.bytes
    );
}
