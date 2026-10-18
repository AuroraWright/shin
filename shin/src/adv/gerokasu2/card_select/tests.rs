use super::*;
fn snapshot(m: &CardSelectModel) -> Vec<u8> {
    let mut out = b"GCS1".to_vec();
    for w in [
        m.phase as u32,
        m.selected as u32,
        m.countdown as u32,
        m.hover.map_or(u32::MAX, |i| i as u32),
        m.automatic as u32,
        m.stagger.to_bits(),
        m.elapsed.to_bits(),
        m.cues,
        m.cards.len() as u32,
        m.caption_y.value().to_bits(),
        m.caption_alpha.value().to_bits(),
    ] {
        out.extend(w.to_le_bytes());
    }
    for (i, c) in m.cards.iter().enumerate() {
        for w in [i as u32, c.face, c.delay.to_bits()] {
            out.extend(w.to_le_bytes());
        }
        for t in &c.tweens {
            for w in [
                t.value().to_bits(),
                t.target_value().to_bits(),
                (!t.is_idle()) as u32,
            ] {
                out.extend(w.to_le_bytes());
            }
        }
    }
    let result = (u64::from(m.cards[m.selected].face) << 32) | u64::from(m.phase < 0);
    out.extend(result.to_le_bytes());
    out
}
#[test]
fn ger_card_select_requires_ready_input_and_preserves_face_ids() {
    let mut m = CardSelectModel::new(5, 42).unwrap();
    assert_eq!(m.cards.iter().map(|c| c.face).collect::<Vec<_>>(), [0, 2]);
    assert!(!m.confirm());
    m.listener = true;
    m.update(Ticks::from_f32(180.0));
    m.update(Ticks::from_f32(1.0));
    assert!(m.ready());
    assert!(m.select(1));
    assert!(m.confirm());
    assert_eq!(m.result(), None);
    m.update(Ticks::from_f32(180.0));
    m.update(Ticks::from_f32(1.0));
    assert_eq!(m.result(), Some(2));
    assert!(CardSelectModel::new(0, 42).is_err());
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
#[ignore = "original CardSelect constructor/phase/tween/shuffle bodies;CPU only"]
fn ger_native_card_constructor_tweens_phases_selection_confirm_and_result() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let folder = root.join("reverse-engineering/gerokasu2/card-select");
    fs::create_dir_all(&folder).unwrap();
    let binary = folder.join("native-card-select");
    let compile = Command::new("clang")
        .args(["-std=c11", "-O2"])
        .arg(root.join("gerokasu2/native-card-select.c"))
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
    fn record(
        request: &mut Vec<u8>,
        expected: &mut Vec<Vec<u8>>,
        row: [u32; 4],
        m: &CardSelectModel,
    ) {
        request.extend(row.into_iter().flat_map(u32::to_le_bytes));
        expected.push(snapshot(m));
    }
    for mask in (1..16).chain([0x1000, 0x1001, 0x1002, 0x1003, 0x100f]) {
        for seed in [0, 1, 42, 12345, 2147483647, u32::MAX] {
            let mut m = CardSelectModel::new(mask, seed).unwrap();
            m.listener = true;
            record(&mut request, &mut expected, [0, mask as u32, seed, 0], &m);
            for delta in [0.0, 1.0, 15.0, 30.0, 180.0, 1.0] {
                m.update(Ticks::from_f32(delta));
                record(&mut request, &mut expected, [1, delta.to_bits(), 0, 0], &m);
            }
            m = m.render_clone_model();
            record(&mut request, &mut expected, [12, 0, 0, 0], &m);
            m.update(Ticks::from_f32(0.0));
            record(&mut request, &mut expected, [1, 0, 1, 0], &m);
            m.listener = true;
            record(&mut request, &mut expected, [13, 0, 0, 0], &m);
            if !m.automatic {
                let x = m.cards[0].tweens[2].value();
                let y = m.cards[0].tweens[3].value();
                for (dx, dy) in [
                    (-138.0, -196.0),
                    (138.0, 0.0),
                    (0.0, 196.0),
                    (0.0, 0.0),
                    (-139.0, 0.0),
                    (0.0, -197.0),
                ] {
                    let p = glam::Vec2::new(x + dx, y + dy);
                    m.pointer_move(p);
                    record(
                        &mut request,
                        &mut expected,
                        [7, p.x.to_bits(), p.y.to_bits(), 0],
                        &m,
                    );
                }
                m.pointer_move(glam::Vec2::new(x, y));
                record(
                    &mut request,
                    &mut expected,
                    [7, x.to_bits(), y.to_bits(), 0],
                    &m,
                );
                m.pointer_press();
                record(&mut request, &mut expected, [8, 0, 0, 0], &m);
                m.pointer_cancel();
                record(&mut request, &mut expected, [10, 0, 0, 0], &m);
                let i = m.cards.len() - 1;
                m.select(i);
                record(&mut request, &mut expected, [6, i as u32, 0, 0], &m);
                m.update(Ticks::from_f32(2.0));
                record(&mut request, &mut expected, [1, 2f32.to_bits(), 1, 0], &m);
                let x = m.cards[i].tweens[2].value();
                let y = m.cards[i].tweens[3].value();
                m.pointer_move(glam::Vec2::new(x, y));
                record(
                    &mut request,
                    &mut expected,
                    [7, x.to_bits(), y.to_bits(), 0],
                    &m,
                );
                m.pointer_press();
                record(&mut request, &mut expected, [8, 0, 0, 0], &m);
                m.pointer_release();
                record(&mut request, &mut expected, [9, 0, 0, 0], &m);
            }
            for _ in 0..35 {
                m.update(Ticks::from_f32(7.0));
                record(&mut request, &mut expected, [1, 7f32.to_bits(), 1, 0], &m);
            }
            m.fast_forward();
            record(&mut request, &mut expected, [3, 0, 0, 0], &m);
            m.cancel();
            record(&mut request, &mut expected, [5, 0, 0, 0], &m);
        }
    }
    let input = folder.join("request.bin");
    fs::write(&input, request).unwrap();
    let output = Command::new(binary)
        .arg(root.join("gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
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
        let actual = &output.stdout[at..at + e.len()];
        let differences: Vec<_> = actual
            .chunks_exact(4)
            .zip(e.chunks_exact(4))
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(word, (a, b))| {
                (
                    word,
                    u32::from_le_bytes(a.try_into().unwrap()),
                    u32::from_le_bytes(b.try_into().unwrap()),
                )
            })
            .take(12)
            .collect();
        assert!(
            differences.is_empty(),
            "card snapshot{i} word/native/Rust {differences:?}"
        );
        at += e.len();
    }
    assert_eq!(at, output.stdout.len());
    println!(
        "GEROKASU2_CARD_MODEL_NATIVE snapshots={} compared_bytes={} ctor_shuffle_7tweens_phases_selection_confirm_auto_skip_result=verified",
        expected.len(),
        at
    );
}
