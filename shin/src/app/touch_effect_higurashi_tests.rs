// Include only inside touch_effect::tests. Parent runs this ignored hardware test.
// Required: SHIN_HIGURASHI_ASSETS, SHIN_HIGURASHI_TOUCH_EXECUTABLE,
// SHIN_HIGURASHI_TOUCH_NATIVE (unchanged native-touch-effects.c binary), and
// SHIN_HIGURASHI_TOUCH_LITERAL_CAPTURE (a directory that must not exist).
// glslc must be on PATH. No audio, window, progress storage or media is opened.
// Exact equality is deliberately the initial gate. All raw observations and GPU
// frames are retained BEFORE that gate; any measured tolerance needs review.
mod higurashi_literal_ripple {
    use super::*;
    use crate::adv::higurashi::{reference_gpu_assets, reference_render_to_texture};
    use crate::render::PreRenderContext;
    use shin_core::primitives::color::UnormColor;
    use shin_render::{
        gpu_texture::GpuTexture,
        quad_vertices::QuadVertices,
        shaders::types::texture::{TextureSource, TextureTarget, TextureTargetKind},
    };
    use std::{
        borrow::Cow,
        fs,
        io::Write,
        path::{Path, PathBuf},
        process::{Command, Stdio},
    };
    use wgpu::util::DeviceExt;

