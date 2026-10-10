//! Reuse the actual native Hints and parent/frozen-capture bodies across frames.
use super::*;
use crate::layer::{DrawableLayer, Layer};
use shin_core::vm::command::types::LayerProperty;
use shin_render::shaders::types::texture::TextureTargetKind;
use std::{
    io::Write,
    process::{Command, Stdio},
};

struct Case {
    values: [f32; 90],
    parent: TransformParams,
    progress: usize,
    scroll: f32,
    focus: [f32; 21],
    flash: [f32; 21],
    back: f32,
}
impl Case {
    fn install(&self, layer: &mut MurdererAccusationView) {
        layer.properties_mut().reference_install_native_state(
            &self.values,
            &[(17, 0, 0.); 6],
            None,
        );
        layer
            .inner_ref()
            .model
            .lock()
            .unwrap()
            .reference_set_hints_drawing(
                self.progress as i32,
                self.scroll,
                self.focus,
                self.flash,
                self.back,
            );
    }
    fn request(&self) -> String {
        let mut s = format!(
            "{} {:x} {:x} ",
            self.progress,
            self.scroll.to_bits(),
            self.back.to_bits()
        );
        for row in 0..=self.progress {
            s += &format!(
                "{:x} {:x} ",
                self.focus[row].to_bits(),
                self.flash[row].to_bits()
            );
        }
        for value in self.values {
            s += &format!("{:x} ", value.to_bits());
        }
        s += &"11 0 0 ".repeat(6);
        s += "0 0 ";
        for value in self
            .parent
            .transform
            .to_cols_array()
            .into_iter()
            .chain(self.parent.camera_position.to_array())
            .chain(self.parent.unconditionally_inherited_translation.to_array())
            .chain(self.parent.wobble_translation.to_array())
        {
            s += &format!("{:x} ", value.to_bits());
        }
        s.push('\n');
        s
    }
}
fn make_case(index: usize) -> Case {
    use num_traits::FromPrimitive;
    let mut values =
        std::array::from_fn(|i| LayerProperty::from_usize(i).unwrap().initial_value() as f32);
    let parent = match index % 6 {
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
            values[12] = 750.;
            values[13] = 1200.;
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
    let progress = [20, 0, 8, 12][index % 4];
    let max = ((progress + 1) as f32 * 90. - 720.).max(0.);
    let scroll = if index % 3 == 0 { max } else { max.min(0.125) };
    Case {
        values,
        parent,
        progress,
        scroll,
        focus: std::array::from_fn(|row| {
            if index % 2 == 0 {
                [0.125, 0.7, 1.][row % 3]
            } else {
                0.
            }
        }),
        flash: std::array::from_fn(|row| {
            if index % 3 == 0 {
                [0., 0.333, 0.999][row % 3]
            } else {
                0.
            }
        }),
        back: if index % 2 == 0 { 0.7 } else { 0. },
    }
}
fn native(
    binary: &std::path::Path,
    executable: &std::path::Path,
    request: String,
    frozen: bool,
) -> String {
    let mut cmd = Command::new(binary);
    cmd.arg(executable);
    if frozen {
        cmd.arg("--freeze");
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(request.as_bytes()).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn target(context: &PreRenderContext, label: &str) -> wgpu::Texture {
    context.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: 1920,
            height: 1080,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: shin_render::TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
fn expected(
    context: &mut PreRenderContext,
    texture: &wgpu::Texture,
    layer: &MurdererAccusationView,
    packets: &str,
    initialize: bool,
) -> Vec<u8> {
    let view = texture.create_view(&Default::default());
    let mut pass = context.begin_pass(
        TextureTarget {
            view: &view,
            kind: TextureTargetKind::RenderTexture,
        },
        Some(context.depth_stencil),
        "Native reused capture",
    );
    if initialize {
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
    }
    super::hints_draw_tests::draw_native_hints(layer, &mut pass, packets);
    drop(pass);
    super::super::TileLayer::readback_reference_texture(context, texture)
}
fn compare(actual: &[u8], expected: &[u8], label: &str) {
    assert_eq!(actual.len(), expected.len());
    let differing = actual.iter().zip(expected).filter(|(a, b)| a != b).count();
    let worst = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| (*a as i16 - *b as i16).unsigned_abs())
        .max()
        .unwrap();
    eprintln!("Capture {label}: differing_bytes{differing}, maxRGBAerror{worst}");
    assert_eq!(
        differing, 0,
        "{label}: native persistent capture mismatch, maxRGBAerror{worst}"
    );
}

pub(crate) fn reference_capture_lifecycle(
    context: &mut PreRenderContext,
    server: &Arc<AssetServer>,
) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let updated = std::env::var_os("SHIN_REFERENCE_PATCH_ROM").is_some();
    let executable = root.join("umineko_switch_files").join(if updated {
        "uncompressed_executable_update"
    } else {
        "uncompressed_executable"
    });
    let binary = std::env::temp_dir().join(format!("shin-murderer-capture-{}", std::process::id()));
    let output = Command::new("/usr/bin/clang")
        .args(["-O0", "-Wall", "-Wextra"])
        .arg(root.join("reverse-engineering/native-murderer-hints-draw.c"))
        .arg("-lm")
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cases: Vec<_> = (0..16).map(make_case).collect();
    let normal = native(
        &binary,
        &executable,
        cases.iter().map(Case::request).collect(),
        false,
    );
    let frames: Vec<_> = normal.split("V ").skip(1).collect();
    assert_eq!(frames.len(), 16);
    // Native8cb60 captures the old child with current properties and its own
    // canonical parent, ignoring the caller's parent supplied for live capture.
    let mut freeze = make_case(0);
    freeze.values = cases[1].values;
    freeze.parent = cases[1].parent;
    let frozen = native(&binary, &executable, freeze.request(), true);
    let frozen_frame = frozen.split("V ").nth(1).unwrap();
    let version = if updated { "1.03" } else { "original" };
    std::fs::write(
        std::env::temp_dir().join(format!("shin-murderer-capture-native-{version}.txt")),
        format!("{normal}FROZEN\n{frozen}"),
    )
    .unwrap();
    let mut checked = 0;
    let mut frozen_checks = 0;
    for kind in [
        None,
        Some(MurdererWipeKind::Default),
        Some(MurdererWipeKind::Glass),
    ] {
        let mut layer = shin_tasks::block_on(MurdererAccusationView::load_menu(server, 20));
        let live_expected = target(context, "Native persistent live expected");
        let frozen_expected = target(context, "Native canonical frozen expected");
        let mut alias = None;
        let mut frozen_pixels = None;
        for (i, (case, packets)) in cases.iter().zip(&frames).enumerate() {
            let active = alias.as_mut().unwrap_or(&mut layer);
            case.install(active);
            if i == 1 {
                if let Some(kind) = kind {
                    let mut model = active.inner_ref().model.lock().unwrap();
                    let mut source = model.clone();
                    // Freeze old rows while current properties/parent changed.
                    source.reference_set_hints_drawing(
                        cases[0].progress as i32,
                        cases[0].scroll,
                        cases[0].focus,
                        cases[0].flash,
                        cases[0].back,
                    );
                    model.reference_capture_source(source, kind);
                }
            }
            if i == 15 && kind.is_some() {
                use crate::update::AdvUpdatable;
                use shin_core::primitives::update::FrameId;
                active.fast_forward();
                let mut frame = FrameId::default();
                frame.advance();
                active.update(&AdvUpdateContext {
                    frame_id: frame,
                    delta_ticks: Ticks::ZERO,
                    asset_server: server,
                    device: context.device,
                    queue: context.queue,
                    are_animations_allowed: false,
                    voice_callback: None,
                    movie_volume_settings: Default::default(),
                });
                case.install(active);
                assert!(active.inner_ref().model.lock().unwrap().wipe.is_none());
                let renderer = active.inner_ref().transition.lock();
                assert!(renderer.source.get().is_none() && renderer.live.get().is_some());
            }
            active.pre_render(context, &case.parent);
            let pixels = expected(context, &live_expected, active, packets, i == 0);
            let actual = if kind.is_some() && i > 0 && i < 15 {
                capture_pixels(context, &active.inner_ref().transition.lock().live)
            } else {
                let holder = &active.inner_ref().transition.lock().live;
                if holder.get().is_some() {
                    capture_pixels(context, holder)
                } else {
                    let capture = active
                        .reference_with_capture(|t| t.render_clone(&mut context.render_clone_ctx()))
                        .unwrap();
                    let mut holder = RenderTextureHolder::new("Actual normal capture");
                    *holder.as_inner_mut() = Some(capture);
                    capture_pixels(context, &holder)
                }
            };
            compare(&actual, &pixels, &format!("{kind:?} frame{i}"));
            checked += 1;
            if kind.is_some() && i > 0 && i < 15 {
                if i == 1 {
                    frozen_pixels = Some(expected(
                        context,
                        &frozen_expected,
                        active,
                        frozen_frame,
                        true,
                    ));
                }
                compare(
                    &active.reference_frozen_pixels(context),
                    frozen_pixels.as_ref().unwrap(),
                    &format!("{kind:?} frozen{i}"),
                );
                frozen_checks += 1;
            }
            if i == 2 && kind.is_some() {
                alias = Some(layer.render_clone(&mut context.render_clone_ctx()));
            }
        }
    }
    // Full-image guards cover both complete parent and frozen draw entrypoints.
    for at in [0x9b250usize, 0x8cb60] {
        let mut bytes = std::fs::read(&executable).unwrap();
        bytes[256 + at - if updated { 0xa0 } else { 0 }] ^= 1;
        let mutant = binary.with_extension("mutant");
        std::fs::write(&mutant, bytes).unwrap();
        assert_eq!(
            Command::new(&binary)
                .arg(&mutant)
                .arg("--freeze")
                .output()
                .unwrap()
                .status
                .code(),
            Some(2)
        );
        std::fs::remove_file(mutant).unwrap();
    }
    std::fs::remove_file(binary).unwrap();
    println!(
        "Murderer native capture lifecycle:{checked}live frames/{frozen_checks}frozen comparisons/{}pixels, exact RGBA bytes; normal reuse, Default/Glass, aliases, canonical frozen parent, paused cleanup and retained live target;2whole-image mutants rejected",
        (checked + frozen_checks) * 1920 * 1080
    );
}
