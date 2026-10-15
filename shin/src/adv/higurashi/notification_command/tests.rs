use super::*;
use crate::adv::higurashi::{
    input::ReadState,
    layers::replay_snapshot,
    read_state::{Checkpoint, CommandReadState, ReadServices},
};
use shin_core::format::scenario::instruction_elements::MessageId;
use std::fmt::Write as _;

const INPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/notification-command-capture/input.txt"
));
const OUTPUT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../reverse-engineering/higurashi/notification-command-capture/output.txt"
));
fn hex(word: &str) -> u32 {
    u32::from_str_radix(word, 16).unwrap()
}
#[derive(Default)]
struct Transport {
    ready: bool,
    callback: bool,
    requested: bool,
    attached: bool,
    polls: u32,
    events: String,
    params: Vec<u32>,
    fail_prepare: bool,
    fail_poll: bool,
}
impl Services for Transport {
    fn prepare(
        &mut self,
        request: &Request,
        target: Target,
        _: &mut HigurashiLayers,
    ) -> Result<bool> {
        ensure!(!self.fail_prepare, "Checked notification producer failure");
        assert_eq!(target.bank, 0);
        if !self.requested {
            self.requested = true;
            self.events.push('F');
            let params = request.producer_params();
            let count = if request.kind() == 13 {
                1
            } else {
                1 + params[0] as usize
            };
            self.params = params[..count].iter().map(|value| *value as u32).collect();
        }
        self.polls += 1;
        if self.ready {
            assert!(!self.attached);
            self.attached = true;
            self.events.push_str("TWSU");
        }
        Ok(self.ready)
    }
    fn poll(&mut self) -> Result<bool> {
        ensure!(!self.fail_poll, "Checked notification callback failure");
        Ok(self.callback)
    }
}
fn compare(actual: &str, original: &str, operation: usize) {
    let actual = actual.split_whitespace().collect::<Vec<_>>();
    let original = original.split_whitespace().collect::<Vec<_>>();
    assert_eq!(
        actual.len(),
        original.len(),
        "Notification token count at operation {operation}"
    );
    if let Some((word, (actual, original))) = actual
        .iter()
        .zip(&original)
        .enumerate()
        .find(|(_, (a, b))| a != b)
    {
        panic!(
            "Notification original comparison operation {operation}, word {word}: Rust {actual}, native {original}"
        );
    }
}

#[test]
fn actual_otsu_kakera_commands_match_42_original_owners() {
    let mut original = OUTPUT.lines();
    let mut layers: Option<HigurashiLayers> = None;
    let mut owner: Option<NotificationCommand> = None;
    let mut target = None;
    let mut services = Transport::default();
    let mut kind = 0;
    let mut released = false;
    let mut profiles = 0;
    for (index, input) in INPUT.lines().enumerate() {
        let operation = index + 1;
        let mut args = input.split_whitespace();
        let op = args.next().unwrap().chars().next().unwrap();
        let mut result = -1;
        match op {
            'N' => {
                assert!(owner.is_none());
                kind = args.next().unwrap().parse::<i32>().unwrap();
                let used = args.next().unwrap().parse::<usize>().unwrap();
                let _source_seed = hex(args.next().unwrap());
                let argument = hex(args.next().unwrap()) as i32;
                let count = args.next().unwrap().parse::<usize>().unwrap();
                let ids = (0..count)
                    .map(|_| hex(args.next().unwrap()) as i32)
                    .collect::<Vec<_>>();
                let command = if kind == 13 {
                    Cmd::OTSUGET { argument }
                } else {
                    Cmd::KAKERAGET {
                        argument,
                        ids: ids.into_iter().collect(),
                    }
                };
                let mut created_layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
                for i in 0..used {
                    assert_eq!(created_layers.state.allocate((i * 2) as u16), Some(i as u8));
                }
                let created =
                    NotificationCommand::new(&command, &mut created_layers.state).unwrap();
                target = created.target();
                layers = Some(created_layers);
                owner = Some(created);
                services = Transport::default();
                released = false;
                profiles += 1;
            }
            'R' => result = i32::from(owner.as_ref().unwrap().poll_replay()),
            'P' => result = i32::from(owner.as_ref().unwrap().forward_complete()),
            'L' => {
                result = i32::from(
                    owner
                        .as_mut()
                        .unwrap()
                        .poll_live(layers.as_mut().unwrap(), &mut services)
                        .unwrap(),
                )
            }
            'Q' => services.ready = args.next().unwrap() == "1",
            'V' => services.callback = true, // Actual ec1c/fc70 thunk captured independently.
            'D' => {
                owner
                    .as_mut()
                    .unwrap()
                    .release(&mut layers.as_mut().unwrap().state);
                if services.attached {
                    services.events.push_str("us");
                }
                released = true;
            }
            _ => unreachable!(),
        }
        assert!(args.next().is_none());
        let owner_ref = owner.as_ref().unwrap();
        let bank = &layers.as_ref().unwrap().state;
        let (logical, slot, mapped) = target.map_or((-1, -1, 32), |target| {
            (
                i32::from(target.logical),
                i32::from(target.slot),
                bank.target(target.logical)
                    .map_or(32, |target| u32::from(target.slot)),
            )
        });
        let job_refs = u32::from(services.requested && !released);
        let child_refs = if services.attached && !released { 2 } else { 0 };
        let mut actual = format!(
            "O {operation} {op} {kind} {result} {} {} {} {} {} {} {} {logical} {slot} {mapped} {job_refs} {child_refs} {child_refs} {:016x} {} P {}",
            u32::from(owner_ref.skipped()),
            u32::from(services.callback),
            u32::from(services.requested),
            services.polls,
            u32::from(services.attached),
            u32::from(services.attached),
            if services.attached { 3 } else { 0 },
            replay_snapshot(bank),
            if services.events.is_empty() {
                "-"
            } else {
                &services.events
            },
            services.params.len()
        );
        for value in &services.params {
            write!(actual, " {value}").unwrap();
        }
        compare(&actual, original.next().unwrap(), operation);
        if op == 'L' && services.callback {
            assert!(owner_ref.callback_received());
        }
        if released {
            assert!(owner_ref.target().is_none());
            owner = None;
        }
    }
    assert_eq!(profiles, 42);
    assert!(original.next().is_none());
}

