//! Actual native constructor/child/font/scrollbar packets are the independent
//! geometry oracle. Parent composition has separate native validation.
use super::*;
use crate::asset::font::{GpuFontGlyphHandle, GpuFontLazy};
use crate::asset::system::{Asset, AssetDataAccessor, AssetLoadContext};
use crate::layer::{DrawableLayer, Layer};
use shin_core::vm::command::types::LayerProperty;
use shin_render::shaders::types::{
    buffer::VertexSource,
    texture::TextureTargetKind,
    vertices::{PosColTexVertex, PosColVertex, TextVertex},
};
use shin_render::{
    DepthFunction, DepthState, DepthStencilState, DrawPrimitive, RenderProgramWithArguments,
};
use std::{
    collections::HashMap,
    io::Write,
    process::{Command, Stdio},
};

struct FontBytes(Vec<u8>);
impl Asset for FontBytes {
    type Args = ();
    async fn load(
        _context: &Arc<AssetLoadContext>,
        _args: (),
        _name: &str,
        data: AssetDataAccessor,
    ) -> anyhow::Result<Self> {
        Ok(Self(data.read_all().await?))
    }
}
struct Case {
    profile: usize,
    chapter: bool,
    index: usize,
    feedback: usize,
    list_scroll: f32,
    text_scroll: f32,
    height: f32,
    focus: [f32; 23],
    flash: [f32; 23],
    heading: [f32; 6],
    back: f32,
    values: [f32; 90],
    parent: TransformParams,
}

fn capture(
    binary: &std::path::Path,
    executable: &std::path::Path,
    fonts: &[std::path::PathBuf],
    input: String,
) -> String {
    let mut child = Command::new(binary)
        .arg(executable)
        .args(fonts)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()).unwrap());
    let result = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

