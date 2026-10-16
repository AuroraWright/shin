//! Original native depth/stencil/blend state words translated to wgpu, shared
//! by the independent native-program replays in tests.

pub(crate) fn compare(value: u32) -> wgpu::CompareFunction {
    match value {
        0 => wgpu::CompareFunction::Never,
        1 => wgpu::CompareFunction::Less,
        2 => wgpu::CompareFunction::Equal,
        3 => wgpu::CompareFunction::LessEqual,
        4 => wgpu::CompareFunction::Greater,
        5 => wgpu::CompareFunction::NotEqual,
        6 => wgpu::CompareFunction::GreaterEqual,
        7 => wgpu::CompareFunction::Always,
        value => panic!("Unreviewed original compare {value}"),
    }
}

pub(crate) fn stencil_operation(value: u32) -> wgpu::StencilOperation {
    match value {
        0 => wgpu::StencilOperation::Keep,
        1 => wgpu::StencilOperation::Zero,
        2 => wgpu::StencilOperation::Replace,
        3 => wgpu::StencilOperation::IncrementClamp,
        4 => wgpu::StencilOperation::DecrementClamp,
        5 => wgpu::StencilOperation::Invert,
        6 => wgpu::StencilOperation::IncrementWrap,
        7 => wgpu::StencilOperation::DecrementWrap,
        value => panic!("Unreviewed original stencil operation {value}"),
    }
}

pub(crate) fn native_blend(value: u32) -> wgpu::BlendState {
    // Original a8630 endpoint + SDK enum names. This is independent of the
    // shared ColorBlendType conversion, including older RGB ReverseSubtract.
    // https://smo.monsterdruide.one/docs/nvn_8h.html
    use wgpu::{BlendComponent, BlendFactor as F, BlendOperation as O};
    if value == 1 {
        return wgpu::BlendState {
            color: BlendComponent {
                src_factor: F::One,
                dst_factor: F::Zero,
                operation: O::Add,
            },
            alpha: BlendComponent {
                src_factor: F::One,
                dst_factor: F::Zero,
                operation: O::Add,
            },
        };
    }
    let (source, destination, operation) = match value {
        2 => (F::SrcAlpha, F::OneMinusSrcAlpha, O::Add),
        3 => (F::SrcAlpha, F::One, O::Add),
        4 => (F::SrcAlpha, F::One, O::ReverseSubtract),
        5 => (F::One, F::OneMinusSrcAlpha, O::Add),
        value => panic!("Unreviewed original blend {value}"),
    };
    wgpu::BlendState {
        color: BlendComponent {
            src_factor: source,
            dst_factor: destination,
            operation,
        },
        alpha: BlendComponent {
            src_factor: F::OneMinusDstAlpha,
            dst_factor: F::One,
            operation: O::Add,
        },
    }
}