#[derive(Default)]
struct Reads {
    checkpoints: usize,
}
impl ReadServices for Reads {
    fn checkpoint(&mut self, _: Checkpoint, _: ReadState) -> Result<()> {
        self.checkpoints += 1;
        Ok(())
    }
    fn message_is_read(&self, _: u32) -> bool {
        false
    }
    fn message_completed(&mut self, _: u32) -> Result<()> {
        unreachable!()
    }
}

#[test]
fn skipped_kakera_needs_no_slot_and_notifications_preserve_read_state() {
    let mut banks = LayerBanks::default();
    for i in 0..32 {
        assert_eq!(banks.allocate(i), Some(i as u8));
    }
    let before = replay_snapshot(&banks);
    let skipped = Cmd::KAKERAGET {
        argument: i32::MIN,
        ids: [1, 2, 3].into_iter().collect(),
    };
    let mut owner = NotificationCommand::new(&skipped, &mut banks).unwrap();
    assert!(owner.skipped() && owner.target().is_none());
    assert_eq!(replay_snapshot(&banks), before);
    let mut transport = Transport {
        fail_prepare: true,
        fail_poll: true,
        ..Default::default()
    };
    assert!(
        owner
            .poll_live(&mut HigurashiLayers::new(|| [1, 2, 3, 4]), &mut transport)
            .unwrap()
    );
    assert!(!owner.callback_received() && owner.poll_replay() && !owner.forward_complete());
    assert!(NotificationCommand::new(&Cmd::OTSUGET { argument: 27 }, &mut banks).is_err());
    assert_eq!(replay_snapshot(&banks), before);
    let overflow = Cmd::KAKERAGET {
        argument: -1,
        ids: (0..17).collect(),
    };
    assert!(NotificationCommand::new(&overflow, &mut banks).is_err());
    assert_eq!(replay_snapshot(&banks), before);

    let mut reads = CommandReadState::new();
    let mut services = Reads::default();
    reads
        .prepare(
            &Cmd::MSGSET {
                msg_id: MessageId(1),
                auto_wait: 1,
                argument: 0,
                text: "retained text".into(),
            },
            &mut services,
        )
        .unwrap();
    services.checkpoints = 0;
    reads.timeline_mut().boundary = u32::MAX;
    reads.timeline_mut().resume_position = 0x1234;
    reads.timeline_mut().replay_target = 0x4567;
    for command in [Cmd::OTSUGET { argument: 27 }, skipped] {
        reads.prepare(&command, &mut services).unwrap();
        assert_eq!(reads.timeline().boundary, u32::MAX);
        assert_eq!(reads.timeline().resume_position, 0x1234);
        assert_eq!(reads.timeline().replay_target, 0x4567);
        assert!(reads.descriptors().message.text.is_some());
    }
    assert_eq!(services.checkpoints, 0);
}

#[test]
fn notification_desktop_service_fault_preserves_ready_callback_owner() {
    let mut layers = HigurashiLayers::new(|| [1, 2, 3, 4]);
    let mut owner =
        NotificationCommand::new(&Cmd::OTSUGET { argument: 27 }, &mut layers.state).unwrap();
    let mut transport = Transport {
        fail_prepare: true,
        ..Default::default()
    };
    assert!(owner.poll_live(&mut layers, &mut transport).is_err());
    assert!(!transport.requested && !owner.callback_received());
    transport.fail_prepare = false;
    transport.ready = true;
    transport.fail_poll = true;
    assert!(owner.poll_live(&mut layers, &mut transport).is_err());
    assert_eq!(transport.events, "FTWSU");
    transport.fail_poll = false;
    transport.callback = true;
    assert!(owner.poll_live(&mut layers, &mut transport).unwrap());
    assert!(owner.callback_received());
    assert_eq!(
        transport.events, "FTWSU",
        "Acknowledged publication is never repeated after callback fault"
    );
    assert!(owner.poll_live(&mut layers, &mut transport).unwrap());
}
