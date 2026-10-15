use super::*;
use shin_core::format::{font::read_font_metrics, text::write_sjis_string};
use std::{
    cell::Cell,
    fmt::Write as _,
    io::{BufReader, Write as _},
    process::{Command as Process, Stdio},
    rc::Rc,
};

struct Resource {
    layout: Layout,
    retired: Rc<Cell<usize>>,
}
impl Drop for Resource {
    fn drop(&mut self) {
        self.retired.set(self.retired.get() + 1);
    }
}
fn floats(out: &mut String, values: impl IntoIterator<Item = f32>) {
    for v in values {
        write!(out, " {}", v.to_bits()).unwrap();
    }
}
fn hex(text: &str) -> String {
    let mut bytes = Vec::new();
    write_sjis_string(text, &mut bytes).unwrap();
    if bytes.is_empty() {
        "-".into()
    } else {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
fn packets(
    out: &mut String,
    resource: &Resource,
    label: &Label,
    entries: &[Entry],
    scroll: f32,
    slide: f32,
    selected: usize,
    first: usize,
    end: usize,
) -> usize {
    let layout = &resource.layout;
    let matrix = layout
        .native_transform(label.index, scroll, slide)
        .to_cols_array();
    let fill = label.fill(selected, entries[label.index].row, first, end);
    for border in [true, false] {
        for g in &layout.glyphs {
            write!(out, " P {} {}", if border { 4 } else { 3 }, g.point as u32).unwrap();
            floats(out, matrix);
            let extra = if border {
                g.border
                    .into_iter()
                    .flat_map(|v| v.to_array())
                    .collect::<Vec<_>>()
            } else {
                fill.to_array().into_iter().chain([0.0; 12]).collect()
            };
            floats(out, extra);
            floats(
                out,
                (if border {
                    vec4(0.0, 0.0, 0.0, 240.0 * f32::from_bits(0x3b808081))
                } else {
                    fill
                })
                .to_array()
                .into_iter()
                .chain([0.0; 12]),
            );
            for v in &g.vertices {
                let position = v.position;
                floats(out, position.to_array());
                floats(out, [v.color]);
            }
        }
    }
    layout.glyphs.len() * 2
}

#[test]
#[ignore = "executes updated7c31c populated label caches and exact SimpleText glyph packets without GPU/audio/saves"]
fn higurashi_navigation_visible_label_cache_matches_native_preparation_and_retirement() {
    let font = Arc::new(
        read_font_metrics(&mut BufReader::new(
            std::fs::File::open(std::env::var("SHIN_HIGURASHI_FONT").unwrap()).unwrap(),
        ))
        .unwrap(),
    );
    let retired = Rc::new(Cell::new(0));
    let mut created = 0;
    let mut frames = 0;
    let mut owners = 0;
    let mut glyph_packets = 0;
    let mut input = String::new();
    let mut expected = String::new();
    for count in [1, 8, 20, 32, 64] {
        for anchors in [false, true] {
            for profile in 0..3 {
                owners += 1;
                let rows: Vec<_> = (0..count)
                    .map(|i| Row {
                        boundary: i as u32,
                        index: i as u32,
                        item: match i % 4 {
                            0 => Item::Message {
                                text: "本文".into(),
                                style: 0,
                                argument: None,
                            },
                            1 => Item::Choice {
                                title: match profile {
                                    0 => "@r選択\n二行 @c900色",
                                    1 => "",
                                    _ => "ASCII && 日本語",
                                }
                                .into(),
                                text: "無視される本文".into(),
                            },
                            2 => Item::Checkpoint {
                                title: "章タイトル".into(),
                                text: if profile == 1 {
                                    ""
                                } else {
                                    "詳細 @r本文\n行"
                                }
                                .into(),
                            },
                            _ => Item::CharacterChoice { response: 3 },
                        },
                    })
                    .collect();
                let marks: Vec<_> = if profile == 2 {
                    (0..count).rev().map(|i| i as u32).collect()
                } else {
                    (0..count)
                        .filter(|i| i % 4 != 0)
                        .map(|i| i as u32)
                        .collect()
                };
                let selected = count / 2;
                let current = if anchors { selected } else { count - 1 };
                let same_anchor = current == selected;
                write!(input, "N {count} {selected} {current} {}\n", marks.len()).unwrap();
                for row in &rows {
                    let (kind, title, text) = match &row.item {
                        Item::Message { .. } => (0, "", ""),
                        Item::Choice { title, text } => (1, title.as_str(), text.as_str()),
                        Item::Checkpoint { title, text } => (2, title.as_str(), text.as_str()),
                        Item::CharacterChoice { .. } => (3, "", ""),
                    };
                    writeln!(input, "{kind} {} {}", hex(title), hex(text)).unwrap();
                }
                for m in &marks {
                    write!(input, "{m} ").unwrap();
                }
                input.push('\n');
                let mut entries = vec![Entry {
                    kind: 0,
                    marker: -1,
                    row: 0,
                }];
                entries.extend(marks.iter().map(|i| Entry {
                    kind: 1,
                    marker: *i as i32,
                    row: *i as i32,
                }));
                entries.push(Entry {
                    kind: if same_anchor { 99 } else { 97 },
                    marker: -1,
                    row: selected as i32,
                });
                if !same_anchor {
                    entries.push(Entry {
                        kind: 98,
                        marker: -1,
                        row: current as i32,
                    });
                }
                entries.push(Entry {
                    kind: 100,
                    marker: -1,
                    row: count as i32 - 1,
                });
                entries.sort_by_key(|e| (e.row, e.kind));
                let max = (entries.len() as f32 * 60.0 - 720.0).max(0.0);
                let mut cache: Cache<Resource> = Cache::default();
                let mut native_glyphs = 0;
                let start_created = created;
                let start_retired = retired.get();
                for (n, scroll) in [
                    0.0,
                    0.25,
                    59.5,
                    60.0,
                    60.25,
                    max,
                    max - 0.25,
                    max * 0.5,
                    0.0,
                    max,
                    0.0,
                ]
                .into_iter()
                .enumerate()
                {
                    // Out-of-bounds document offsets exercise the original
                    // strict visibility checks and off-screen cache eviction.
                    let slide = [0.0, 0.25, 1.0][n % 3];
                    let sel = [0, entries.len() / 2, entries.len() - 1][n % 3];
                    let first = if profile == 1 { count / 3 } else { 0 };
                    let end = if profile == 1 {
                        count * 2 / 3
                    } else {
                        count - 1
                    };
                    writeln!(input, "R {scroll} {slide} {sel} {first} {end}").unwrap();
                    cache
                        .synchronize(&entries, &rows, scroll, |label| {
                            let layout = Layout::new(font.clone(), &label.text)?;
                            native_glyphs += layout.glyphs.len();
                            created += 1;
                            Ok(Resource {
                                layout,
                                retired: retired.clone(),
                            })
                        })
                        .unwrap();
                    expected.push('R');
                    let mut draws = 0;
                    for c in &cache.entries {
                        draws += packets(
                            &mut expected,
                            &c.value,
                            &c.label,
                            &entries,
                            scroll,
                            slide,
                            sel,
                            first,
                            end,
                        );
                    }
                    write!(expected, " C {}", cache.entries.len()).unwrap();
                    for c in &cache.entries {
                        write!(expected, " T {}", c.label.index).unwrap();
                        floats(&mut expected, c.label.tint.to_array());
                        floats(&mut expected, c.value.layout.size.to_array());
                        floats(
                            &mut expected,
                            c.label
                                .fill(sel, entries[c.label.index].row, first, end)
                                .to_array(),
                        );
                    }
                    writeln!(
                        expected,
                        " E {draws} {} {native_glyphs}",
                        cache
                            .entries
                            .iter()
                            .filter(|c| !c.value.layout.glyphs.is_empty())
                            .count()
                    )
                    .unwrap();
                    assert_eq!(
                        created - start_created - (retired.get() - start_retired),
                        cache.entries.len()
                    );
                    frames += 1;
                    glyph_packets += draws;
                }
                input.push_str("D\n");
                drop(cache);
                assert_eq!(created, retired.get());
            }
        }
    }
    let mut child = Process::new(std::env::var("SHIN_HIGURASHI_NATIVE_NAVIGATION_RENDER").unwrap())
        .args([
            std::env::var("SHIN_HIGURASHI_EXE").unwrap(),
            std::env::var("SHIN_HIGURASHI_FONT").unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let send = input.clone();
    let writer = std::thread::spawn(move || stdin.write_all(send.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    if let Ok(directory) = std::env::var("SHIN_HIGURASHI_NAVIGATION_RENDER_CAPTURE") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("input.txt"), &input).unwrap();
        std::fs::write(directory.join("native.txt"), &output.stdout).unwrap();
        std::fs::write(directory.join("native.log"), &output.stderr).unwrap();
        std::fs::write(directory.join("rust.txt"), &expected).unwrap();
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).unwrap();
    for (frame, (a, b)) in actual.lines().zip(expected.lines()).enumerate() {
        let a: Vec<_> = a.split_whitespace().collect();
        let b: Vec<_> = b.split_whitespace().collect();
        assert_eq!(a.len(), b.len(), "frame {frame} word count");
        for (word, (a, b)) in a.iter().zip(&b).enumerate() {
            assert_eq!(a, b, "frame {frame},word {word}");
        }
    }
    assert_eq!(actual.lines().count(), frames);
    eprintln!(
        "Navigation labels:{owners} owners,{frames} frames,{glyph_packets} glyph packets,{created} resources created/retired; {} exact words",
        expected.split_whitespace().count()
    );
}
