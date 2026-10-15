use super::*;
use crate::render::PreRenderContext;
use crate::{
    adv::higurashi::save_menu::resources::thumbnail_size,
    adv::higurashi::{reference_gpu_assets, reference_render_to_texture},
};
use shin_core::primitives::color::UnormColor;
use shin_render::shaders::types::texture::{TextureSource, TextureTarget, TextureTargetKind};
use std::borrow::Cow;
use wgpu::util::DeviceExt;

fn motion_cases() -> Vec<Case> {
    let mut descriptors = vec![
        (8, [0; 8], 0, 0),
        (8, [0; 8], 4, 1),
        (8, [0; 8], 9, 2),
        (8, [-200, 100, 0, 0, 0, 0, 0, 0], 0, 2),
    ];
    for mode in 0..4 {
        descriptors.push((9, [mode, 100, 1500, 0, 0, 0, 0, 0], 4, 0));
        descriptors.push((
            9,
            [mode, 100, 1500, 0, 0, 0, 0, 0],
            9,
            if mode % 2 == 0 { 1 } else { 2 },
        ));
    }
    descriptors
        .into_iter()
        .map(|(kind, params, old, profile)| Case {
            kind,
            params,
            old,
            profile,
            steps: 6,
            seed: 1,
        })
        .collect()
}
fn native_vertices(words: [u32; 16]) -> [PosTexVertex; 4] {
    std::array::from_fn(|i| PosTexVertex {
        position: vec2(
            f32::from_bits(words[4 * i]),
            f32::from_bits(words[4 * i + 1]),
        ),
        texture_position: vec2(
            f32::from_bits(words[4 * i + 2]),
            f32::from_bits(words[4 * i + 3]),
        ),
    })
}
fn images(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    prepared: &Prepared,
    owner: &Owner,
    props: &HigurashiProperties,
    draws: &[Draw],
    shaders: &[Vec<NativeShader>],
    size: (u32, u32),
) -> (Vec<u8>, Vec<u8>) {
    let background = UnormColor::from_rgba(173, 127, 91, 113);
    let actual = reference_render_to_texture(device, queue, size, |context, target| {
        let mut pass = context.begin_pass(
            TextureTarget {
                view: target,
                kind: TextureTargetKind::Screen,
            },
            Some(context.depth_stencil),
            "Live particle moving state",
        );
        pass.clear(Some(background), Some(0), Some(1.));
        for kind in [PassKind::Opaque, PassKind::Transparent] {
            owner
                .render(&mut pass, props, &HigurashiTransform::default(), 0, kind)
                .unwrap();
        }
    });
    let expected = reference_render_to_texture(device, queue, size, |context, target| {
        {
            let mut pass = context.begin_pass(
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::Screen,
                },
                None,
                "Original live particle background",
            );
            pass.clear(Some(background), None, None);
        }
        for draw in draws {
            let operation = [6, 9, 7, 8, 10, 11]
                .iter()
                .position(|&program| program == draw.program)
                .unwrap();
            let texture = [
                "Sakura",
                "focusline",
                "rain",
                "hanabira",
                "snow",
                "goldsnow",
            ][draw.texture as usize];
            shaders[(draw.blend - 2) as usize][operation].draw_quad(
                context,
                prepared.atlas.as_ref().unwrap().get(texture).as_source(),
                TextureTarget {
                    view: target,
                    kind: TextureTargetKind::Screen,
                },
                Mat4::from_cols_array(&draw.matrix.map(f32::from_bits)),
                Vec4::from_array(draw.color.map(f32::from_bits)),
                Vec4::from_array(draw.param.map(f32::from_bits)),
                &native_vertices(draw.vertices),
            );
        }
    });
    (actual, expected)
}
fn exact_pixels(actual: &[u8], expected: &[u8], context: &str) {
    assert_eq!(actual.len(), expected.len());
    let differences = actual.iter().zip(expected).filter(|(a, b)| a != b).count();
    let max = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert_eq!(differences, 0, "{context}:native channels differ,max{max}");
}
#[test]
#[ignore = "real live Focus/Rain initial and moving deep-copy packets, literal original GLSL and all5 supplied effects assets at both720p/1080p thumbnails; pure GPU, no audio/windows/user saves"]
fn higurashi_live_animation_moving_real_art_pixels_match_original() {
    let cases = motion_cases();
    assert_eq!(cases.len(), 12);
    let originals = native(&cases);
    let (device, queue, assets) = reference_gpu_assets();
    let shaders = (2..=4)
        .map(|blend| {
            (0..6)
                .map(|operation| NativeShader::new(&device, Some(operation), blend))
                .collect()
        })
        .collect::<Vec<Vec<_>>>();
    let mut frames = 0;
    let mut packets = 0;
    let mut moving_pairs = 0;
    let mut changed_channels = 0;
    let mut textures = std::collections::BTreeSet::new();
    for (index, (case, original)) in cases.iter().zip(originals).enumerate() {
        let context = format!("motion case{index} {case:?}");
        assert!(original.present);
        assert_eq!((original.os, original.mt), (0, 1));
        assert_eq!(original.copy_entropy, [0, 0]);
        let prepared =
            shin_tasks::block_on(Prepared::load(&assets, case.kind, &case.params)).unwrap();
        let mut properties = old_base(case.profile);
        let elapsed = if matches!(case.old, 4 | 9) {
            Some(123.25)
        } else {
            None
        };
        let mut owner = prepared
            .construct(&mut properties, elapsed, case.seed)
            .unwrap()
            .unwrap();
        check(&owner, &properties, &original.phases[0], &context);
        let actual_packets = owner
            .packets(
                &properties,
                &HigurashiTransform::default(),
                PassKind::Transparent,
            )
            .unwrap();
        assert_eq!(actual_packets.len(), original.draws[0].len());
        for (p, d) in actual_packets.iter().zip(&original.draws[0]) {
            equal_draw(p, d, &context);
            textures.insert(d.texture);
        }
        packets += original.draws[0].len();
        let mut initial = vec![];
        for canvas in [(1280, 720), (1920, 1080)] {
            let (actual, expected) = images(
                &device,
                &queue,
                &prepared,
                &owner,
                &properties,
                &original.draws[0],
                &shaders,
                thumbnail_size(canvas),
            );
            exact_pixels(
                &actual,
                &expected,
                &format!("{context}:initial pixels{canvas:?}"),
            );
            let colors: std::collections::BTreeSet<_> = expected.chunks_exact(4).collect();
            assert!(
                colors.len() >= 3,
                "{context}:initial must contain nonuniform real artwork"
            );
            initial.push(expected);
            frames += 1;
        }
        check(&owner, &properties, &original.phases[1], &context);
        for delta in [0., 0.25, 1., 2.5, 0., -0.25] {
            owner.advance(Ticks::from_f32(delta), &mut properties);
        }
        check(&owner, &properties, &original.phases[7], &context);
        let mut copy = owner.clone();
        let mut copied_properties = properties.clone();
        let source = snapshot(&owner);
        check(&copy, &copied_properties, &original.phases[8], &context);
        copy.advance(Ticks::from_f32(1.), &mut copied_properties);
        assert_eq!(
            snapshot(&owner),
            source,
            "Copy motion must leave source simulation intact"
        );
        check(&copy, &copied_properties, &original.phases[10], &context);
        let actual_packets = copy
            .packets(
                &copied_properties,
                &HigurashiTransform::default(),
                PassKind::Transparent,
            )
            .unwrap();
        assert_eq!(actual_packets.len(), original.draws[1].len());
        for (p, d) in actual_packets.iter().zip(&original.draws[1]) {
            equal_draw(p, d, &context);
            textures.insert(d.texture);
        }
        packets += original.draws[1].len();
        for ((canvas, previous), extent_index) in [(1280, 720), (1920, 1080)]
            .into_iter()
            .zip(initial)
            .zip(0..)
        {
            let (actual, expected) = images(
                &device,
                &queue,
                &prepared,
                &copy,
                &copied_properties,
                &original.draws[1],
                &shaders,
                thumbnail_size(canvas),
            );
            exact_pixels(
                &actual,
                &expected,
                &format!("{context}:moved-copy pixels{canvas:?}"),
            );
            let colors: std::collections::BTreeSet<_> = expected.chunks_exact(4).collect();
            assert!(
                colors.len() >= 3,
                "{context}:moved copy must contain nonuniform real artwork"
            );
            let changes = previous
                .iter()
                .zip(&expected)
                .filter(|(a, b)| a != b)
                .count();
            moving_pairs += usize::from(changes > 0);
            changed_channels += changes;
            frames += 1;
            assert!(extent_index < 2);
        }
        check(&copy, &copied_properties, &original.phases[11], &context);
        drop(owner);
        check(&copy, &copied_properties, &original.phases[12], &context);
    }
    assert_eq!(frames, 48);
    assert_eq!(textures, std::collections::BTreeSet::from([1, 2, 3, 4, 5]));
    assert!(
        moving_pairs >= 8,
        "Need visible original moving-state differences at both resolutions,got{moving_pairs}/24"
    );
    eprintln!(
        "Live animations:12 real-art owner profiles,{packets} original quad packets,{frames} byte-identical720p/1080p frames,{moving_pairs}/24 changed moving pairs,{changed_channels} changed channels;5 effects names,queued inherited clocks,private deep copies,original literal GLSL and nativeblend2/4;no audio/windows/user saves"
    );
}

