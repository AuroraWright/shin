use super::*;
use std::process::Command;

#[test]
#[ignore = "actual updated full save writer/reader with all256 choice bytes; no audio/GPU/user files"]
fn higurashi_runtime_choice_bytes_use_native_four_bit_disk_projection() {
    let output = Command::new(
        std::env::var("SHIN_HIGURASHI_NATIVE_SAVE_CHOICE_BYTES")
            .unwrap_or_else(|_| "/tmp/higurashi-native-save-choice-bytes".into()),
    )
    .args([
        "higurashi/uncompressed_executable_update",
        "/tmp/higurashi-newrodin.fnt",
    ])
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    let mut profiles = 0;
    for line in actual.lines() {
        let words = line.split_whitespace().collect::<Vec<_>>();
        assert_eq!(words.len(), 5);
        assert_eq!(words[0], "C");
        let length = words[1].parse::<usize>().unwrap();
        let salt = words[2].parse::<usize>().unwrap();
        let raw = (0..words[4].len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&words[4][i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let mut settings = SaveData::default();
        settings.save_vectors.select_last_choices = (0..length)
            .map(|i| ((i * 37 + salt * 23) & 255) as u8)
            .collect();
        let original = settings.clone();
        let slots = Slots::loaded(&settings);
        let projection = slots.disk(&settings);
        let encoded = projection.encode_raw().unwrap();
        assert_eq!(
            encoded, raw,
            "original complete save bitstream length{length}/salt{salt}"
        );
        assert_eq!(
            serde_json::to_value(SaveData::decode_raw(raw).unwrap()).unwrap(),
            serde_json::to_value(projection).unwrap()
        );
        assert_eq!(
            serde_json::to_value(settings).unwrap(),
            serde_json::to_value(original).unwrap(),
            "disk projection changed live settings"
        );
        profiles += 1;
    }
    assert_eq!(profiles, 30);
    eprintln!(
        "Runtime choice disk projection:{profiles} original complete bitstreams,all256 byte values,odd/even lengths,live full-byte preservation and original decoded snapshot pass;no audio/GPU/user saves"
    );
    if let Ok(directory) = std::env::var("SHIN_HIGURASHI_SAVE_CHOICE_BYTES_CAPTURE") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            std::path::Path::new(&directory).join("native-output.txt"),
            actual,
        )
        .unwrap();
        std::fs::write(
            std::path::Path::new(&directory).join("native-stderr.txt"),
            output.stderr,
        )
        .unwrap();
    }
}
