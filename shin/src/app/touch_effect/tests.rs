use super::*;
mod native_render;
mod higurashi_literal_ripple;
mod native_particles;

#[test]
fn taps_ripple_and_only_drags_emit_stars() {
    let mut effect = TouchEffect::with_seed(5489);
    let p = Vec2::new(960.0, 540.0);
    effect.update(Ticks::ZERO, false, true, p);
    effect.update(Ticks::ZERO, true, false, p);
    assert!(effect.stars.is_empty() && effect.ripples.is_empty());
    effect.update(Ticks::ZERO, true, true, p);
    effect.update(Ticks::from_f32(20.0), true, true, p);
    assert!(
        effect.stars.is_empty(),
        "Stationary held taps do not emit drag stars"
    );
    effect.update(Ticks::ZERO, true, false, p);
    assert_eq!(effect.ripples.len(), 1);
    effect.update(Ticks::ZERO, true, true, p);
    effect.update(Ticks::ZERO, true, true, p + Vec2::X * 8.0);
    assert!(effect.stars.is_empty());
    effect.update(Ticks::ZERO, true, true, p + Vec2::X * 20.0);
    assert_eq!(effect.stars.len(), 4);
    effect.update(Ticks::ZERO, true, false, p);
    assert_eq!(
        effect.ripples.len(),
        1,
        "Drag release cannot generate a tap"
    );
    effect.update(Ticks::from_f32(61.0), false, false, p);
    assert!(effect.stars.is_empty() && effect.ripples.is_empty());
    effect.update(Ticks::ZERO, true, true, p);
    effect.cancel_contact();
    effect.update(Ticks::ZERO, true, false, p);
    assert!(
        effect.ripples.is_empty(),
        "Focus/viewport cancellation cannot become a tap"
    );
    effect.update(Ticks::ZERO, true, true, Vec2::new(-1.0, 540.0));
    effect.update(Ticks::ZERO, true, false, p);
    assert!(
        effect.ripples.is_empty(),
        "Letterbox press cannot become a tap"
    );
}