// Independent raw WGPU replay copied from the frozen original GLSL transport.
// Literal SPV/native uniform words and a8630 equations only; no production
// Owner::render, RenderProgramWithArguments or fragment evaluator in reference.
struct ShaderDraw {
    count: u32,
    vertex_bytes: u32,
    words: [u32; 72],
}
struct NativeShader {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}
impl NativeShader {
    pub fn new(device: &wgpu::Device, operation: Option<usize>, blend: u32) -> Self {
        let dir =
            std::path::PathBuf::from(std::env::var("SHIN_HIGURASHI_POSTPROCESS_SHADERS").unwrap());
        let suffix = operation.map_or_else(|| "fill".into(), |n| n.to_string());
        let module = |stage| {
            let raw = std::fs::read(dir.join(format!("native-postprocess-{suffix}.{stage}.spv")))
                .unwrap();
            let words = raw
                .chunks_exact(4)
                .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
                .collect();
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Original updated framebuffer GLSL"),
                source: wgpu::ShaderSource::SpirV(Cow::Owned(words)),
            })
        };
        let vertex = module("vert");
        let fragment = module("frag");
        let mut entries = (0..3)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect::<Vec<_>>();
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 4,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = if operation.is_some() {
            vec![wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            }]
        } else {
            vec![
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Unorm8x4,
                    offset: 12,
                    shader_location: 1,
                },
            ]
        };
        // Executed native a8630 endpoints: RGB SrcAlpha/(1-SrcAlpha or1),
        // alpha (1-DstAlpha)/1; equation3 is ReverseSubtract per SDK enum.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: match blend {
                    2 => wgpu::BlendFactor::OneMinusSrcAlpha,
                    3 | 4 => wgpu::BlendFactor::One,
                    _ => panic!("Unreviewed original particle blend"),
                },
                operation: if blend == 4 {
                    wgpu::BlendOperation::ReverseSubtract
                } else {
                    wgpu::BlendOperation::Add
                },
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Original framebuffer GLSL reference"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vertex,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &fragment,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: shin_render::TEXTURE_FORMAT,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { layout, pipeline }
    }
    pub(super) fn draw(
        &self,
        context: &mut PreRenderContext,
        source: TextureSource,
        target: TextureTarget,
        draw: &ShaderDraw,
    ) {
        let buffers = draw.words[..48]
            .chunks_exact(16)
            .map(|values| {
                context
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytemuck::cast_slice(values),
                        usage: wgpu::BufferUsages::UNIFORM,
                    })
            })
            .collect::<Vec<_>>();
        let vertices = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(
                    &draw.words[48..48 + draw.vertex_bytes as usize / 4],
                ),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut entries = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect::<Vec<_>>();
        entries.push(wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::TextureView(source.view),
        });
        entries.push(wgpu::BindGroupEntry {
            binding: 4,
            resource: wgpu::BindingResource::Sampler(context.sampler_store.get(source.sampler)),
        });
        let bindings = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.layout,
                entries: &entries,
            });
        let mut pass = context
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                multiview_mask: None,
                label: Some("Original framebuffer shader"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
                    view: target.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..draw.count, 0..1);
    }
    pub fn draw_quad(
        &self,
        context: &mut PreRenderContext,
        source: TextureSource,
        target: TextureTarget,
        matrix: Mat4,
        color: Vec4,
        param: Vec4,
        vertices: &[PosTexVertex; 4],
    ) {
        let mut words = [0; 72];
        words[..16].copy_from_slice(&matrix.to_cols_array().map(f32::to_bits));
        words[16..20].copy_from_slice(&color.to_array().map(f32::to_bits));
        words[32..36].copy_from_slice(&param.to_array().map(f32::to_bits));
        words[48..64].copy_from_slice(bytemuck::cast_slice(vertices));
        self.draw(
            context,
            source,
            target,
            &ShaderDraw {
                count: 4,
                vertex_bytes: 64,
                words,
            },
        );
    }
}
