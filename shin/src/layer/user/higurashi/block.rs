//! Original PIC/BUP6a04c block ranges, straight-RGBA shaders and blend states.
use glam::Mat4;
use shin_render::{
    ColorBlendType, DrawPrimitive, RenderProgramWithArguments, RenderRequestBuilder,
    render_pass::RenderPass, shaders::types::buffer::VertexSource,
};

use crate::{
    asset::picture::GpuPictureBlock,
    layer::user::picture_layer::{PictureBlockParams, PictureBlockPassKind},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Submission {
    pub index_start: usize,
    pub index_count: usize,
    pub blend: ColorBlendType,
}

/// Counts are indices, matching native block+4/+6, rather than rectangles.
pub(super) fn submissions(
    native_format: u16,
    opaque: usize,
    transparent: usize,
    params: PictureBlockParams,
) -> [Option<Submission>; 2] {
    let regular = ColorBlendType::from_higurashi_layer(params.blend_type);
    let range = |start, count, blend| {
        (count != 0).then_some(Submission {
            index_start: start,
            index_count: count,
            blend,
        })
    };
    match params.pass_kind {
        PictureBlockPassKind::OpaqueOnly => [range(0, opaque, ColorBlendType::Opaque), None],
        PictureBlockPassKind::TransparentOnly => [range(opaque, transparent, regular), None],
        PictureBlockPassKind::OpaqueAndTransparent if native_format & 3 == 2 => [
            range(opaque, transparent, regular),
            range(0, opaque, regular),
        ],
        PictureBlockPassKind::OpaqueAndTransparent => {
            [range(0, opaque + transparent, regular), None]
        }
    }
}

pub(crate) fn render(
    block: &GpuPictureBlock,
    pass: &mut RenderPass,
    builder: RenderRequestBuilder,
    params: PictureBlockParams,
    transform: Mat4,
) {
    for submission in submissions(
        block.native_format,
        block.opaque_rect_count * GpuPictureBlock::INDICES_PER_RECT,
        block.transparent_rect_count * GpuPictureBlock::INDICES_PER_RECT,
        params,
    )
    .into_iter()
    .flatten()
    {
        let vertices = VertexSource::VertexAndIndexBuffer {
            vertices: block.vertex_buffer.as_buffer_ref(),
            indices: block
                .index_buffer
                .as_sliced_buffer_ref(submission.index_start, submission.index_count),
        };
        // Programs6/9/7/8/10/11 are the same ordinary programs used by the
        // original framebuffer path. They do not premultiply RGB on output.
        pass.run(builder.color_blend_type(submission.blend).build(
            RenderProgramWithArguments::HigurashiPostprocess {
                vertices,
                texture: block.texture.as_source(),
                transform,
                color: params.color_multiplier,
                operation: params.fragment_shader,
                param: params.fragment_shader_param,
            },
            DrawPrimitive::Triangles,
        ));
    }
}

/// Production older Core6a04c culls only nonempty index ranges, using the
/// original matrix before the GL-to-WGPU depth bridge. Zero products in that
/// bridge can affect exceptional X/Y/W values, so its result is not a culler.
pub(crate) fn render_native(
    block: &GpuPictureBlock,
    pass: &mut RenderPass,
    builder: RenderRequestBuilder,
    params: PictureBlockParams,
    native_transform: Mat4,
) {
    let ranges = submissions(
        block.native_format,
        block.opaque_rect_count * GpuPictureBlock::INDICES_PER_RECT,
        block.transparent_rect_count * GpuPictureBlock::INDICES_PER_RECT,
        params,
    );
    if ranges.iter().all(Option::is_none) {
        return;
    }
    let [x, y, width, height] = block.native_bounds;
    if !(super::leaf_clip::BlockBounds {
        x: x as i16,
        y: y as i16,
        width,
        height,
    })
    .visible(native_transform)
    {
        return;
    }
    render(
        block,
        pass,
        builder,
        params,
        crate::layer::render_params::higurashi::HigurashiTransform::convert_clip_depth(
            native_transform,
        ),
    );
}
