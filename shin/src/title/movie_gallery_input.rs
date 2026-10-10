//! MovieMode88d10: shared contact capture consumes input before controller.
use super::movie_gallery::{MovieControls, MovieGalleryList};
use crate::{
    adv::input_area::{InputArea, InputAreaTarget, PointerFrame},
    app::AppAction,
};
use enum_map::EnumMap;
use glam::Vec2;
use shin_input::ActionState;

impl InputAreaTarget for MovieGalleryList {
    fn probe(&mut self, point: Vec2) -> bool {
        self.pointer_probe(point)
    }
    fn reset(&mut self) {
        self.pointer_reset();
    }
    fn press(&mut self, _: Vec2) {
        self.pointer_press();
    }
    fn release(&mut self, _: Vec2) {
        self.pointer_release();
    }
    fn long_press(&mut self, _: Vec2) {} //89780 no-op.
    fn drag(&mut self, _: Vec2, _: Vec2) {
        self.pointer_cancel();
    }
    fn gesture(&mut self, _: Vec2) {} //89790 no-op; no gallery fling.
    fn cancel(&mut self) {
        self.pointer_cancel();
    }
}
pub(super) struct MovieGalleryInput {
    area: InputArea,
}
impl MovieGalleryInput {
    pub fn new() -> Self {
        Self {
            area: InputArea::canvas(),
        }
    }
    pub fn resize(&mut self, gallery: &mut MovieGalleryList, extent: (u16, u16)) {
        self.area.disable(gallery);
        self.area = InputArea::new(extent.0, extent.1);
    }
    ///The resource/playback owner supplies eligibility. Disabling cancels old
    ///contacts without manufacturing a release or a new Down on publication.
    pub fn input_frame(
        &mut self,
        gallery: &mut MovieGalleryList,
        enabled: bool,
        frame: &PointerFrame<'_>,
        state: &EnumMap<AppAction, ActionState>,
    ) {
        gallery.prepare_frame();
        if !enabled || gallery.closing() {
            self.area.disable(gallery);
            return;
        }
        if self.area.update(gallery, frame) {
            return;
        }
        gallery.controller(MovieControls {
            cancel: state[AppAction::Cancel].is_clicked,
            confirm: state[AppAction::Enter].is_clicked,
            left: state[AppAction::Left].is_clicked_or_repeated,
            right: state[AppAction::Right].is_clicked_or_repeated,
            up: state[AppAction::Up].is_clicked_or_repeated,
            down: state[AppAction::AnyDown].is_clicked_or_repeated,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adv::input_area::{PointerEvent, PointerKind};
    use shin_core::{
        format::{
            save::SaveData,
            scenario::{
                info::{
                    BgmIdOpt, MovieInfoItem, MovieTransparencyMode, MovieVolumeSource, PictureIdOpt,
                },
                instruction_elements::FromNumber,
            },
            text::U16String,
        },
        time::Ticks,
    };
    fn gallery() -> MovieGalleryList {
        let info = (0..10)
            .map(|id| MovieInfoItem {
                name: U16String::new(format!("movie{id}")),
                linked_picture_id: PictureIdOpt::from_number(0),
                linked_bgm_id: BgmIdOpt::from_number(-1),
                volume_source: MovieVolumeSource::Bgm,
                transparency: MovieTransparencyMode::Opaque,
            })
            .collect();
        let mut save = SaveData::default();
        for id in 0..10 {
            save.save_vectors.unlocked_movies.add(id);
        }
        MovieGalleryList::new(&info, &save).unwrap()
    }
    fn event(kind: PointerKind, x: i32, y: i32) -> PointerEvent {
        PointerEvent {
            kind,
            id: 0,
            position: glam::ivec2(x, y),
        }
    }
    fn clicked() -> ActionState {
        ActionState {
            is_clicked: true,
            is_clicked_or_repeated: true,
            ..Default::default()
        }
    }
    fn frame(
        input: &mut MovieGalleryInput,
        g: &mut MovieGalleryList,
        number: u32,
        time: f64,
        enabled: bool,
        events: &[PointerEvent],
        state: &EnumMap<AppAction, ActionState>,
    ) {
        input.input_frame(
            g,
            enabled,
            &PointerFrame {
                number,
                time_seconds: time,
                events,
            },
            state,
        );
    }
    #[test]
    fn thumbnail_insets_boundaries_gaps_and_missing_tail_are_not_plate_hits() {
        let g = gallery();
        assert_eq!(g.rows()[0].thumbnail_bounds(), [150.0, 203.0, 534.0, 419.0]);
        assert_eq!(g.rows()[9].thumbnail_bounds(), [562.0, 687.0, 946.0, 903.0]);
        for p in [glam::vec2(150.0, 203.0), glam::vec2(533.999, 418.999)] {
            assert_eq!(g.pointer_hit(p), Some(0));
        }
        for p in [
            glam::vec2(149.999, 203.0),
            glam::vec2(534.0, 250.0),
            glam::vec2(250.0, 419.0),
            glam::vec2(140.5, 250.0),
            glam::vec2(1000.0, 750.0),
            glam::vec2(250.0, 916.0),
        ] {
            assert_eq!(g.pointer_hit(p), None);
        }
        assert_eq!(g.pointer_hit(glam::vec2(562.0, 445.0)), Some(5));
    }
    #[test]
    fn half_size_tap_uses_original_row_and_waits_flash_without_simultaneous_cancel() {
        let mut g = gallery();
        let mut input = MovieGalleryInput::new();
        input.resize(&mut g, (960, 540));
        let mut state = EnumMap::default();
        state[AppAction::Cancel] = clicked();
        frame(
            &mut input,
            &mut g,
            0,
            0.0,
            true,
            &[event(PointerKind::Down, 300, 120)],
            &state,
        );
        g.update(Ticks::ZERO);
        frame(
            &mut input,
            &mut g,
            1,
            0.05,
            true,
            &[event(PointerKind::Up, 0, 0)],
            &state,
        );
        assert_eq!(g.selected(), 1);
        assert!(!g.closing());
        assert_eq!(g.take_sounds(), ["decide"]);
        g.update(Ticks::from_f32(7.0));
        assert!(g.take_requests().is_empty());
        g.update(Ticks::ZERO);
        assert_eq!(
            g.take_requests(),
            [super::super::movie_gallery::MovieRequest::OpenMovie(
                g.rows()[1].movie
            )]
        );
    }
    #[test]
    fn delayed_press_selects_silently_and_drag_cancels_feedback_without_playback() {
        let mut g = gallery();
        let mut input = MovieGalleryInput::new();
        let state = EnumMap::default();
        frame(
            &mut input,
            &mut g,
            0,
            0.0,
            true,
            &[event(PointerKind::Down, 600, 700)],
            &state,
        );
        g.update(Ticks::ZERO);
        frame(&mut input, &mut g, 1, 0.16, true, &[], &state);
        g.update(Ticks::from_f32(3.5));
        assert_eq!(g.selected(), 9);
        assert!((g.feedback(9).1 - 0.7).abs() < 0.00001);
        assert!(g.take_sounds().is_empty());
        frame(
            &mut input,
            &mut g,
            2,
            0.17,
            true,
            &[event(PointerKind::Move, 650, 700)],
            &state,
        );
        g.update(Ticks::from_f32(3.5));
        assert_eq!(g.feedback(9).1, 0.0);
        frame(
            &mut input,
            &mut g,
            3,
            0.18,
            true,
            &[event(PointerKind::Up, 600, 700)],
            &state,
        );
        g.update(Ticks::from_f32(20.0));
        assert_eq!(g.selected(), 9);
        assert!(!g.closing());
        assert!(g.take_requests().is_empty());
        assert!(g.take_sounds().is_empty());
    }
    #[test]
    fn gap_rejection_passes_controller_but_background_capture_consumes_it() {
        let mut g = gallery();
        let mut input = MovieGalleryInput::new();
        let mut state = EnumMap::default();
        state[AppAction::Enter] = clicked();
        frame(
            &mut input,
            &mut g,
            0,
            0.0,
            true,
            &[event(PointerKind::Down, 540, 250)],
            &state,
        );
        g.update(Ticks::ZERO);
        assert_eq!(g.take_sounds(), ["decide"]);
        state = EnumMap::default();
        state[AppAction::Cancel] = clicked();
        frame(
            &mut input,
            &mut g,
            1,
            0.1,
            true,
            &[event(PointerKind::Down, 20, 20)],
            &state,
        );
        g.update(Ticks::ZERO);
        frame(
            &mut input,
            &mut g,
            2,
            0.15,
            true,
            &[event(PointerKind::Up, 20, 20)],
            &state,
        );
        assert!(!g.closing());
        assert!(g.take_sounds().is_empty());
    }
    #[test]
    fn close_tap_uses_decide_and_close_drag_becomes_inert() {
        let mut g = gallery();
        let mut input = MovieGalleryInput::new();
        let state = EnumMap::default();
        frame(
            &mut input,
            &mut g,
            0,
            0.0,
            true,
            &[event(PointerKind::Down, 1800, 40)],
            &state,
        );
        g.update(Ticks::ZERO);
        frame(&mut input, &mut g, 1, 0.16, true, &[], &state);
        g.update(Ticks::from_f32(3.5));
        assert!((g.close_flash() - 0.7).abs() < 0.00001);
        frame(
            &mut input,
            &mut g,
            2,
            0.17,
            true,
            &[event(PointerKind::Move, 1800, 70)],
            &state,
        );
        g.update(Ticks::from_f32(3.5));
        frame(
            &mut input,
            &mut g,
            3,
            0.18,
            true,
            &[event(PointerKind::Up, 1800, 70)],
            &state,
        );
        g.update(Ticks::ZERO);
        assert!(!g.closing());
        assert_eq!(g.close_flash(), 0.0);
        assert!(g.take_requests().is_empty());
        frame(
            &mut input,
            &mut g,
            4,
            0.2,
            true,
            &[event(PointerKind::Down, 1800, 40)],
            &state,
        );
        g.update(Ticks::ZERO);
        frame(
            &mut input,
            &mut g,
            5,
            0.25,
            true,
            &[event(PointerKind::Up, 0, 0)],
            &state,
        );
        assert!(g.closing());
        assert_eq!(g.take_sounds(), ["decide"]);
    }
    #[test]
    fn losing_eligibility_or_resizing_cancels_capture_without_synthetic_release() {
        for resize in [false, true] {
            let mut g = gallery();
            let mut input = MovieGalleryInput::new();
            let state = EnumMap::default();
            frame(
                &mut input,
                &mut g,
                0,
                0.0,
                true,
                &[event(PointerKind::Down, 200, 250)],
                &state,
            );
            g.update(Ticks::ZERO);
            frame(&mut input, &mut g, 1, 0.16, true, &[], &state);
            g.update(Ticks::from_f32(3.5));
            if resize {
                input.resize(&mut g, (960, 540));
            } else {
                frame(&mut input, &mut g, 2, 0.17, false, &[], &state);
            }
            g.update(Ticks::from_f32(3.5));
            frame(&mut input, &mut g, 3, 0.18, true, &[], &state);
            g.update(Ticks::ZERO);
            frame(
                &mut input,
                &mut g,
                4,
                0.19,
                true,
                &[event(PointerKind::Up, 200, 250)],
                &state,
            );
            g.update(Ticks::from_f32(10.0));
            assert!(g.take_requests().is_empty());
            assert!(g.take_sounds().is_empty());
            assert_eq!(g.feedback(0).1, 0.0);
        }
    }
    #[test]
    #[ignore = "requires supplied executable alongside SHIN_REFERENCE_ROM"]
    fn reference_native_movie_pointer_hit_capture_and_feedback() {
        let path = std::env::var("SHIN_REFERENCE_ROM").unwrap();
        let exe = std::fs::read(
            std::path::Path::new(&path)
                .parent()
                .unwrap()
                .join("uncompressed_executable"),
        )
        .unwrap();
        let word = |a: usize| u32::from_le_bytes(exe[a + 256..a + 260].try_into().unwrap());
        for (a, expected) in [
            (0xcd810, 0xd65f03c0),
            (0xcd820, 0xd65f03c0),
            (0x89234, 0xbd49d902),
            (0x89248, 0xbd434102),
            (0x89258, 0xbd45dd02),
            (0x89268, 0xbd45e502),
            (0x89280, 0x1e222800),
            (0x89288, 0x1e222821),
            (0x892a8, 0xbd450900),
            (0x892b4, 0xbd46a100),
            (0x892bc, 0xbd45490a),
            (0x892c4, 0xbd482d0b),
            (0x892f0, 0x1e20284d),
            (0x89304, 0x1e21286c),
            (0x8930c, 0x94011145),
            (0x89348, 0x1e2921e0),
            (0x8934c, 0x1e2895c2),
            (0x89350, 0x1e209500),
            (0x89354, 0x54000065),
            (0x89358, 0x1e212120),
            (0x8935c, 0x540000e4),
            (0x89378, 0xb9400ab3),
            (0x893b0, 0xb900dc08),
            (0x893fc, 0xb900da60),
            (0x89438, 0xb900da69),
            (0x89444, 0x97ffff73),
            (0x89450, 0x531f7d00),
            (0x89544, 0xf9006c08),
            (0x89580, 0x7100051f),
            (0x89588, 0x91038260),
            (0x89594, 0xbd45a500),
            (0x8959c, 0xbd497501),
            (0x895dc, 0x9400c909),
            (0x8963c, 0xb900d274),
            (0x896a8, 0xf9437108),
            (0x896c8, 0x17fffe0e),
            (0x89730, 0xb900d274),
            (0x8975c, 0x17fffe41),
            (0x89780, 0xd65f03c0),
            (0x89790, 0xd65f03c0),
            (0x897b4, 0x37f801a8),
            (0x897d8, 0xb900da7f),
            (0x89818, 0x9400c87a),
            (0x898f4, 0x37f801a8),
            (0x89918, 0xb900da7f),
            (0x88d38, 0x97ff8af2),
            (0x88d3c, 0x37000cc0),
        ] {
            assert_eq!(word(a), expected, "native MovieMode pointer{a:x}");
        }
        for (a, value) in [
            (0x1599d8, 916.0f32),
            (0x159340, 136.0),
            (0x1595dc, 1784.0),
            (0x1595e4, 190.0),
            (0x159890, -136.0),
            (0x159728, -190.0),
            (0x159508, -384.0),
            (0x1596a0, -216.0),
            (0x159548, 384.0),
            (0x15982c, 216.0),
            (0x159480, 1764.0),
            (0x159694, 32.0),
            (0x15971c, 1848.0),
            (0x159658, 116.0),
            (0x159a20, 150.0),
            (0x159944, 992.0),
        ] {
            assert_eq!(
                word(a),
                value.to_bits(),
                "native MovieMode pointer constant{a:x}"
            );
        }
        let g = gallery();
        // Independent pitch/remainder oracle over every integer canvas point;
        // production hit testing walks the original row rectangles instead.
        for y in 0..1080 {
            for x in 0..1920 {
                let expected = if (136..1784).contains(&x) && (190..916).contains(&y) {
                    let dx = x - 136;
                    let dy = y - 190;
                    let id = (dy / 242 * 4 + dx / 412) as usize;
                    if (14..398).contains(&(dx % 412)) && (13..229).contains(&(dy % 242)) && id < 10
                    {
                        Some(id)
                    } else {
                        None
                    }
                } else {
                    None
                };
                assert_eq!(
                    g.pointer_hit(glam::vec2(x as f32, y as f32)),
                    expected,
                    "native MovieMode canvas hit{x}/{y}"
                );
            }
        }
        println!(
            "Original MovieMode pointer:47 instruction anchors,16 float constants and2073600 canvas hits pass"
        );
    }
}