fn native_frame(
    layer: &MurdererAccusationView,
    pass: &mut RenderPass,
    native: &str,
    glyphs: &HashMap<(usize, u32), GpuFontGlyphHandle>,
    case: &Case,
) {
    let inner = layer.inner_ref();
    let unpack = |word: u32| {
        UnormColor::from_rgba(
            (word >> 16) as u8,
            (word >> 8) as u8,
            word as u8,
            (word >> 24) as u8,
        )
    };
    let mut last_pass = None;
    for line in native.lines() {
        let f: Vec<_> = line.split_whitespace().collect();
        if f.first() == Some(&"C") {
            assert_eq!(f.len(), 4);
            let flags = f[1].parse::<u32>().unwrap();
            pass.clear(
                (flags & 1 != 0).then(|| unpack(f[2].parse().unwrap())),
                (flags & 4 != 0).then_some(0),
                (flags & 2 != 0).then(|| f32::from_bits(f[3].parse().unwrap())),
            );
            continue;
        }
        if f.first() != Some(&"Q") {
            continue;
        }
        let int = |i: usize| f[i].parse::<u32>().unwrap();
        let scalar = |i: usize| f32::from_bits(int(i));
        let vector = |i: usize| vec4(scalar(i), scalar(i + 1), scalar(i + 2), scalar(i + 3));
        let count = int(7) as usize;
        let program = int(2);
        assert_eq!(
            f.len(),
            51 + count * if program == 3 || program == 4 { 5 } else { 6 }
        );
        if last_pass != Some(int(4)) {
            pass.clear(None, None, Some(1.0));
            last_pass = Some(int(4));
        }
        let mut matrix: [f32; 16] = std::array::from_fn(|i| scalar(11 + i));
        for column in 0..4 {
            matrix[4 * column + 2] = matrix[4 * column + 2] * 0.5 + matrix[4 * column + 3] * 0.5;
        }
        let matrix = Mat4::from_cols_array(&matrix);
        let builder = RenderRequestBuilder::new()
            .depth_stencil(DepthStencilState {
                depth: DepthState {
                    function: match int(5) {
                        1 => DepthFunction::Less,
                        7 => DepthFunction::Always,
                        v => panic!("depth{v}"),
                    },
                    write_enable: int(6) != 0,
                },
                ..DepthStencilState::shorthand(1, true, false)
            })
            .color_blend_type(match int(3) {
                0 => ColorBlendType::NoColor,
                1 => ColorBlendType::Opaque,
                2 => ColorBlendType::Layer1,
                v => panic!("blend{v}"),
            });
        if program == 3 || program == 4 {
            let vertices: Vec<_> = (0..count)
                .map(|i| TextVertex {
                    position: vector(51 + 5 * i),
                    color: scalar(55 + 5 * i),
                })
                .collect();
            let glyph = glyphs
                .get(&(int(9) as usize, int(10)))
                .unwrap()
                .as_texture_source()
                .expect("Reference glyph upload must succeed");
            let request = if program == 3 {
                RenderProgramWithArguments::Font {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    glyph,
                    transform: matrix,
                    color1: FloatColor4::from_vec4(vector(27)),
                    color2: FloatColor4::from_vec4(vector(31)),
                }
            } else {
                RenderProgramWithArguments::FontBorder {
                    vertices: VertexSource::VertexData {
                        vertices: &vertices,
                    },
                    glyph,
                    transform: matrix,
                    color: FloatColor4::from_vec4(vector(31)),
                    distances: std::array::from_fn(|i| {
                        Vec2::new(scalar(35 + 2 * i), scalar(36 + 2 * i))
                    }),
                }
            };
            pass.run(builder.build(request, DrawPrimitive::TrianglesStrip));
            continue;
        }
        let vertices: Vec<_> = (0..count)
            .map(|i| {
                let at = 51 + 6 * i;
                PosColTexVertex {
                    position: glam::vec3(scalar(at), scalar(at + 1), scalar(at + 2)),
                    color: unpack(int(at + 3)),
                    texture_position: Vec2::new(scalar(at + 4), scalar(at + 5)),
                }
            })
            .collect();
        if program == 1 {
            let fill: Vec<_> = vertices
                .iter()
                .map(|v| PosColVertex {
                    position: v.position,
                    color: v.color,
                })
                .collect();
            pass.run(builder.build(
                RenderProgramWithArguments::Fill {
                    vertices: VertexSource::VertexData { vertices: &fill },
                    transform: matrix,
                },
                DrawPrimitive::TrianglesStrip,
            ));
            continue;
        }
        let texture = match f[1] {
            "different_space_2d" => &inner.textures._different_space_2d,
            "left" => &inner.textures.left,
            "right2" => &inner.textures._right2,
            "chap_menu" => &inner.textures._chap_menu,
            "chap_text" => &inner.textures._chap_text,
            "chapter" => &inner.textures._chapter,
            "hana" => &inner.textures._hana,
            "scroll" => inner.adv_textures.get("scroll"),
            "key" => &inner.textures._key,
            "help_chars" => &inner.textures._help_chars,
            "back" => &inner.textures._back,
            "non" => &inner.textures.non,
            "line" => &inner.textures.line,
            "silhouette" => inner.silhouettes[if matches!(case.index, 3 | 4 | 6 | 7 | 9 | 12) {
                0
            } else {
                case.index
            }]
            .as_ref()
            .unwrap()
            .first_texture(),
            name => {
                let names = [
                    "kla0", "nat0", "jes0", "eva0", "hid0", "geo0", "rud0", "kir0", "but0", "ros0",
                    "mar1", "nan0", "gen0", "sha0", "kan0", "goh0", "kum0",
                ];
                inner.textures.suspects()[names
                    .iter()
                    .position(|candidate| *candidate == name)
                    .unwrap()]
            }
        };
        let request = match program {
            2 => RenderProgramWithArguments::Sprite {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                sprite: texture.as_source(),
                transform: matrix,
            },
            5 => RenderProgramWithArguments::Button {
                vertices: VertexSource::VertexData {
                    vertices: &vertices,
                },
                texture: texture.as_source(),
                transform: matrix,
                flash1: vector(27),
                flash2: vector(31),
            },
            v => panic!("program{v}"),
        };
        pass.run(builder.build(request, DrawPrimitive::TrianglesStrip));
    }
}

