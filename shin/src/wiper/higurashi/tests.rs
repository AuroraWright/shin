use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
#[ignore = "executes updated Higurashi AArch64 clock and CPU draw; no GPU"]
fn higurashi_native_default_wipe_clock_and_draw() {
    let mut input = String::new();
    let mut expected = Vec::new();
    let mut random = 0x624ec994u32;
    let durations = [
        0,
        1,
        2,
        60,
        120,
        16777215,
        16777217,
        i32::MAX as u32,
        0x80000000,
        u32::MAX,
    ];
    for duration in durations {
        let mut wipe = HigurashiDefaultWiper::new(duration);
        input.push_str(&format!("N {duration}\n"));
        expected.push((wipe.clone(), 0u32));
        for step in 0..256 {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            if step == 240 {
                input.push_str("F\n");
                wipe.fast_forward();
                expected.push((wipe.clone(), 0));
            } else {
                let delta = match step % 12 {
                    0 => 0.0,
                    1 => 0.125,
                    2 => -0.25,
                    3 => duration as f32 * 0.125,
                    4 => 1.0,
                    5 => 10.0,
                    6 => -1.0,
                    _ => (random % 1000) as f32 / 16.0,
                };
                input.push_str(&format!("U {:08x}\n", delta.to_bits()));
                let running = u32::from(wipe.advance(Ticks::from_f32(delta)));
                expected.push((wipe.clone(), running));
            }
        }
    }
    let mut child = Command::new(std::env::var("SHIN_HIGURASHI_NATIVE_DEFAULT_WIPE").unwrap())
        .arg(std::env::var("SHIN_HIGURASHI_EXE").unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.lines().count(), expected.len());
    let matrix = crate::render::normalized_projection_matrix()
        .to_cols_array()
        .map(f32::to_bits);
    let vertices = shin_render::quad_vertices::build_quad_vertices(|v| {
        [v.x.to_bits(), v.y.to_bits(), v.x.to_bits(), v.y.to_bits()]
    });
    for (row, (native, (wipe, running))) in output.lines().zip(expected.iter()).enumerate() {
        let w: Vec<_> = native
            .split_whitespace()
            .skip(1)
            .map(|s| u32::from_str_radix(s, 16).unwrap())
            .collect();
        assert_eq!(w.len(), 41);
        assert_eq!(&w[..2], &wipe.time_words(), "clock row{row}");
        assert_eq!(w[2], *running, "running row{row}");
        // Native 74d34 emits OpenGL depth [-1,1]. The shared renderer
        // converts it to WGPU [0,1]; compare after that explicit conversion.
        let native_words: [u32; 16] = w[3..19].try_into().unwrap();
        let mut native_matrix = native_words.map(f32::from_bits);
        for col in 0..4 {
            native_matrix[col * 4 + 2] =
                native_matrix[col * 4 + 2] * 0.5 + native_matrix[col * 4 + 3] * 0.5;
        }
        assert_eq!(
            native_matrix,
            matrix.map(f32::from_bits),
            "converted matrix row{row}"
        );
        assert_eq!(
            &w[19..23],
            &[wipe.progress().to_bits(), 0, 0, 0],
            "progress row{row}"
        );
        assert_eq!(&w[23..39], &vertices.concat(), "quad row{row}");
        assert_eq!(&w[39..], &[1, 21]);
    }
    println!(
        "Higurashi default wipe: {} native clock/draw packets match, including unsigned durations, completed updates, negative/fractional deltas and fast-forward",
        expected.len()
    );
}
