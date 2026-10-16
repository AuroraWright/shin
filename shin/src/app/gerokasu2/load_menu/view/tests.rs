use super::*;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
#[test]
#[ignore = "executes original Ger SaveMenu artwork/digit/footer/scrollbar bodies; CPU only"]
fn ger_native_load_artwork_and_scrollbar_match_original_packets() {
    compare_artwork(crate::adv::gerokasu2::save_menu::Mode::Load);
}
#[test]
#[ignore = "executes original Ger Save caption/footer/artwork bodies; CPU only"]
fn ger_native_save_artwork_and_scrollbar_match_original_packets() {
    compare_artwork(crate::adv::gerokasu2::save_menu::Mode::Save);
}
fn compare_artwork(mode: crate::adv::gerokasu2::save_menu::Mode) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let dir = root.join(if mode == crate::adv::gerokasu2::save_menu::Mode::Load {
        "gerokasu2/.local/save-menu-render"
    } else {
        "gerokasu2/.local/save-menu-render/save"
    });
    std::fs::create_dir_all(&dir).unwrap();
    let binary = std::env::var_os("SHIN_GEROKASU2_SAVE_RENDER_ORACLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join("native-save-menu-render"));
    if std::env::var_os("SHIN_GEROKASU2_SAVE_RENDER_ORACLE").is_none() {
        assert!(
            Command::new("clang")
                .args(["-std=c11", "-O2"])
                .arg(root.join("gerokasu2/native-save-menu-render.c"))
                .arg("-o")
                .arg(&binary)
                .status()
                .unwrap()
                .success()
        );
    }
    let mut input = String::new();
    let mut expected = vec![];
    for progress in [0., 0.25, 0.75, 1.] {
        for scroll in [0., 202.5, 19296.] {
            for selected in [0, 37, 99] {
                for occupied in [false, true] {
                    for flash in [0., 0.7] {
                        input.push_str(&format!(
                            "{selected} {} {progress} {scroll} {flash} {flash}\n",
                            u8::from(occupied)
                        ));
                        let dates = std::array::from_fn(|_| {
                            occupied.then_some(SaveDate {
                                year: 2026,
                                month: 10,
                                day: 15,
                                hour: 9,
                                minute: 7,
                                ..Default::default()
                            })
                        });
                        expected.push(
                            art::plan_with_mode(
                                mode,
                                progress,
                                scroll,
                                selected,
                                &dates,
                                |i| if i == selected { flash } else { 0. },
                                flash,
                            )
                            .into_iter()
                            .filter(|p| !matches!(p.kind, art::Kind::Text(_)))
                            .collect::<Vec<_>>(),
                        );
                    }
                }
            }
        }
    }
    std::fs::write(dir.join("profiles.txt"), &input).unwrap();
    let output_path = dir.join("original-output.txt");
    let mut child = Command::new(&binary)
        .env("SHIN_GEROKASU2_SAVE_RENDER_MODE", (mode as u8).to_string())
        .args([
            root.join("gerokasu2/uncompressed_executable"),
            root.join("gerokasu2/.local/newrodin-medium.fnt"),
            root.join("gerokasu2/.local/system.fnt"),
        ])
        .stdin(Stdio::piped())
        .stdout(std::fs::File::create(&output_path).unwrap())
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
    let output = std::fs::read_to_string(output_path).unwrap();
    let mut case = 0;
    let mut packet = 0;
    let mut quads = 0;
    let mut max_xy = 0f32;
    for line in output.lines() {
        let w = line.split_whitespace().collect::<Vec<_>>();
        match w[0] {
            "R" => {
                case = w[1].parse::<usize>().unwrap() - 1;
                packet = 0;
            }
            "E" => assert_eq!(packet, expected[case].len(), "profile{case}"),
            "P" => {
                let blend = w[1].parse::<u32>().unwrap();
                let shader = w[2].parse::<u32>().unwrap();
                let n = w[5].parse::<usize>().unwrap();
                let first = w[6].parse::<usize>().unwrap();
                let word = |i: usize| w[i].parse::<u32>().unwrap();
                let f = |i: usize| f32::from_bits(word(i));
                let stride = match shader {
                    1 => 4,
                    7 => 8,
                    _ => 6,
                };
                let matrix = glam::Mat4::from_cols_array(&std::array::from_fn(|i| f(i + 8)));
                // Original scrollbar submits three strips as one8-vertex packet,
                // with repeated joins; expand it to the same six ordinary quads.
                let offsets = if n == 8 {
                    vec![0, 2, 4]
                } else {
                    assert_eq!(n, 4);
                    vec![0]
                };
                for offset in offsets {
                    let p = &expected[case][packet];
                    let a = 32 + (first + offset) * stride;
                    let project = |at| {
                        let xy = matrix * glam::vec4(f(at), f(at + 1), f(at + 2), 1.);
                        glam::vec2((xy.x + 1.) * 960., (1. - xy.y) * 540.)
                    };
                    for (vertex, point) in [
                        glam::vec2(p.rect.x, p.rect.y),
                        glam::vec2(p.rect.z, p.rect.y),
                        glam::vec2(p.rect.x, p.rect.w),
                        glam::vec2(p.rect.z, p.rect.w),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let native = project(a + vertex * stride);
                        let difference = (native - point).abs().max_element();
                        max_xy = max_xy.max(difference);
                        assert!(
                            difference <= 0.001,
                            "profile{case} packet{packet} {} vertex{vertex} native{native:?} port{point:?}",
                            p.texture
                        );
                        assert_eq!(
                            word(a + vertex * stride + 3),
                            p.color,
                            "profile{case} packet{packet} vertex{vertex} color"
                        );
                    }
                    let expected_shader = match p.kind {
                        art::Kind::Window { .. } => 7,
                        art::Kind::Fill => 1,
                        art::Kind::Button { flash } if flash > 0. => 6,
                        _ => 2,
                    };
                    assert_eq!(shader, expected_shader, "profile{case} packet{packet}");
                    if shader != 1 {
                        assert_eq!(w[3], p.texture, "profile{case} packet{packet}");
                        for (vertex, uv) in [
                            [p.uv.x, p.uv.y],
                            [p.uv.z, p.uv.y],
                            [p.uv.x, p.uv.w],
                            [p.uv.z, p.uv.w],
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            let at = a + vertex * stride;
                            assert_eq!(
                                [f(at + 4), f(at + 5)].map(f32::to_bits),
                                uv.map(f32::to_bits),
                                "profile{case} packet{packet} vertex{vertex} UV"
                            );
                            if shader == 7 {
                                assert_eq!(
                                    [f(at + 6), f(at + 7)].map(f32::to_bits),
                                    uv.map(f32::to_bits),
                                    "profile{case} packet{packet} vertex{vertex} second UV"
                                );
                            }
                        }
                    }
                    if let art::Kind::Window { selected } = p.kind {
                        assert_eq!(w[4], "window_focus");
                        assert_eq!(
                            [f(24), f(25), f(26), f(27)],
                            [
                                f32::from(selected),
                                f32::from(selected),
                                f32::from(selected),
                                1.
                            ]
                        );
                        assert_eq!([f(28), f(29), f(30), f(31)], [1., 1., 1., 0.]);
                    }
                    if let art::Kind::Button { flash } = p.kind {
                        if flash > 0. {
                            assert_eq!([f(24), f(25), f(26), f(27)], [0.; 4]);
                            assert_eq!([f(28), f(29), f(30), f(31)], [1., 1., 1., flash]);
                        }
                    }
                    assert_eq!(
                        blend,
                        match p.texture {
                            "top" => 3,
                            "nodata" => 1,
                            _ => 2,
                        },
                        "profile{case} packet{packet} blend"
                    );
                    packet += 1;
                    quads += 1;
                }
            }
            _ => panic!("unexpected native packet"),
        }
    }
    assert_eq!(case + 1, expected.len());
    println!(
        "GER_LOAD_NATIVE_RENDER profiles={} quads={quads} exact_uv_color_program=true max_canvas_absolute_difference={max_xy}",
        expected.len()
    );
}
