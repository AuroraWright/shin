//! Literal original Ger13800/11D20 CPU submissions; no GPU/window/audio.
//! The standalone validator includes the production packet helpers verbatim.
use super::backdrop::{backdrop_plan, blur_packets, projection};
use std::{
    fmt::Write,
    io::Write as IoWrite,
    path::PathBuf,
    process::{Command, Stdio},
};

fn float_words(actual: &[u32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &b)) in actual.iter().zip(expected).enumerate() {
        assert!(
            a == b.to_bits() || (f32::from_bits(a) == 0. && b == 0.),
            "float {i}: native {a:08x}, Rust {:08x}",
            b.to_bits()
        );
    }
}

fn packet(
    line: &str,
    program: u32,
    blend: u32,
    source: u32,
    target: u32,
    matrix: &[f32; 16],
    distances: &[f32; 8],
    weights: &[f32; 4],
    vertices: &[f32],
    color: Option<u32>,
) {
    let words: Vec<u32> = line
        .split_whitespace()
        .skip(1)
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(&words[..4], &[program, blend, source, target]);
    float_words(&words[4..20], matrix);
    float_words(&words[20..28], distances);
    float_words(&words[28..32], weights);
    assert_eq!(words[32] as usize, vertices.len());
    assert_eq!(words.len(), 33 + vertices.len());
    if let Some(color) = color {
        for vertex in 0..4 {
            float_words(
                &words[33 + vertex * 6..36 + vertex * 6],
                &vertices[vertex * 6..vertex * 6 + 3],
            );
            assert_eq!(words[36 + vertex * 6], color);
            float_words(
                &words[37 + vertex * 6..39 + vertex * 6],
                &vertices[vertex * 6 + 4..vertex * 6 + 6],
            );
        }
    } else {
        float_words(&words[33..], vertices);
    }
}

#[test]
#[ignore = "requires original executable/fonts and standalone native-modal-render CPU probe"]
fn ger_native_modal_capture_and_backdrop_packets_match() {
    let root = PathBuf::from(std::env::var("GER_MODAL_ROOT").unwrap());
    let out = root.join("reverse-engineering/gerokasu2/modal-render");
    let binary = std::env::var_os("GER_MODAL_NATIVE")
        .map(PathBuf::from)
        .unwrap_or_else(|| out.join("native-modal-render"));
    let sizes = [
        (1920, 1080),
        (1280, 720),
        (3840, 2160),
        (1919, 1079),
        (853, 479),
        (1, 1),
    ];
    let mut fades = vec![
        -1., -0., 0., 0.00001, 0.125, 0.25, 0.5, 0.75, 0.999, 1., 1.25, 2.,
    ];
    // All byte quantization transitions, including the representable neighbors.
    for i in 1..255 {
        let f = i as f32 / 255.;
        fades.extend([
            f32::from_bits(f.to_bits() - 1),
            f,
            f32::from_bits(f.to_bits() + 1),
        ]);
    }
    let mut input = String::new();
    for (w, h) in sizes {
        for back in 0..2 {
            for mask in 0..4 {
                writeln!(input, "C {w} {h} {back} {mask}").unwrap();
            }
        }
    }
    for has in 0..2 {
        for &fade in &fades {
            writeln!(input, "D {has} {:08x}", fade.to_bits()).unwrap();
        }
    }
    std::fs::write(out.join("native-modal-render.input"), &input).unwrap();
    let mut command = Command::new(binary);
    command
        .arg(root.join("game-data/gerokasu2/uncompressed_executable"))
        .arg(root.join("test-fixtures/gerokasu2/system.fnt"))
        .arg(root.join("test-fixtures/gerokasu2/newrodin-medium.fnt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    // The corpus is larger than a pipe: write while the main thread drains output.
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    std::fs::write(out.join("native-modal-render.output"), &result.stdout).unwrap();
    std::fs::write(out.join("native-modal-render.stderr"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = String::from_utf8(result.stdout).unwrap();
    let mut lines = text.lines();
    let mut profile = 0;
    let mut captures = 0;
    let mut backdrops = 0;
    let mut packets = 0;
    for (w, h) in sizes {
        for back in 0..2 {
            for mask in 0..4 {
                assert_eq!(
                    lines.next().unwrap(),
                    format!("C {profile} {w} {h} {back} {mask}")
                );
                for expected in [
                    format!("A 0 {w} {h} 0"),
                    format!("A 1 {w} {h} 0"),
                    "B 1 0".into(),
                    "K 1 4278190080 0 1065353216".into(),
                    "E".into(),
                    format!("Z {w} {h}"),
                ] {
                    assert_eq!(lines.next().unwrap(), expected);
                }
                for slot in 1..=2 {
                    let id = if mask & (1 << (slot - 1)) != 0 {
                        1 + back * 2 + slot - 1
                    } else {
                        0
                    };
                    assert_eq!(lines.next().unwrap(), format!("L {back} {slot} {id}"));
                }
                for expected in [
                    format!("N {back} 1 0"),
                    format!("N {back} 2 0"),
                    format!("Q {back} 0"),
                    "B 0 1".into(),
                    "K 7 4278190080 0 1065353216".into(),
                    format!("R {back} 0"),
                    "E".into(),
                ] {
                    assert_eq!(lines.next().unwrap(), expected);
                }
                for slot in 1..=2 {
                    if mask & (1 << (slot - 1)) != 0 {
                        assert_eq!(
                            lines.next().unwrap(),
                            format!("S {back} {slot} {}", 1 + back * 2 + slot - 1)
                        );
                    }
                }
                assert_eq!(lines.next().unwrap(), format!("Q {back} {mask}"));
                for (i, expected) in blur_packets(w, h).into_iter().enumerate() {
                    assert_eq!(
                        lines.next().unwrap(),
                        if i == 0 { "B 1 0" } else { "B 0 0" }
                    );
                    assert_eq!(lines.next().unwrap(), "W");
                    packet(
                        lines.next().unwrap(),
                        44,
                        1,
                        i as u32,
                        1 - i as u32,
                        &expected.projection,
                        &expected.distances,
                        &expected.weights,
                        &expected.vertices,
                        None,
                    );
                    assert_eq!(lines.next().unwrap(), "E");
                    packets += 1;
                }
                assert_eq!(lines.next().unwrap(), format!("F 0 2 1 {mask} {mask}"));
                profile += 1;
                captures += 1;
            }
        }
    }
    let vertices = [
        0., 0., 0., 0., 0., 0., 1920., 0., 0., 0., 1., 0., 0., 1080., 0., 0., 0., 1., 1920., 1080.,
        0., 0., 1., 1.,
    ];
    for has in [false, true] {
        for &fade in &fades {
            assert_eq!(
                lines.next().unwrap(),
                format!("D {profile} {} {}", has as u8, fade.to_bits())
            );
            let plan = backdrop_plan(has, fade);
            if plan.render_root {
                assert_eq!(lines.next().unwrap(), "V");
            }
            if plan.draw {
                packet(
                    lines.next().unwrap(),
                    2,
                    plan.blend,
                    0,
                    u32::MAX,
                    &projection(),
                    &[0.; 8],
                    &[0.; 4],
                    &vertices,
                    Some(plan.color),
                );
                packets += 1;
            }
            for expected in ["I", "M", "F"] {
                assert_eq!(lines.next().unwrap(), expected);
            }
            profile += 1;
            backdrops += 1;
        }
    }
    assert_eq!(lines.next(), None);
    eprintln!(
        "GER_MODAL_PACKETS captures={captures} backdrops={backdrops} packets={packets} native_capture_ui_restore_extent_blur_and_backdrop=exact"
    );
}
