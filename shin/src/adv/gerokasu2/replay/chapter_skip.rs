//! Plus/Minus chapter navigation targets: forward search `EBB0` (Plus, Yes
//! callback `18DB0`) and the Minus Yes callback `18A84`. Both return the
//! `E820` fork that `105E0` hands to reconstruction mode 0.
use super::*;

impl ReplaySession {
    /// Command skip polls (vtable +0x18).
    fn skip_poll(&mut self, skip_unread: bool) -> Result<bool> {
        let p = self.pending.as_mut().unwrap();
        match &p.command {
            Cmd::MessageSet { .. } => Ok(self.save.timeline.event >= 0
                || skip_unread
                || self.save.descriptors.current_was_read != 0),
            Cmd::Selection { .. } | Cmd::Shared(RuntimeCommand::QUIZ(_)) => Ok(p.replay_stop > 0),
            // +1A is set only when the replay poll reached the saved boundary.
            Cmd::Shared(RuntimeCommand::AUTOSAVE(_)) => {
                Ok(p.autosave.as_ref().unwrap().flags()[1] != 0)
            }
            Cmd::Shared(RuntimeCommand::EXIT(_) | RuntimeCommand::RESUME(_)) => Ok(false),
            // SGET/SSET skip polls run their live bodies on the clone.
            Cmd::Shared(RuntimeCommand::SGET(c)) => {
                let value = self.save.persistent_get(c.slot_number);
                let destination = self
                    .vm
                    .pending_destination()
                    .context("Ger skip SGET destination missing")?;
                self.vm.write_pending_result(destination, value)?;
                self.response = CommandResult::WriteMemory(destination, value);
                Ok(true)
            }
            Cmd::Shared(RuntimeCommand::SSET(c)) => {
                self.save.variables.set(c.slot_number, c.value);
                Ok(true)
            }
            _ => Ok(true),
        }
    }
}

/// `EBB0`: clone the live state (`E500`), replay polls until one blocks
/// (without refork), then skip polls until one stops. The target is the
/// clone's `last_skip`, used only past the source boundary.
pub fn next(
    scenario: &Arc<Gerokasu2Scenario>,
    live: &SaveSession,
    seen: &IndexSet,
    skip_unread: bool,
) -> Result<Option<ReplaySession>> {
    let source = live.timeline.boundary;
    let (save, restored) = live.fork_for_restore(source, source);
    let mut clone = ReplaySession::new(scenario.clone(), save, restored)?;
    loop {
        clone.prepare(seen, 100000)?;
        if !clone.poll()? {
            break;
        }
        clone.pending = None;
    }
    loop {
        clone.prepare(seen, 100000)?;
        if !clone.skip_poll(skip_unread)? {
            break;
        }
        clone.pending = None;
    }
    let target = clone.save.timeline.last_skip;
    clone.save.timeline.boundary = target;
    if target <= source {
        return Ok(None);
    }
    let (save, restored) = clone.save.fork_for_restore(target, source);
    ReplaySession::new(scenario.clone(), save, restored).map(Some)
}

/// `ECC0` (B during an event, `12C4C`): clone, replay polls, then skip polls
/// while the event lasts; fork at the clone's boundary. Null outside events.
pub fn event(
    scenario: &Arc<Gerokasu2Scenario>,
    live: &SaveSession,
    seen: &IndexSet,
    skip_unread: bool,
) -> Result<Option<ReplaySession>> {
    if live.timeline.event < 0 {
        return Ok(None);
    }
    let source = live.timeline.boundary;
    let (save, restored) = live.fork_for_restore(source, source);
    let mut clone = ReplaySession::new(scenario.clone(), save, restored)?;
    loop {
        clone.prepare(seen, 100000)?;
        if !clone.poll()? {
            break;
        }
        clone.pending = None;
    }
    loop {
        clone.prepare(seen, 100000)?;
        if !clone.skip_poll(skip_unread)? {
            break;
        }
        clone.pending = None;
        if clone.save.timeline.event < 0 {
            break;
        }
    }
    let target = clone.save.timeline.boundary;
    let (save, restored) = clone.save.fork_for_restore(target, source);
    ReplaySession::new(scenario.clone(), save, restored).map(Some)
}

/// `18A84`: the current chapter once it has more than one message, else
/// the previous one; `E820(vm, t, t)`.
pub fn previous(scenario: &Arc<Gerokasu2Scenario>, live: &SaveSession) -> Result<ReplaySession> {
    let t = &live.timeline;
    let target = if t.chapter_messages > 1 {
        t.current_chapter
    } else {
        t.previous_chapter
    };
    let (save, restored) = live.fork_for_restore(target, target);
    ReplaySession::new(scenario.clone(), save, restored)
}

#[cfg(test)]
mod tests {
    use super::{super::tests::*, *};

    fn ready(mut replay: ReplaySession, seen: &IndexSet) -> ReplaySession {
        for _ in 0..1000 {
            if replay.pump(seen, 4096, 100000, || false).unwrap() == ReplayStatus::Ready {
                return replay;
            }
        }
        panic!("Ger chapter replay did not become ready");
    }

