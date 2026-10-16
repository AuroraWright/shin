use super::*;
use std::{
    io::Read,
    sync::atomic::{AtomicU64, Ordering},
};

struct Assets(PathBuf);
impl Assets {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "shin-mod-rom-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn rom(&self, name: &str, version: u32, files: &[(&str, &[u8])]) {
        // A real ROM2 index and payload, without depending on game files.
        let index_size =
            4 + files.len() * 12 + files.iter().map(|(n, _)| n.len() + 1).sum::<usize>();
        let mut data = b"ROM2".to_vec();
        for word in [version, index_size as u32, 1, 0, 0, 0, 0] {
            data.extend_from_slice(&word.to_le_bytes());
        }
        data.extend_from_slice(&(files.len() as u32).to_le_bytes());
        let mut name_offset = 4 + files.len() * 12;
        let mut payload_offset = 32 + index_size;
        for (name, bytes) in files {
            for word in [
                name_offset as u32,
                payload_offset as u32,
                bytes.len() as u32,
            ] {
                data.extend_from_slice(&word.to_le_bytes());
            }
            name_offset += name.len() + 1;
            payload_offset += bytes.len();
        }
        for (name, _) in files {
            data.extend_from_slice(name.as_bytes());
            data.push(0);
        }
        for (_, bytes) in files {
            data.extend_from_slice(bytes);
        }
        std::fs::write(self.0.join(name), data).unwrap();
    }
}
impl Drop for Assets {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn read(io: &LayeredAssetIo, path: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    io.read_file(path)
        .unwrap()
        .cursor()
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

#[test]
fn mod_rom_priority_and_sparse_fallback_for_every_game() {
    // Gerokasu2's data.rom uses the same ROM2 version as Umineko.
    for (game, version) in [
        (Game::Umineko, 0x10001),
        (Game::Higurashi, 1),
        (Game::Gerokasu2, 0x10001),
    ] {
        let assets = Assets::new();
        assets.rom(
            "data.rom",
            version,
            &[("shared", b"base"), ("base-only", b"base-only")],
        );
        assets.rom(
            "patch.rom",
            version,
            &[("shared", b"patch"), ("patch-only", b"patch-only")],
        );
        std::fs::create_dir(assets.0.join("patch")).unwrap();
        std::fs::write(assets.0.join("patch/shared"), b"loose-patch").unwrap();
        assets.rom("mod.rom", version, &[("shared", b"mod")]);
        let io = locate_assets_for_game(game, Some(&assets.0)).unwrap();
        assert_eq!(read(&io, "/shared"), b"mod");
        assert_eq!(read(&io, "/patch-only"), b"patch-only");
        assert_eq!(read(&io, "/base-only"), b"base-only");
        drop(io);
        std::fs::remove_file(assets.0.join("mod.rom")).unwrap();
        let io = locate_assets_for_game(game, Some(&assets.0)).unwrap();
        assert_eq!(read(&io, "/shared"), b"loose-patch");
    }
}

#[test]
#[ignore = "requires supplied original/updated Higurashi main.snr in SHIN_HIGURASHI_ASSETS; CPU only"]
fn mod_rom_selected_scenario_controls_append_and_overrides_all_rom_layers() {
    use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
    let root = PathBuf::from(std::env::var_os("SHIN_HIGURASHI_ASSETS").unwrap());
    let scenario = |name: &str| {
        let mut rom = RomReader::new(
            StatelessFile::new(std::fs::File::open(root.join(name)).unwrap()).unwrap(),
        )
        .unwrap();
        let file = rom.find_file("/main.snr").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    };
    let original = scenario("data.rom");
    let updated = scenario("patch.rom");
    for (selected, origin) in [(&original, false), (&updated, true)] {
        let assets = Assets::new();
        assets.rom(
            "data.rom",
            1,
            &[("main.snr", &original), ("shared", b"base")],
        );
        assets.rom(
            "patch.rom",
            1,
            &[("main.snr", &updated), ("shared", b"patch")],
        );
        assets.rom(
            "append.rom",
            1,
            &[
                ("append.txt", b"append"),
                ("shared", b"append"),
                ("append-only", b"append-only"),
            ],
        );
        assets.rom("mod.rom", 1, &[("main.snr", selected), ("shared", b"mod")]);
        let io = locate_assets_for_game(Game::Higurashi, Some(&assets.0)).unwrap();
        assert_eq!(read(&io, "/main.snr"), *selected);
        assert_eq!(read(&io, "/shared"), b"mod");
        assert_eq!(io.read_file("/append.txt").is_ok(), origin);
        assert_eq!(io.read_file("/append-only").is_ok(), origin);
    }
}
