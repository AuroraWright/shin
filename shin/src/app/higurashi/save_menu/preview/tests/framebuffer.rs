use super::*;
use crate::adv::higurashi::reference_gpu_assets;
use std::sync::atomic::{AtomicU32, Ordering};

#[test]
#[ignore = "actual original b83f8/b8454/root+three6b0c0 group constructors,16 OS seeds and final root/saved-screen seed overrides;bounded original NVN transport plus GPU asset server,no audio/windows/user saves"]
fn higurashi_saved_preview_framebuffer_constructor_matches_native_entropy_and_screen_seeds() {
    let (_, _, server) = reference_gpu_assets();
    let server = Arc::new(server);
    let authored: Arc<HigurashiScenario> = server.load_sync("/main.snr").unwrap();
    let mut input = String::new();
    let mut expected = Vec::new();
    for canvas in [(1280, 720), (1920, 1080)] {
        let count = Arc::new(AtomicU32::new(0xabc00001));
        let seeds: SeedSource = {
            let count = count.clone();
            Arc::new(move || count.fetch_add(1, Ordering::SeqCst))
        };
        let preview = PreviewService::new(server.clone(), authored.clone(), canvas, true, 0, seeds);
        assert_eq!(
            count.load(Ordering::SeqCst),
            0xabc00011,
            "b8454 consumes16 OS values"
        );
        let size = resources::thumbnail_size(canvas);
        input += &format!("N {} {}\n", size.0, size.1);
        expected.push((size, preview.group_seeds));
    }
    let mut child =
        Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_PREVIEW_FRAMEBUFFER").unwrap())
            .args([
                std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
                std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    if let Ok(path) = std::env::var("SHIN_HIGURASHI_SAVE_PREVIEW_FRAMEBUFFER_CAPTURE") {
        let path = std::path::Path::new(&path);
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("input.txt"), input).unwrap();
        std::fs::write(path.join("output.txt"), &result.stdout).unwrap();
        std::fs::write(path.join("stderr.txt"), &result.stderr).unwrap();
    }
    let output = String::from_utf8(result.stdout).unwrap();
    assert_eq!(output.lines().count(), 2);
    for (row, (size, seeds)) in output.lines().zip(expected) {
        let words = row.split_whitespace().collect::<Vec<_>>();
        assert_eq!(words.len(), 25);
        assert_eq!(&words[..4], &["N", "16", "0", "0"]);
        assert_eq!(
            (
                words[4].parse::<u32>().unwrap(),
                words[5].parse::<u32>().unwrap()
            ),
            size
        );
        assert_eq!(&words[6..9], &["N4shin11ScreenLayerE", "2", "1"]);
        let root: [u32; 4] = std::array::from_fn(|i| words[9 + i].parse().unwrap());
        let screen: [u32; 4] = std::array::from_fn(|i| words[17 + i].parse().unwrap());
        assert_eq!(seeds, [root, screen]);
    }
    eprintln!(
        "Saved preview framebuffer:2 actual constructors at228x128/342x192;16 OS samples,root first4 and second-plane screen[1,2,seeds11,12] match,bounded texture/depth/opaque-clear resources retire;no MT advance/audio/windows/user saves"
    );
}

#[test]
fn higurashi_saved_preview_os_entropy_does_not_advance_global_mt() {
    crate::engine_random::with_reference_seed(5489, || {
        let before = crate::engine_random::seed_call_count();
        for _ in 0..20 {
            super::super::os_seed();
        }
        assert_eq!(crate::engine_random::seed_call_count(), before);
        assert_eq!(crate::engine_random::next_seed(), 3499211612);
    });
}
