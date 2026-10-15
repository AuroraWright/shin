use super::*;
mod hooks;
#[test]
fn ger_gallery_replay_invalid_types_and_indices_leave_all_sets_unchanged() {
    let mut gallery = GalleryUnlocks::default();
    gallery.unlock(0, &[7, 8192], false).unwrap();
    for replay in [false, true] {
        gallery.unlock(255, &[-1], replay).unwrap();
        if replay {
            gallery.unlock(1, &[-1], true).unwrap();
        } else {
            assert!(gallery.unlock(1, &[0, -1], false).is_err());
        }
        assert!(gallery.cgs.contains(7) && gallery.cgs.contains(8192));
        assert_eq!(gallery.bgms.size_in_words(), 0);
        assert_eq!(gallery.movies.size_in_words(), 0);
    }
    let mut copy = gallery.clone();
    copy.unlock(0, &[8], false).unwrap();
    assert!(!gallery.cgs.contains(8));
}
fn snapshot(gallery: &GalleryUnlocks, locks: u32) -> Vec<u8> {
    let mut result = b"GGL1".to_vec();
    for word in [1, locks, locks, 0] {
        result.extend(word.to_le_bytes());
    }
    for set in [&gallery.cgs, &gallery.bgms, &gallery.movies] {
        result.extend((set.size_in_words() as u32).to_le_bytes());
        let mut words = vec![0u32; set.size_in_words()];
        for index in set.iter() {
            words[index / 32] |= 1 << (index & 31);
        }
        result.extend(words.into_iter().flat_map(u32::to_le_bytes));
    }
    result
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original Ger trophy/UNLOCK callbacks and bitset growth;CPU only"]
fn ger_native_switch_trophy_and_gallery_unlocks_match_shared_index_sets() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("gerokasu2/.local/gallery");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-gallery");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-gallery.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut request = Vec::new();
    let mut expected = Vec::new();
    fn row(out: &mut Vec<u8>, words: [u32; 4], values: &[i32]) {
        out.extend(words.into_iter().flat_map(u32::to_le_bytes));
        out.extend(values.iter().flat_map(|w| w.to_le_bytes()));
    }
    for seed in [1u32, 42, 0xdeadbeef, u32::MAX] {
        let mut gallery = GalleryUnlocks::default();
        let mut locks = 0;
        row(&mut request, [0; 4], &[]);
        expected.push(snapshot(&gallery, locks));
        for id in [0, 1, -1, 17, 65535, i32::MIN, i32::MAX] {
            for replay in [false, true] {
                trophy(id);
                row(&mut request, [2, id as u32, 0, replay as u32], &[]);
                locks += u32::from(!replay);
                expected.push(snapshot(&gallery, locks));
            }
        }
        let mut random = seed;
        for kind in [0, 1, 2, 3, 255] {
            for replay in [false, true] {
                for count in [0, 1, 8, 32] {
                    let mut values = Vec::new();
                    for i in 0..count {
                        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                        values.push(if i < 8 {
                            [0, 31, 32, 8191, 8192, 16383, 16384, 65535][i]
                        } else {
                            (random & 65535) as i32
                        });
                    }
                    for _ in 0..2 {
                        gallery.unlock(kind, &values, replay).unwrap();
                        row(
                            &mut request,
                            [1, u32::from(kind), count as u32, replay as u32],
                            &values,
                        );
                        expected.push(snapshot(&gallery, locks));
                    }
                }
            }
        }
    }
    let input = folder.join("request.bin");
    fs::write(&input, request).unwrap();
    let output = Command::new(binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("gerokasu2/.local/newrodin-medium.fnt"))
        .arg(root.join("gerokasu2/.local/system.fnt"))
        .stdin(Stdio::from(fs::File::open(input).unwrap()))
        .output()
        .unwrap();
    fs::write(folder.join("native.trace"), &output.stdout).unwrap();
    fs::write(folder.join("native.stderr"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut at = 0;
    for (i, e) in expected.iter().enumerate() {
        assert_eq!(&output.stdout[at..at + e.len()], e, "gallery snapshot{i}");
        at += e.len();
    }
    assert_eq!(at, output.stdout.len());
    println!(
        "GEROKASU2_GALLERY_NATIVE snapshots={} compared_bytes={} trophy_mutex_only_shared_bitset_all3_galleries_growth_replay=verified",
        expected.len(),
        at
    );
}
