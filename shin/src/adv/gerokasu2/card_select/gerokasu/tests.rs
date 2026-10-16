use super::*;

fn settle(m: &mut GerokasuCardModel, frames: usize) {
    for _ in 0..frames {
        m.update(Ticks::from_f32(1.0));
        if m.events.iter().any(|e| matches!(e, CardEvent::StopVoice)) {
            m.voice_playing = false;
        }
    }
}
fn wait_phase(m: &mut GerokasuCardModel, phase: i32) {
    for _ in 0..2000 {
        if m.phase == phase {
            return;
        }
        // The cut-in voice ends.
        if m.phase == 5 {
            m.voice_playing = false;
        }
        settle(m, 1);
    }
    panic!("phase {} never reached {phase}", m.phase);
}
fn index_of(m: &GerokasuCardModel, kind: u8) -> i32 {
    m.hand.iter().position(|c| c.kind == kind).unwrap() as i32
}
/// Play `kind` onto `slot` with the keyboard path.
fn play(m: &mut GerokasuCardModel, kind: u8, slot: i32) {
    wait_phase(m, 1);
    m.selected = index_of(m, kind);
    assert!(m.confirm());
    wait_phase(m, 3);
    m.slot = slot;
    assert!(m.confirm());
    assert_eq!(m.events.last(), Some(&CardEvent::Sound("decide")));
    assert_eq!(m.phase, 4);
    if kind == PRISONER {
        for _ in 0..100 {
            if m.phase == 5 {
                break;
            }
            settle(m, 1);
        }
        assert!(m.voice_playing);
        settle(m, 200);
        // The cut-in holds phase 5 until the voice ends.
        assert_eq!(m.phase, 5);
    }
}

#[test]
fn constructor_counts_pairings_and_shuffles_hand() {
    // fu->ka (0), fu->mi (1), ka->fu (3).
    let m = GerokasuCardModel::new(0b1011 | 0x7000, 12345);
    assert_eq!(m.mask, 0b1011);
    assert_eq!(m.progress(), 3);
    assert_eq!(m.done_prisoner, [2, 1, 0, 0]);
    assert_eq!(m.done_pierrot, [1, 1, 1, 0]);
    let mut kinds: Vec<_> = m.hand.iter().map(|c| c.kind).collect();
    kinds.sort();
    assert_eq!(kinds, [0, 1, 2, 3]);
    assert!(m.hand.iter().enumerate().all(|(i, c)| c.id as usize == i));
    assert_eq!((m.phase, m.selected, m.slot, m.result), (0, 0, 0, -1));
}

#[test]
fn full_round_returns_pairing_and_plays_card_voice() {
    let mut m = GerokasuCardModel::new(0b100, 7);
    assert!(!m.ready());
    m.attach_listener();
    // Prisoner to slot 2 (karin), pierrot to slot 1 (doremi).
    play(&mut m, PRISONER, 2);
    assert!(m.events.contains(&CardEvent::Voice("sys/karin_card01".into())));
    play(&mut m, PIERROT, 1);
    play(&mut m, 2, 0);
    play(&mut m, 3, 3);
    wait_phase(&mut m, -1);
    assert!(m.success);
    assert_eq!(m.result(), Some(PAIRING[1 * 4 + 3]));
    assert_eq!(m.result(), Some(5));
    assert!(m.fade.value() >= 1.0);
}

#[test]
fn exhausted_and_completed_pairs_are_rejected() {
    // fuuka was prisoner three times (0, 1, 2) and ka->mi (4) is done.
    let mut m = GerokasuCardModel::new(0b10111, 1);
    m.attach_listener();
    wait_phase(&mut m, 1);
    m.selected = index_of(&m, PRISONER);
    m.confirm();
    wait_phase(&mut m, 3);
    m.slot = 0;
    assert!(m.confirm());
    assert_eq!(m.events.last(), Some(&CardEvent::Sound("error")));
    assert_eq!(
        m.message,
        Some(Message::Exhausted {
            heroine: 0,
            card: PRISONER
        })
    );
    assert_eq!(m.phase, 3);
    // Karin (slot 2) takes the prisoner; mina (slot 3) then cannot be pierrot.
    m.slot = 2;
    m.confirm();
    wait_phase(&mut m, 1);
    m.selected = index_of(&m, PIERROT);
    m.confirm();
    wait_phase(&mut m, 3);
    m.slot = 3;
    m.confirm();
    assert_eq!(m.message, Some(Message::PairDoneForPierrot));
    assert_eq!(m.message.unwrap().text(), "この＜死刑囚＞と＜ピエロ＞の組み合わせは実行済みです。");
}

#[test]
fn cancel_undoes_last_placement_and_restores_id_order() {
    let mut m = GerokasuCardModel::new(0, 3);
    m.attach_listener();
    wait_phase(&mut m, 1);
    // Nothing placed: B is refused.
    assert!(m.cancel());
    assert_eq!(m.events.last(), Some(&CardEvent::Sound("error")));
    play(&mut m, 2, 1);
    wait_phase(&mut m, 1);
    assert_eq!(m.hand.len(), 3);
    assert!(m.cancel());
    assert_eq!(m.events.last(), Some(&CardEvent::Sound("cancel")));
    wait_phase(&mut m, 1);
    assert_eq!(m.hand.len(), 4);
    assert!(m.placed.is_empty());
    assert!(m.hand.windows(2).all(|w| w[0].id < w[1].id));
    assert_eq!(m.hand[m.selected as usize].kind, 2);
}

#[test]
fn navigation_skips_placed_slots_without_wrapping() {
    let mut m = GerokasuCardModel::new(0, 9);
    m.attach_listener();
    play(&mut m, 2, 1);
    wait_phase(&mut m, 1);
    m.selected = 0;
    m.confirm();
    wait_phase(&mut m, 3);
    assert_eq!(m.slot, 0);
    m.navigate(1);
    assert_eq!(m.slot, 2);
    m.navigate(1);
    m.navigate(1);
    assert_eq!(m.slot, 3);
    m.navigate(-1);
    m.navigate(-1);
    assert_eq!(m.slot, 0);
    m.navigate(-1);
    assert_eq!(m.slot, 0);
}
