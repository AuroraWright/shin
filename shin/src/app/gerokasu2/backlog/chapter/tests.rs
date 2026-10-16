use super::*;
use crate::adv::gerokasu2::backlog::chapter::Phase;

fn input() -> Input {
    Input {
        permission: true,
        first_row: 0,
        last_row: 15,
        ..Input::default()
    }
}
#[test]
fn chapter_contact_bridge_keeps_native_capture_and_close_origin_order() {
    let mut model = Controller::new(16, &(0..16).collect::<Vec<_>>(), 5, 7).unwrap();
    let mut sampler = PointerSampler::default();
    let mut area = InputArea::canvas();
    let mut dispatch = |model: &mut Controller, point: Vec2, held: bool, delta: f32| {
        sampler.sample(point, held, 0.016);
        model.update_with_contact(delta, input(), |model| {
            let mut events = vec![];
            let captured = area.update_with_mouse_release(
                &mut Contact {
                    model,
                    events: &mut events,
                },
                &sampler.frame(),
                sampler.mouse_release(),
            );
            (captured, events)
        })
    };
    dispatch(&mut model, Vec2::new(1800., 100.), true, 5.);
    let events = dispatch(&mut model, Vec2::new(1800., 100.), false, 1.);
    assert_eq!(events, [Event::Decide, Event::CloseBrowser]);
    assert_eq!((model.phase(), model.progress()), (Phase::Closing, 1.));
    model.update(1., input());
    assert_eq!(model.progress(), 0.8);
}
#[test]
fn chapter_gpu_adapter_keeps_gradient_channels_clip_state_and_padded_anchor() {
    let model = Controller::new(16, &(0..16).collect::<Vec<_>>(), 5, 7).unwrap();
    let frame = artwork::frame(1., 60., model.entries(), model.selected(), 0, 15, |_| {
        Marker::Other
    });
    let left = vertices(&frame.panel[1]);
    assert_eq!(left[0].color.0.to_le_bytes(), [128, 128, 128, 0]);
    assert_eq!(left[1].color.0.to_le_bytes(), [128, 128, 128, 192]);
    let track = vertices(&frame.scrollbar.unwrap()[0]);
    assert_eq!(track[0].color.0.to_le_bytes(), [16, 16, 64, 96]);
    let params = layout_params();
    assert_eq!((params.layout_width, params.text_size), (1274., 48.));
    assert!(!params.perform_soft_breaks && !params.always_leave_space_for_rubi);
    let clip = stencil_state(artwork::CLIP_TEXT);
    assert_eq!(clip.stencil.pipeline.function, StencilFunction::Equal);
    assert_eq!(clip.stencil.pipeline.pass_operation, StencilOperation::Keep);
    assert_eq!(clip.stencil.stencil_reference, 1);
    let matrix = text_transform(
        artwork::PROJECTION,
        [1800., 63.],
        [1., 0.],
        [1., 1.],
        Vec2::new(106., 54.),
    )
    .to_cols_array();
    assert_eq!(matrix[12], artwork::PROJECTION[0] * 1694. - 1.);
    assert_eq!(matrix[13], artwork::PROJECTION[5] * 63. + 1.);
}

#[path = "gpu_tests.rs"]
mod gpu_tests;