    #[test]
    fn ger_chapter_forward_search_stops_at_unread_or_choice() {
        let s = scenario(|d| {
            for _ in 0..3 {
                message(d);
            }
            d.push(0x8d);
            d.extend(1u16.to_le_bytes());
            d.extend(1u16.to_le_bytes());
            d.extend(0u16.to_le_bytes());
            num(d, 5);
            text(d, "");
            let s = "A\0B\0\0";
            d.extend((s.len() as u16).to_le_bytes());
            d.extend(s.as_bytes());
            message(d);
        });
        let unread = IndexSet::new();
        let live = ready(saved(s.clone(), 1, 1, vec![]), &unread);
        assert_eq!(live.save.timeline.boundary, 1);
        // Unread and disallowed: last_skip stays at the source boundary.
        assert!(next(&s, &live.save, &unread, false).unwrap().is_none());
        // Allowed (or read): every message skips and the choice stops. A
        // choice under displayed text shares its message's boundary (3), so
        // the fork lands on that message, just before the choice.
        let result = next(&s, &live.save, &unread, true).unwrap().unwrap();
        let entry = result.save.progress.active_entry();
        assert_eq!((entry.save_position, entry.replay_target), (3, 1));
        let result = ready(result, &unread);
        assert_eq!(result.save.timeline.boundary, 3);
        assert!(matches!(
            result.pending.as_ref().unwrap().command,
            Cmd::MessageSet { .. }
        ));
        // At the choice itself there is no further target.
        assert!(next(&s, &result.save, &unread, true).unwrap().is_none());
        // Minus: three messages in the (zeroth) chapter return to its start.
        assert_eq!(result.save.timeline.chapter_messages, 3);
        let back = previous(&s, &result.save).unwrap();
        assert_eq!(back.save.progress.active_entry().save_position, 0);
    }

    #[test]
    fn ger_event_skip_forks_past_the_event_end() {
        let s = scenario(|d| {
            message(d);
            d.push(0xa2);
            num(d, 7);
            message(d);
            message(d);
            d.push(0xa3);
            message(d);
            d.extend([0, 0, 0]);
        });
        let seen = IndexSet::new();
        let mut live = ready(saved(s.clone(), 1, 1, vec![]), &seen);
        assert!(event(&s, &live.save, &seen, false).unwrap().is_none());
        // Enter the event as the live VM does: EVBEGIN advances to 2.
        while live.save.timeline.event < 0 {
            live.pending = None;
            live.prepare(&seen, 100000).unwrap();
        }
        assert_eq!(live.save.timeline.boundary, 2);
        // Unread event messages still skip; EVEND's boundary is the target.
        let result = event(&s, &live.save, &seen, false).unwrap().unwrap();
        let entry = result.save.progress.active_entry();
        assert_eq!((entry.save_position, entry.replay_target), (3, 2));
        let result = ready(result, &seen);
        assert_eq!(result.save.timeline.event, -1);
        assert!(result.save.timeline.boundary >= 3);
    }

    #[test]
    #[ignore = "requires supplied Ger ROM; memory replay only, no hardware or live effects"]
    fn ger_chapter_skip_authored_targets_without_hardware() {
        use shin_core::{format::rom::RomReader, primitives::stateless_reader::StatelessFile};
        use std::io::Read;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let mut rom = RomReader::new(
            StatelessFile::new(
                std::fs::File::open(root.join("game-data/gerokasu2/data.rom")).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let file = rom.find_file("/main.snr").unwrap();
        let mut bytes = Vec::new();
        rom.open_file(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let s = Arc::new(Gerokasu2Scenario::new(bytes.into()).unwrap());
        let seen = IndexSet::new();
        for source in [20, 383, 1000] {
            let live = ready(saved(s.clone(), source, source, vec![0; 4096]), &seen);
            let started = std::time::Instant::now();
            let result = next(&s, &live.save, &seen, true).unwrap().unwrap();
            let elapsed = started.elapsed();
            let target = result.save.progress.active_entry().save_position;
            assert!(target > live.save.timeline.boundary);
            let result = ready(result, &seen);
            assert_eq!(result.save.timeline.boundary, target);
            let stop = &result.pending.as_ref().unwrap().command;
            assert!(
                matches!(
                    stop,
                    Cmd::Selection { .. }
                        | Cmd::Shared(
                            RuntimeCommand::AUTOSAVE(_)
                                | RuntimeCommand::QUIZ(_)
                                | RuntimeCommand::EXIT(_)
                        )
                ),
                "{stop:?}"
            );
            let t = result.save.timeline;
            let back = previous(&s, &result.save).unwrap();
            let back_target = back.save.progress.active_entry().save_position;
            assert!(back_target <= t.current_chapter);
            let back = ready(back, &seen);
            assert_eq!(back.save.timeline.boundary, back_target);
            println!(
                "GEROKASU2_CHAPTER_SKIP source={} next={target} stop={} chapter={} previous={back_target} search_ms={}",
                live.save.timeline.boundary,
                match stop {
                    Cmd::Selection { .. } => "SELECT",
                    Cmd::Shared(RuntimeCommand::AUTOSAVE(_)) => "AUTOSAVE",
                    Cmd::Shared(RuntimeCommand::QUIZ(_)) => "QUIZ",
                    _ => "EXIT",
                },
                t.current_chapter,
                elapsed.as_millis()
            );
        }
    }
}
