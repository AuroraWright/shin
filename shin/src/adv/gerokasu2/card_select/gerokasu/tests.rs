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
fn messages_localize_names_cards_and_sentences() {
    let table = crate::asset::localization::TextOverrides::parse(
        "\"風華\" = \"Fuuka\"\n\"＜死刑囚＞\" = \"<Prisoner>\"\n\
         \"これ以上{}に{}を渡すことはできません。\" = \"You can't give {} any more {}.\"\n\
         \"この組み合わせは既に実行済みです。\" = \"Already played.\"\n",
    )
    .unwrap();
    let localize = |text: &str| table.translate(text).into_owned();
    let exhausted = Message::Exhausted { heroine: 0, card: PRISONER };
    assert_eq!(exhausted.localized_text(localize), "You can't give Fuuka any more <Prisoner>.");
    assert_eq!(Message::AlreadyPlayed.localized_text(localize), "Already played.");
    // Missing keys keep the source.
    assert_eq!(
        Message::PairDoneForPierrot.localized_text(localize),
        Message::PairDoneForPierrot.text()
    );
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

/// Drive the model with the debug autoplayer until QUIZ has a result.
fn autoplay(m: &mut GerokasuCardModel) -> i32 {
    for _ in 0..20_000 {
        if let Some(result) = m.result() {
            return result;
        }
        if m.phase == 5 {
            m.voice_playing = false;
        }
        match m.autoplay_action() {
            Some(AutoAction::Navigate(direction)) => assert!(m.navigate(direction)),
            Some(AutoAction::Confirm) => assert!(m.confirm()),
            Some(AutoAction::Cancel) => assert!(m.cancel()),
            None => {}
        }
        settle(m, 1);
    }
    panic!("autoplay stalled in phase {} with {:?}", m.phase, m.placed);
}

#[test]
fn debug_autoplay_backs_out_of_dead_ends_and_finds_an_unfinished_pairing() {
    let finishable = |mask: u32| {
        (0..4).any(|p| {
            (0..4).any(|q| {
                let index = PAIRING[p * 4 + q];
                index >= 0 && mask & (1 << index) == 0
            })
        })
    };
    let mut dead_ends = 0;
    for mask in (0..0x1000u32).filter(|&mask| finishable(mask)).step_by(7) {
        for seed in [1, 99, 4242] {
            let mut m = GerokasuCardModel::new(mask as i32, seed);
            m.attach_listener();
            // Like the stalled playtest: a judge and a prisoner leave only
            // slots whose pierrot pairings are done, when the mask allows it.
            let dead_end = (0..4)
                .flat_map(|j| (0..4).map(move |p| (j, p)))
                .find(|&(j, slot)| {
                    let p = SLOT_HEROINE[slot];
                    j != slot
                        && m.done_prisoner[p] < 3
                        && (0..4).filter(|&q| q != slot && q != j).all(|q| {
                            let index = PAIRING[p * 4 + SLOT_HEROINE[q]];
                            index >= 0 && mask & (1 << index) != 0
                        })
                });
            if let Some((judge, slot)) = dead_end {
                dead_ends += 1;
                play(&mut m, 2, judge as i32);
                play(&mut m, PRISONER, slot as i32);
            }
            let result = autoplay(&mut m);
            assert!((0..12).contains(&result), "mask {mask:#x}: {result}");
            assert_eq!(
                mask & (1 << result),
                0,
                "mask {mask:#x} repeated pairing {result}"
            );
        }
    }
    assert!(dead_ends > 0);
}