    const VERTEX: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reverse-engineering/higurashi/touch-effects-port/higurashi-program19.vertex.glsl"
    ));
    const FRAGMENT: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reverse-engineering/higurashi/touch-effects-port/higurashi-program19.fragment.glsl"
    ));
    const INPUT: &str = "0 0 1 2 960 540 960 540 0\n\
                        1 3.75 1 0 960 540 960 540 1\n\
                        2 3.75 1 0 960 540 960 540 1\n\
                        3 7.5 1 0 960 540 960 540 1\n\
                        4 7.5 1 0 960 540 960 540 1\n";
    const AGES: [f32; 4] = [0.125, 0.25, 0.5, 0.75];

    fn save(path: impl AsRef<Path>, data: &[u8]) {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
            .write_all(data)
            .unwrap();
    }

    fn fnv(data: &[u8]) -> u64 {
        data.iter().fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
    }

    fn native_packets(directory: &Path) -> Vec<Vec<u32>> {
        let executable = PathBuf::from(std::env::var("SHIN_HIGURASHI_TOUCH_EXECUTABLE").unwrap());
        let bytes = fs::read(&executable).unwrap();
        assert_eq!(bytes.len(), 1_892_616);
        let raw = &bytes[256..];
        assert_eq!(fnv(raw), 0xe5ac08a484b4865d, "exact updated primary");
        for (record, begin, end, source) in [
            (0x11f358, 0x1c4ebe, 0x1c50ff, VERTEX),
            (0x11f3a0, 0x1c49a9, 0x1c4ebe, FRAGMENT),
        ] {
            let word = |offset| u64::from_le_bytes(raw[offset..offset + 8].try_into().unwrap());
            assert_eq!(word(record), begin as u64);
            assert_eq!(word(record + 16), 0x403);
            assert_eq!(&raw[begin..end], source.as_bytes());
        }
        save(directory.join("native-input.txt"), INPUT.as_bytes());
        let mut observations = Vec::new();
        for repeat in 0..2 {
            let mut child = Command::new(std::env::var("SHIN_HIGURASHI_TOUCH_NATIVE").unwrap())
                .arg(&executable)
                .arg("5489")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(INPUT.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            save(
                directory.join(format!("native-{repeat}.stdout")),
                &output.stdout,
            );
            save(
                directory.join(format!("native-{repeat}.stderr")),
                &output.stderr,
            );
            save(
                directory.join(format!("native-{repeat}.status")),
                output.status.to_string().as_bytes(),
            );
            observations.push(output);
        }
        for output in &observations {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stderr).contains("outstanding=0"));
        }
        assert_eq!(
            observations[0].stdout, observations[1].stdout,
            "literal original repeats"
        );
        assert_eq!(observations[0].stderr, observations[1].stderr);
        let text = std::str::from_utf8(&observations[0].stdout).unwrap();
        assert_eq!(
            text.lines().filter(|line| line.starts_with("F ")).count(),
            5
        );
        assert!(!text.lines().any(|line| line.starts_with("S ")));
        let packets: Vec<Vec<u32>> = text
            .lines()
            .filter_map(|line| line.strip_prefix("Q "))
            .map(|line| {
                line.split_whitespace()
                    .map(|word| word.parse().unwrap())
                    .collect()
            })
            .collect();
        assert_eq!(packets.len(), AGES.len());
        for (packet, age) in packets.iter().zip(AGES) {
            assert_eq!(packet.len(), 44);
            // Frozen adapter's semantic kind1 is native Higa program19,blend5.
            assert_eq!(&packet[..4], &[1, 5, 8, 0]);
            assert_eq!(packet[24], age.to_bits()); // slot1 params[1].x
        }
        packets
    }

    fn adapt(stage: &str, original: &str) -> String {
        let mut source = original.replace("\r\n", "\n");
        assert_eq!(source.matches("#version 440").count(), 1);
        source = source.replace("#version 440", "#version 450");
        assert_eq!(source.matches("layout(binding=0)").count(), 1);
        if stage == "vert" {
            source = source.replace("layout(binding=0)", "layout(set=0, binding=0)");
            assert_eq!(source.matches("    return;").count(), 1);
            source = source.replace(
                "    return;",
                "    gl_Position.z = (gl_Position.z + gl_Position.w) * 0.5;\n    return;",
            );
        } else {
            source = source.replace("layout(binding=0)", "layout(set=0, binding=1)");
            let sampler = "layout(location = 0, binding = 0) uniform  sampler2D texture2d_0;";
            assert_eq!(source.matches(sampler).count(), 1);
            source = source.replace(
                sampler,
                "layout(set=0, binding=2) uniform texture2D source_texture;\n\
                 layout(set=0, binding=3) uniform sampler source_sampler;",
            );
            assert_eq!(source.matches("texture(texture2d_0,").count(), 1);
            source = source.replace(
                "texture(texture2d_0,",
                "texture(sampler2D(source_texture, source_sampler),",
            );
        }
        source
    }

    struct Literal {
        layout: wgpu::BindGroupLayout,
        pipeline: wgpu::RenderPipeline,
    }

    impl Literal {
        fn new(device: &wgpu::Device, directory: &Path) -> Self {
            let module = |stage: &str, source: &str| {
                let adapted = directory.join(format!("literal19.{stage}"));
                let output = directory.join(format!("literal19.{stage}.spv"));
                save(&adapted, adapt(stage, source).as_bytes());
                let result = Command::new("glslc")
                    .arg("-fauto-map-locations")
                    .arg(&adapted)
                    .arg("-o")
                    .arg(&output)
                    .output()
                    .unwrap();
                save(
                    directory.join(format!("glslc-{stage}.stdout")),
                    &result.stdout,
                );
                save(
                    directory.join(format!("glslc-{stage}.stderr")),
                    &result.stderr,
                );
                assert!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                let bytes = fs::read(output).unwrap();
                assert_eq!(bytes.len() % 4, 0);
                assert_eq!(&bytes[..4], &0x07230203u32.to_le_bytes());
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Exact Higurashi program19 GLSL arithmetic"),
                    source: wgpu::ShaderSource::SpirV(Cow::Owned(
                        bytes
                            .chunks_exact(4)
                            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                            .collect(),
                    )),
                })
            };
            let vertex = module("vert", VERTEX);
            let fragment = module("frag", FRAGMENT);
            let uniform = |binding, visibility| wgpu::BindGroupLayoutEntry {
                binding,
                visibility,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            };
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Original ripple descriptors"),
                entries: &[
                    uniform(0, wgpu::ShaderStages::VERTEX),
                    uniform(1, wgpu::ShaderStages::FRAGMENT),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Literal Higa ripple19/blend5"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &vertex,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 16,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 0,
                            shader_location: 0,
                        }],
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &fragment,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: shin_render::TEXTURE_FORMAT,
                        blend: Some(wgpu::BlendState {
                            // Original blend5, independently spelled out.
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: shin_render::DEPTH_STENCIL_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: wgpu::StencilState::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
            Self { layout, pipeline }
        }

        fn draw(
            &self,
            context: &mut PreRenderContext,
            view: &wgpu::TextureView,
            source: TextureSource,
            packet: &[u32],
        ) {
            let buffer = |words: &[u32], usage| {
                context
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Original ripple uploaded words"),
                        contents: bytemuck::cast_slice(words),
                        usage,
                    })
            };
            let matrix = buffer(&packet[4..20], wgpu::BufferUsages::UNIFORM);
            let params = buffer(&packet[20..28], wgpu::BufferUsages::UNIFORM);
            let vertices = buffer(&packet[28..44], wgpu::BufferUsages::VERTEX);
            let sampler = context.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Native default linear/clamped source sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            });
            let bindings = context
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: matrix.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: params.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(source.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Sampler(&sampler),
                        },
                    ],
                });
            let mut pass = context
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Original Higurashi ripple arithmetic replay"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: context.depth_stencil.view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        }),
                    }),
                    multiview_mask: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.draw(0..4, 0..1);
        }
    }

    fn background(context: &mut PreRenderContext, view: &wgpu::TextureView, source: TextureSource) {
        let mut pass = context.begin_pass(
            TextureTarget {
                view,
                kind: TextureTargetKind::RenderTexture,
            },
            Some(context.depth_stencil),
            "Identical composed background for both ripple implementations",
        );
        pass.clear(Some(UnormColor::BLACK), Some(0), Some(1.0));
        QuadVertices::new()
            .with_box(0.0, 0.0, 1920.0, 1080.0)
            .render_sprite(
                &mut pass,
                RenderRequestBuilder::new().color_blend_type(ColorBlendType::Opaque),
                source,
                crate::render::top_left_projection_matrix(),
            );
    }

    #[test]
    #[ignore = "requires supplied updated original, frozen native adapter, glslc and offscreen Metal; no audio"]
    fn higurashi_touch_ripple_matches_literal_program19_at_both_viewports() {
        let directory =
            PathBuf::from(std::env::var("SHIN_HIGURASHI_TOUCH_LITERAL_CAPTURE").unwrap());
        fs::create_dir(&directory).expect("preserve prior captures: choose a NEW directory");
        let packets = native_packets(&directory);
        let (device, queue, assets) = reference_gpu_assets();
        // Verify that the actual older shared atlas is loadable. Stars have their
        // own real-art check; this comparison intentionally isolates ripple19.
        let _atlas: std::sync::Arc<CharacterTextures> = assets.load_sync("/systex.txa").unwrap();
        let literal = Literal::new(&device, &directory);
        let (width, height) = (257u32, 193u32);
        let mut pattern = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                // Smooth gradients plus sharp asymmetric boundaries and nonopaque
                // alpha exercise distortion, all-RGBA highlight and blend5.
                pattern.extend([
                    ((x * 13 + y * 7 + (x / 17) * 61) % 256) as u8,
                    ((x * 3 + y * 19 + (y / 11) * 47) % 256) as u8,
                    ((x * 11 + y * 5 + ((x + y) / 23) * 73) % 256) as u8,
                    (120 + (x * 7 + y * 3) % 136) as u8,
                ]);
            }
        }
        save(directory.join("source-257x193.rgba"), &pattern);
        let source = GpuTexture::new_static_with_data(
            &device,
            &queue,
            Some("Ripple distortion nonuniform RGBA source"),
            winit::dpi::PhysicalSize::new(width, height),
            shin_render::TEXTURE_FORMAT,
            &pattern,
        );
        let mut observations = Vec::new();
        for size in [(1280, 720), (1920, 1080)] {
            let base = reference_render_to_texture(&device, &queue, size, |context, view| {
                background(context, view, source.as_source());
            });
            save(
                directory.join(format!("{}x{}-background.rgba", size.0, size.1)),
                &base,
            );
            for (packet, age) in packets.iter().zip(AGES) {
                let mut effect = TouchEffect::with_seed(5489);
                effect.ripples.push(TapRipple {
                    position: Vec2::new(960.0, 540.0),
                    age,
                });
                let actual = reference_render_to_texture(&device, &queue, size, |context, view| {
                    background(context, view, source.as_source());
                    let mut pass = context.begin_pass(
                        TextureTarget {
                            view,
                            kind: TextureTargetKind::RenderTexture,
                        },
                        Some(context.depth_stencil),
                        "Shared ripple component",
                    );
                    effect.render_ripples(&mut pass, source.as_source());
                });
                let native = reference_render_to_texture(&device, &queue, size, |context, view| {
                    background(context, view, source.as_source());
                    literal.draw(context, view, source.as_source(), packet);
                });
                let stem = format!("{}x{}-age{:08x}", size.0, size.1, age.to_bits());
                save(directory.join(format!("{stem}-shared.rgba")), &actual);
                save(directory.join(format!("{stem}-literal.rgba")), &native);
                let changed = actual.iter().zip(&base).filter(|(a, b)| a != b).count();
                let changed_native = native.iter().zip(&base).filter(|(a, b)| a != b).count();
                let differences = actual.iter().zip(&native).map(|(a, b)| a.abs_diff(*b));
                let max = differences.clone().max().unwrap();
                let unequal = differences.clone().filter(|&delta| delta != 0).count();
                let above_one = differences.filter(|&delta| delta > 1).count();
                observations.push((stem, max, unequal, above_one, changed, changed_native));
            }
        }
        let mut report = String::from(
            "case max_channel unequal_channels above_one shared_changed literal_changed\n",
        );
        for (stem, max, unequal, above_one, changed, changed_native) in &observations {
            report += &format!("{stem} {max} {unequal} {above_one} {changed} {changed_native}\n");
        }
        // Preserve EVERY case and metric before equality assertions. Do not
        // silently choose a tolerance or stop at the first measured mismatch.
        save(directory.join("metrics.txt"), report.as_bytes());
        eprintln!("{report}");
        assert_eq!(observations.len(), 8);
        for (stem, max, unequal, _, changed, changed_native) in observations {
            assert!(
                changed > 100 && changed_native > 100,
                "{stem}: visible effect control"
            );
            assert_eq!(
                max, 0,
                "{stem}: {unequal} channels differ; raw frames retained for review"
            );
        }
    }
}