pub(crate) fn reference_purple_draw(context: &mut PreRenderContext, server: &Arc<AssetServer>) {
    use num_traits::FromPrimitive;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
    let executable = root.join("umineko_switch_files").join(if updated {
        "uncompressed_executable_update"
    } else {
        "uncompressed_executable"
    });
    let scratch =
        std::env::temp_dir().join(format!("shin-purple-reference-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let fonts: Vec<_> = ["medium", "bold"]
        .into_iter()
        .map(|weight| {
            let name = format!("newrodin-{weight}.fnt");
            let bytes =
                shin_tasks::block_on(server.load::<FontBytes, _>(&format!("/{name}"))).unwrap();
            let path = scratch.join(name);
            std::fs::write(&path, &bytes.0).unwrap();
            path
        })
        .collect();
    let binaries: Vec<_> = ["native-purple-font-draw", "native-murderer-purple-draw"]
        .into_iter()
        .map(|name| {
            let path = scratch.join(name);
            let result = Command::new("/usr/bin/clang")
                .args(["-O0", "-Wall", "-Wextra"])
                .arg(root.join("reverse-engineering").join(format!("{name}.c")))
                .arg("-lm")
                .arg("-o")
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                result.status.success() && result.stderr.is_empty(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            path
        })
        .collect();
    let texts: Vec<_> = crate::layer::user::purple_text::CHAPTERS
        .iter()
        .map(|entry| entry.text)
        .chain(crate::layer::user::purple_text::PERSON_TEXT)
        .collect();
    let font_input: String = texts
        .iter()
        .map(|text| {
            if text.is_empty() {
                "-\n".to_string()
            } else {
                format!(
                    "{}\n",
                    text.as_bytes()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                )
            }
        })
        .collect();
    let font_packets = capture(&binaries[0], &executable, &fonts, font_input);
    let heights: Vec<_> = font_packets
        .lines()
        .filter_map(|line| {
            let f: Vec<_> = line.split_whitespace().collect();
            (f[0] == "V").then(|| f32::from_bits(f[3].parse().unwrap()))
        })
        .collect();
    assert_eq!(heights.len(), 46);
    let prepared = shin_tasks::block_on(MurdererAccusationView::prepare(server, 20));
    let mut request = String::new();
    let mut cases = Vec::new();
    for profile in 0..6 {
        let mut values = std::array::from_fn::<_, 90, _>(|i| {
            LayerProperty::from_usize(i).unwrap().initial_value() as f32
        });
        let parent = match profile {
            0 => TransformParams::default(),
            1 => {
                values[0] = 147.25;
                values[1] = -81.5;
                values[12] = 1230.;
                values[13] = 870.;
                values[18] = 125.;
                TransformParams {
                    transform: Mat4::from_translation(glam::vec3(-93., 57., 0.)),
                    camera_position: glam::vec3(30., -10., 125.),
                    wobble_translation: Vec2::new(13., -7.),
                    unconditionally_inherited_translation: Vec2::new(19., 23.),
                }
            }
            2 => {
                values[0] = -234.5;
                values[1] = 98.;
                values[2] = 2150.;
                values[14] = 830.;
                values[15] = 1310.;
                values[19] = -90.;
                TransformParams {
                    transform: Mat4::from_rotation_z(0.23),
                    camera_position: glam::vec3(-64., 82., 350.),
                    wobble_translation: Vec2::new(-27., 43.),
                    unconditionally_inherited_translation: Vec2::new(31., -41.),
                }
            }
            3 => {
                values[0] = 23.75;
                values[1] = -42.125;
                values[10] = 133.5;
                values[11] = -75.25;
                values[12] = 750.;
                values[13] = 1200.;
                values[14] = 1100.;
                values[15] = 900.;
                TransformParams::default()
            }
            4 => {
                values[26] = 3.;
                values[18] = 25.;
                values[19] = -11.;
                values[20] = 17.5;
                values[21] = -9.25;
                TransformParams::default()
            }
            _ => {
                values[0] = 0.125;
                values[1] = 0.375;
                values[12] = 997.;
                values[13] = 1003.;
                TransformParams::default()
            }
        };
        for chapter in [true, false] {
            for index in 0..if chapter { 29 } else { 17 } {
                if chapter
                    && crate::layer::user::purple_text::CHAPTERS[index]
                        .heading()
                        .is_some()
                {
                    continue;
                }
                let height = heights[if chapter { index } else { 29 + index }];
                for feedback in 0..3 {
                    let list_scroll: f32 = if chapter {
                        [0., 495.125, 1890.][feedback]
                    } else {
                        0.
                    };
                    let max_scroll = (height - 720.).max(0.);
                    let text_scroll = [0., 0.125f32.min(max_scroll), max_scroll][feedback];
                    let focus: [f32; 23] = std::array::from_fn(|row| match feedback {
                        0 => 0.,
                        1 => 0.125 + (row % 4) as f32 * 0.125,
                        _ => {
                            if row % 3 == 0 {
                                0.
                            } else {
                                1.
                            }
                        }
                    });
                    let flash: [f32; 23] = std::array::from_fn(|row| match feedback {
                        0 => 0.,
                        1 => {
                            if row % 2 != 0 {
                                0.2
                            } else {
                                0.
                            }
                        }
                        _ => 0.7,
                    });
                    let heading: [f32; 6] = [[0.; 6], [0.4; 6], [0.7; 6]][feedback];
                    let back: f32 = [0., 0.7, 1.][feedback];
                    request += &format!(
                        "{} {index} {:x} {:x} {:x} ",
                        u32::from(chapter),
                        list_scroll.to_bits(),
                        text_scroll.to_bits(),
                        back.to_bits()
                    );
                    for row in 0..if chapter { 23 } else { 17 } {
                        request +=
                            &format!("{:x} {:x} ", focus[row].to_bits(), flash[row].to_bits());
                    }
                    for value in heading {
                        request += &format!("{:x} ", value.to_bits());
                    }
                    let layer = prepared.construct(false);
                    let mut layer = layer;
                    layer.properties_mut().reference_install_native_state(
                        &values,
                        &[(17, 0, 0.); 6],
                        None,
                    );
                    let composed = layer.properties().get_composed_transform_params(&parent);
                    let mut matrix = composed.compute_top_left_transform().to_cols_array();
                    for column in 0..4 {
                        matrix[4 * column + 2] =
                            matrix[4 * column + 2] * 2. - matrix[4 * column + 3];
                    }
                    for value in matrix {
                        request += &format!("{:x} ", value.to_bits());
                    }
                    request += "\n";
                    cases.push(Case {
                        profile,
                        chapter,
                        index,
                        feedback,
                        list_scroll,
                        text_scroll,
                        height,
                        focus,
                        flash,
                        heading,
                        back,
                        values,
                        parent,
                    });
                }
            }
        }
    }
    let native = capture(&binaries[1], &executable, &fonts, request);
    std::fs::write(scratch.join("native-packets.txt"), &native).unwrap();
    let frames: Vec<_> = native.split("V ").skip(1).collect();
    assert_eq!(frames.len(), cases.len());
    let font_assets: Vec<_> = ["medium", "bold"]
        .into_iter()
        .map(|weight| {
            shin_tasks::block_on(server.load::<GpuFontLazy, _>(&format!("/newrodin-{weight}.fnt")))
                .unwrap()
        })
        .collect();
    let mut glyphs = HashMap::new();
    for line in native.lines() {
        let f: Vec<_> = line.split_whitespace().collect();
        if f[0] == "Q" && (f[2] == "3" || f[2] == "4") {
            let font = f[9].parse::<usize>().unwrap();
            let codepoint = f[10].parse::<u32>().unwrap();
            glyphs.entry((font, codepoint)).or_insert_with(|| {
                font_assets[font]
                    .clone()
                    .load_glyphs(
                        context.device.clone(),
                        context.queue.clone(),
                        &[char::from_u32(codepoint).unwrap()],
                    )
                    .remove(0)
            });
        }
    }
    let target = context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Native Purple framebuffer comparison"),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mut packets = 0;
    for (case, native) in cases.iter().zip(frames) {
        let mut layer = prepared.construct(false);
        layer.properties_mut().reference_install_native_state(
            &case.values,
            &[(17, 0, 0.); 6],
            None,
        );
        {
            let mut model = layer.inner_ref().model.lock().unwrap();
            model.reference_set_purple_drawing(
                case.chapter,
                case.index,
                case.list_scroll,
                case.text_scroll,
                case.focus,
                case.flash,
                case.heading,
                case.back,
            );
        }
        assert_eq!(
            layer
                .inner_ref()
                .purple_text_height(&layer.inner_ref().model.lock().unwrap())
                .to_bits(),
            case.height.to_bits(),
            "Native transcript height"
        );
        layer.pre_render(context, &case.parent);
        let draw = |context: &mut PreRenderContext, captured: bool| {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Purple native draw",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
            if captured {
                native_frame(&layer, &mut pass, native, &glyphs, case);
            } else {
                layer.render(&mut pass, &case.parent, 4, PassKind::Opaque);
            }
            drop(pass);
            crate::layer::user::TileLayer::readback_reference_texture(context, &target)
        };
        let actual = draw(context, false);
        let expected = draw(context, true);
        // Native comparisons share the GPU clear implementation. Independently
        // remove all text packets and prove glyphs change only the viewport.
        // George/Battler overflow vertically; check top, fractional, and end.
        if case.profile == 0 && !case.chapter && [5, 8].contains(&case.index) {
            assert!(
                case.height > 720.,
                "This case must overflow the text window"
            );
            let without_text = native
                .lines()
                .filter(|line| {
                    let f: Vec<_> = line.split_whitespace().collect();
                    !(f.first() == Some(&"Q") && ["3", "4"].contains(&f[2]))
                })
                .collect::<Vec<_>>()
                .join("\n");
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: &view,
                    kind: TextureTargetKind::RenderTexture,
                },
                Some(context.depth_stencil),
                "Transcript visibility guard",
            );
            pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.));
            native_frame(&layer, &mut pass, &without_text, &glyphs, case);
            drop(pass);
            let baseline = crate::layer::user::TileLayer::readback_reference_texture(context, &target);
            let mut changed_inside = 0;
            for (pixel, (actual, baseline)) in actual
                .chunks_exact(4)
                .zip(baseline.chunks_exact(4))
                .enumerate()
            {
                if actual == baseline {
                    continue;
                }
                let (x, y) = (pixel % 1920, pixel / 1920);
                assert!(
                    (706..1774).contains(&x) && (180..900).contains(&y),
                    "Character{} scroll{} glyph escaped viewport at ({x},{y})",
                    case.index,
                    case.text_scroll
                );
                changed_inside += 1;
            }
            assert!(
                changed_inside > 1000,
                "Clipping must retain visible transcript text"
            );
            if case.feedback == 0 {
                layer.scroll_text(180.);
                assert_eq!(
                    layer.inner_ref().model.lock().unwrap().purple_text_scroll,
                    180.0f32.min(case.height - 720.)
                );
                layer.scroll_text(10000.);
                assert_eq!(
                    layer.inner_ref().model.lock().unwrap().purple_text_scroll,
                    case.height - 720.
                );
                layer.scroll_text(-10000.);
                assert_eq!(
                    layer.inner_ref().model.lock().unwrap().purple_text_scroll,
                    0.
                );
            }
            eprintln!(
                "Character{} scroll{}: {changed_inside} text pixels inside viewport, zero outside; desktop scroll bounds checked",
                case.index, case.text_scroll
            );
        }
        let count = native.lines().filter(|line| line.starts_with("Q ")).count();
        packets += count;
        let max = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| (*a as i16 - *b as i16).unsigned_abs())
            .max()
            .unwrap();
        let different = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
        eprintln!(
            "Purple profile{} chapter{} index{} feedback{} list{} text{} height{} packets{count} different{different} maxRGBAerror{max}",
            case.profile,
            case.chapter,
            case.index,
            case.feedback,
            case.list_scroll,
            case.text_scroll,
            case.height
        );
        if max != 0 {
            std::fs::write(scratch.join("actual.rgba"), &actual).unwrap();
            std::fs::write(scratch.join("expected.rgba"), &expected).unwrap();
        }
        assert_eq!(
            max,
            0,
            "Native Purple full-frame mismatch; artifacts at{}",
            scratch.display()
        );
    }
    println!(
        "Purple: {} strict1920x1080 native full-frame comparisons, {packets} packets, {} byte-identical pixels; six parent/property transforms, all17persons/23chapters, three feedback/scroll profiles, actual ROM glyphs/portraits/depth masks/outlines/scrollbars. Native NVN/cache scheduling remains unproven.",
        cases.len(),
        cases.len() * 1920 * 1080
    );
    std::fs::remove_dir_all(scratch).unwrap();
}
