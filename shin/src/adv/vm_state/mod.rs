pub mod audio;
pub mod layers;

use layers::LayersState;
use shin_core::{
    format::save::{GameData, GameDataEntry, PersistData, RouteRecorder, SaveVectors, Settings},
    format::scenario::instruction_elements::MessageId,
    vm::command::types::MessageboxStyle,
};

use crate::adv::vm_state::audio::AudioState;

#[derive(Clone)]
pub struct SaveInfo {
    pub info: [String; 4],
}

impl SaveInfo {
    pub fn set_save_info(&mut self, level: i32, info: String) {
        assert!(
            (0..=4).contains(&level),
            "SaveInfo::set_save_info: level out of range"
        );

        self.info[level as usize] = info;
    }
}

#[derive(Debug, Clone)]
pub struct MessageState {
    pub message_id: MessageId,
    pub was_read: bool,
    pub received_syncs: u32,
    pub msginit: MessageboxStyle,
    pub messagebox_shown: bool,
    /// Native90e8: asynchronous MSGSET protects the message until MSGWAIT<0.
    pub close_inhibited: bool,
    pub text: Option<String>,
}

impl MessageState {
    pub fn new() -> Self {
        Self {
            message_id: MessageId(u32::MAX),
            was_read: true,
            received_syncs: 0,
            msginit: MessageboxStyle::default(),
            messagebox_shown: false,
            close_inhibited: false,
            text: None,
        }
    }
}

#[derive(Clone)]
pub struct VmState {
    pub settings: Settings,
    pub save_revision: u64,
    pub route: RouteRecorder,
    pub save_position: u32,
    previous_message_position: Option<u32>,
    current_message_position: Option<u32>,
    normal_message_count: u32,
    previous_chapter_position: u32,
    current_chapter_position: u32,
    chapter_message_count: u32,
    /// Native2dc8: the last message, chapter or no-text interactive boundary.
    pub(super) chapter_skip_position: u32,
    pub resume_position: Option<u32>,
    pub auto_save_slot: Option<GameData>,
    /// Native GameData at VM2530 retains entry zero when entry one is active.
    /// f720 copies it unchanged alongside the current route into each autosave.
    pub primary_entry: Option<GameDataEntry>,
    scenario_id: i32,
    random_seed: u32,
    pub save_info: SaveInfo,
    /// Native VM9110: chapter snapshot copied from SAVEINFO1 by AUTOSAVE.
    /// It remains unchanged by later SAVEINFO and is rebuilt during route replay.
    pub scene_title: String,
    pub messagebox_state: MessageState,
    pub persist: PersistData,
    pending_persistent_writes: Vec<(i32, i32)>,
    /// Disabled while reconstructing a saved route, like reference VM byte 0x9170.
    pub persistent_commits_enabled: bool,
    pub save_vectors: SaveVectors,
    pub layers: LayersState,
    pub audio: AudioState,
    pub system_controls_allowed: bool,
    pub current_event: Option<i32>,
    pub information: crate::adv::information::InformationState,
}

impl VmState {
    /// Native VM word 0x2db0 counts message/event/interactive boundaries.
    pub fn advance_save_position(&mut self) {
        self.save_position = self.save_position.wrapping_add(1);
    }

    pub fn record_message_boundary(&mut self) {
        self.previous_message_position = self.current_message_position;
        if self.current_event.is_none() {
            self.advance_save_position();
            self.current_message_position = Some(self.save_position);
            self.normal_message_count = self.normal_message_count.wrapping_add(1);
            self.chapter_message_count = self.chapter_message_count.wrapping_add(1);
            self.chapter_skip_position = self.save_position;
        }
    }

    pub fn record_chapter_boundary(&mut self) {
        self.advance_save_position();
        self.previous_chapter_position = self.current_chapter_position;
        self.current_chapter_position = self.save_position;
        self.chapter_skip_position = self.save_position;
        self.chapter_message_count = 0;
    }

    pub fn record_interactive_boundary(&mut self) {
        self.advance_save_position();
        self.chapter_skip_position = self.save_position;
    }

    /// Native182c0: after at most one message, return to the previous chapter;
    /// otherwise restart this chapter. Event text does not increase the count.
    pub fn previous_chapter_position(&self) -> u32 {
        if self.chapter_message_count > 1 {
            self.current_chapter_position
        } else {
            self.previous_chapter_position
        }
    }

    pub fn record_event_boundary(&mut self) {
        self.advance_save_position();
        self.previous_message_position = self.current_message_position;
    }

    /// Native137f0 requires two normal messages and selects2db4/2db8
    /// according to whether the message text pointer90f0 remains present.
    pub fn quick_return_position(&self) -> Option<u32> {
        if self.normal_message_count <= 1 {
            return None;
        }
        if self.messagebox_state.text.is_some() {
            self.previous_message_position
        } else {
            self.current_message_position
        }
    }

    pub fn snapshot_entry(&self) -> GameDataEntry {
        GameDataEntry {
            scenario_id: self.scenario_id,
            random_seed: self.random_seed,
            save_position: self.save_position,
            route_data: self.route.snapshot(),
        }
    }

    pub fn snapshot_game_data(&self) -> GameData {
        let mut game = GameData::from_entry(self.snapshot_entry());
        game.primary_entry = self.primary_entry.clone();
        game
    }

    pub fn resume_game_data(&self) -> GameData {
        let mut game = self.snapshot_game_data();
        game.entry.save_position = self.resume_position.unwrap_or(0);
        game
    }

    /// Native 7b5c/f210 uses the last RESUMESET boundary (zero initially),
    /// retaining the full route so replay can overwrite its abandoned suffix.
    pub fn resume_entry(&self) -> GameDataEntry {
        let mut entry = self.snapshot_entry();
        entry.save_position = self.resume_position.unwrap_or(0);
        entry
    }

    /// Explicit AUTOSAVE copies the chapter pointer; message/interactive snapshot
    /// captures use capture_autosave without changing this title.
    pub fn snapshot_scene_title(&mut self) {
        self.scene_title.clone_from(&self.save_info.info[1]);
    }

    pub fn capture_autosave(&mut self) {
        if self.persistent_commits_enabled {
            self.save_revision = self.save_revision.wrapping_add(1);
            self.auto_save_slot = Some(self.snapshot_game_data());
        }
    }

    pub fn for_scenario(scenario_id: i32, random_seed: u32) -> Self {
        Self {
            scenario_id,
            random_seed,
            ..Self::new()
        }
    }

    pub fn get_persistent(&self, slot: i32) -> i32 {
        self.pending_persistent_writes
            .iter()
            .find_map(|&(index, value)| (index == slot).then_some(value))
            .unwrap_or_else(|| self.persist.get(slot as u32 as usize))
    }

    pub fn set_persistent(&mut self, slot: i32, value: i32) {
        // Keep full signed 32-bit values until a script boundary commits them.
        // Updating an existing slot preserves the original insertion order.
        if let Some((_, pending)) = self
            .pending_persistent_writes
            .iter_mut()
            .find(|(index, _)| *index == slot)
        {
            *pending = value;
        } else {
            self.pending_persistent_writes.push((slot, value));
        }
    }

    pub fn commit_persistent(&mut self) {
        if !self.persistent_commits_enabled {
            return;
        }
        if !self.pending_persistent_writes.is_empty() {
            self.save_revision = self.save_revision.wrapping_add(1);
        }
        for (slot, value) in self.pending_persistent_writes.drain(..) {
            self.persist.set(slot as u32 as usize, value);
        }
    }

    pub fn can_skip_current_message(&self) -> bool {
        !self.settings.v11_disallowskipunread
            || self.current_event.is_some()
            || !self.messagebox_state.messagebox_shown
            || self.messagebox_state.was_read
    }

    pub fn mark_message_read(&mut self, id: MessageId) {
        if self.persistent_commits_enabled
            && (id.0 as i32) >= 0
            && self.save_vectors.seen_messages.add(id.0 as usize)
        {
            self.save_revision = self.save_revision.wrapping_add(1);
        }
    }

    pub fn allows_system_controls(&self) -> bool {
        self.system_controls_allowed && self.current_event.is_none()
    }

    pub fn allows_playback_mode_changes(&self) -> bool {
        // Native 0x11160..0x1121c enables L/R from SYS independently of EVBEGIN.
        // Execution helpers 0x1233c and 0x12474 still require no active event.
        self.system_controls_allowed
    }

    pub fn new() -> Self {
        Self {
            settings: Settings::default(),
            save_revision: 0,
            route: RouteRecorder::default(),
            save_position: 0,
            previous_message_position: None,
            current_message_position: None,
            normal_message_count: 0,
            previous_chapter_position: 0,
            current_chapter_position: 0,
            chapter_message_count: 0,
            chapter_skip_position: 0,
            resume_position: None,
            auto_save_slot: None,
            primary_entry: None,
            scenario_id: 0,
            random_seed: 0,
            save_info: SaveInfo {
                info: ["", "", "", ""].map(|v| v.to_string()),
            },
            scene_title: String::new(),
            messagebox_state: MessageState::new(),
            persist: PersistData::new(),
            pending_persistent_writes: Vec::new(),
            persistent_commits_enabled: true,
            save_vectors: SaveVectors::default(),
            layers: LayersState::new(),
            audio: AudioState::new(),
            system_controls_allowed: true,
            current_event: None,
            information: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VmState;
    use shin_core::{
        format::scenario::{
            instruction_elements::{MessageId, NumberSpec, U8Bool},
            types::U16FixupString,
        },
        vm::{IntoRuntimeForm, VmCtx, command},
    };

    #[test]
    fn scene_title_is_an_autosave_snapshot_even_when_persistent_writes_are_disabled() {
        let mut state = VmState::new();
        assert!(state.scene_title.is_empty());
        state.save_info.set_save_info(1, "Chapter A".into());
        assert!(state.scene_title.is_empty());
        state.snapshot_scene_title();
        state.capture_autosave();
        assert_eq!(state.scene_title, "Chapter A");
        state.save_info.set_save_info(1, "Chapter B".into());
        state.capture_autosave(); // MSGSET and interactive autosave captures.
        assert_eq!(state.scene_title, "Chapter A");
        let revision = state.save_revision;
        let slot = state.auto_save_slot.clone();
        state.persistent_commits_enabled = false;
        state.snapshot_scene_title();
        state.capture_autosave();
        assert_eq!(state.scene_title, "Chapter B");
        assert_eq!(state.save_revision, revision);
        assert_eq!(format!("{:?}", state.auto_save_slot), format!("{slot:?}"));
        state.save_info.set_save_info(1, String::new());
        state.snapshot_scene_title();
        state.capture_autosave();
        assert!(state.scene_title.is_empty());
    }
    #[test]
    fn quick_return_tracks_messages_across_autosaves_events_and_closed_text() {
        let mut state = VmState::new();
        let ctx = VmCtx::new(0, 42);
        let apply = |cmd: command::CompiletimeCommand, state: &mut VmState| {
            crate::adv::command::apply_command_state(cmd.into_runtime_form(&ctx), state);
        };
        let message = || {
            command::CompiletimeCommand::MSGSET(command::compiletime::MSGSET {
                msg_id: MessageId(1),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("test"),
            })
        };
        assert_eq!(state.quick_return_position(), None);
        apply(message(), &mut state);
        assert_eq!(state.quick_return_position(), None);
        apply(
            command::CompiletimeCommand::AUTOSAVE(command::compiletime::AUTOSAVE {}),
            &mut state,
        );
        assert_eq!(
            state.quick_return_position(),
            None,
            "boundaries alone do not qualify as two messages"
        );
        apply(message(), &mut state);
        assert_eq!(state.save_position, 3);
        assert_eq!(state.quick_return_position(), Some(1));
        apply(
            command::CompiletimeCommand::MSGCLOSE(command::compiletime::MSGCLOSE {
                wait_for_close: U8Bool(false),
            }),
            &mut state,
        );
        assert_eq!(state.quick_return_position(), Some(3));
        apply(
            command::CompiletimeCommand::EVBEGIN(command::compiletime::EVBEGIN {
                arg: NumberSpec::constant(2),
            }),
            &mut state,
        );
        apply(message(), &mut state);
        assert_eq!(state.save_position, 4);
        assert_eq!(state.quick_return_position(), Some(3));
        apply(
            command::CompiletimeCommand::EVEND(command::compiletime::EVEND {}),
            &mut state,
        );
        assert_eq!(state.quick_return_position(), Some(3));
        apply(message(), &mut state);
        assert_eq!(state.save_position, 6);
        assert_eq!(state.quick_return_position(), Some(3));
        apply(
            command::CompiletimeCommand::MSGCLOSE(command::compiletime::MSGCLOSE {
                wait_for_close: U8Bool(false),
            }),
            &mut state,
        );
        assert_eq!(state.quick_return_position(), Some(6));
    }

    #[test]
    fn event_blocks_execution_controls_but_preserves_mode_selection() {
        let mut state = VmState::new();
        let ctx = VmCtx::new(0, 42);
        let apply = |cmd: command::CompiletimeCommand, state: &mut VmState| {
            crate::adv::command::apply_command_state(cmd.into_runtime_form(&ctx), state);
        };
        let sys = |argument| {
            command::CompiletimeCommand::SYSCALL(command::compiletime::SYSCALL {
                call_id: NumberSpec::constant(0),
                argument: NumberSpec::constant(argument),
            })
        };
        apply(sys(1), &mut state);
        assert!(state.allows_playback_mode_changes());
        assert!(state.allows_system_controls());
        apply(
            command::CompiletimeCommand::EVBEGIN(command::compiletime::EVBEGIN {
                arg: NumberSpec::constant(24),
            }),
            &mut state,
        );
        assert_eq!(state.current_event, Some(24));
        assert!(state.allows_playback_mode_changes());
        assert!(!state.allows_system_controls());
        apply(sys(0), &mut state);
        assert!(!state.allows_playback_mode_changes());
        assert!(!state.allows_system_controls());
        apply(
            command::CompiletimeCommand::EVEND(command::compiletime::EVEND {}),
            &mut state,
        );
        assert!(!state.allows_playback_mode_changes());
        apply(sys(1), &mut state);
        assert!(state.allows_system_controls());
    }

    #[test]
    fn unread_message_skip_gate_uses_history_at_message_entry() {
        let mut state = VmState::new();
        let ctx = VmCtx::new(0, 42);
        let message = || {
            command::CompiletimeCommand::MSGSET(command::compiletime::MSGSET {
                msg_id: MessageId(32),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("@rUnread"),
            })
            .into_runtime_form(&ctx)
        };
        assert!(state.can_skip_current_message());
        crate::adv::command::apply_command_state(message(), &mut state);
        assert!(!state.can_skip_current_message());
        assert!(!state.save_vectors.seen_messages.contains(32));
        let revision = state.save_revision;
        state.mark_message_read(MessageId(32));
        assert!(state.save_vectors.seen_messages.contains(32));
        assert_eq!(state.save_revision, revision + 1);
        // The finishing callback does not change this message's cached entry flag.
        assert!(!state.can_skip_current_message());
        state.mark_message_read(MessageId(32));
        assert_eq!(state.save_revision, revision + 1);
        crate::adv::command::apply_command_state(message(), &mut state);
        assert!(state.can_skip_current_message());
        let saved = shin_core::format::save::SaveData {
            save_vectors: state.save_vectors.clone(),
            ..Default::default()
        };
        assert!(
            shin_core::format::save::SaveData::decode(&saved.encode())
                .unwrap()
                .save_vectors
                .seen_messages
                .contains(32)
        );
    }

    #[test]
    fn unread_setting_event_and_reconstruction_match_native_guards() {
        let mut state = VmState::new();
        state.messagebox_state.messagebox_shown = true;
        state.messagebox_state.was_read = false;
        assert!(!state.can_skip_current_message());
        state.settings.v11_disallowskipunread = false;
        assert!(state.can_skip_current_message());
        state.settings.v11_disallowskipunread = true;
        state.current_event = Some(1);
        assert!(state.can_skip_current_message());
        state.mark_message_read(MessageId(u32::MAX));
        assert_eq!(state.save_vectors.seen_messages.size_in_words(), 0);
        state.persistent_commits_enabled = false;
        state.mark_message_read(MessageId(1));
        assert_eq!(state.save_vectors.seen_messages.size_in_words(), 0);
        assert_eq!(state.save_revision, 0);
    }

    #[test]
    fn message_entry_captures_autosave_with_historical_route_and_committed_globals() {
        let mut state = VmState::for_scenario(7, 42);
        state.set_persistent(0, 65535);
        state.route.record_persistent(-14);
        let cmd = command::CompiletimeCommand::MSGSET(command::compiletime::MSGSET {
            msg_id: MessageId(1),
            auto_wait: U8Bool(false),
            text: U16FixupString::new("@rAsync"),
        })
        .into_runtime_form(&VmCtx::new(7, 42));
        crate::adv::command::apply_command_state(cmd, &mut state);
        let saved = state.auto_save_slot.as_ref().unwrap();
        assert_eq!(saved.entry.save_position, 1);
        assert_eq!(
            saved.entry.route_data.reader().read_persistent().unwrap(),
            -14
        );
        assert_eq!(state.persist.get(0), -1);
        assert!(!state.save_vectors.seen_messages.contains(1));
    }

    #[test]
    fn autosave_records_native_boundaries_and_survives_save_encoding() {
        use shin_core::format::save::SaveData;
        let mut state = VmState::for_scenario(7, 42);
        let ctx = VmCtx::new(7, 42);
        let apply = |cmd: command::CompiletimeCommand, state: &mut VmState| {
            crate::adv::command::apply_command_state(cmd.into_runtime_form(&ctx), state);
        };
        let message = || {
            command::CompiletimeCommand::MSGSET(command::compiletime::MSGSET {
                msg_id: MessageId(1),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("test"),
            })
        };
        apply(message(), &mut state);
        assert_eq!(state.save_position, 1);
        apply(
            command::CompiletimeCommand::EVBEGIN(command::compiletime::EVBEGIN {
                arg: NumberSpec::constant(3),
            }),
            &mut state,
        );
        apply(message(), &mut state);
        assert_eq!(state.save_position, 2); // Event text shares its boundary.
        apply(
            command::CompiletimeCommand::EVEND(command::compiletime::EVEND {}),
            &mut state,
        );
        apply(
            command::CompiletimeCommand::RESUMESET(command::compiletime::RESUMESET {}),
            &mut state,
        );
        assert_eq!(state.resume_position, Some(4));
        state.set_persistent(0, 65535);
        apply(command::CompiletimeCommand::SGET(command::compiletime::SGET {
            dest: shin_core::format::scenario::instruction_elements::Register::from_regular_register(1),
            slot_number: NumberSpec::constant(0),
        }), &mut state);
        assert_eq!(state.save_position, 4); // SGET records data without a boundary.
        apply(
            command::CompiletimeCommand::AUTOSAVE(command::compiletime::AUTOSAVE {}),
            &mut state,
        );
        let saved = state.auto_save_slot.as_ref().unwrap();
        assert_eq!(saved.entry.scenario_id, 7);
        assert_eq!(saved.entry.random_seed, 42);
        assert_eq!(saved.entry.save_position, 5);
        assert_eq!(
            saved.entry.route_data.reader().read_persistent().unwrap(),
            -1
        );
        assert_eq!(state.persist.get(0), -1);
        let save = SaveData {
            persist_data: state.persist.clone(),
            save_vectors: state.save_vectors.clone(),
            auto_save_slot: state.auto_save_slot.clone(),
            ..SaveData::default()
        };
        let decoded = SaveData::decode(&save.encode()).unwrap();
        let entry = decoded.auto_save_slot.unwrap().entry;
        assert_eq!(
            (entry.scenario_id, entry.random_seed, entry.save_position),
            (7, 42, 5)
        );
        assert_eq!(entry.route_data.reader().read_persistent().unwrap(), -1);
        state.route.record_quiz(27);
        assert_eq!(saved_route_bits(&state), 17); // Existing save owns its route.
        state.persistent_commits_enabled = false;
        state.capture_autosave();
        assert_eq!(saved_route_bits(&state), 17);
    }

    fn saved_route_bits(state: &VmState) -> usize {
        let mut reader = state
            .auto_save_slot
            .as_ref()
            .unwrap()
            .entry
            .route_data
            .reader();
        reader.read_persistent().unwrap();
        let position = reader.bit_position();
        assert_eq!(reader.read_quiz().unwrap(), 0); // Padding, not the later response.
        position
    }

    #[test]
    fn messages_commit_between_events_and_events_commit_at_their_boundaries() {
        let mut state = VmState::new();
        let ctx = VmCtx::new(0, 0);
        let apply = |cmd: command::CompiletimeCommand, state: &mut VmState| {
            crate::adv::command::apply_command_state(cmd.into_runtime_form(&ctx), state);
        };
        let message = || {
            command::CompiletimeCommand::MSGSET(command::compiletime::MSGSET {
                msg_id: MessageId(1),
                auto_wait: U8Bool(true),
                text: U16FixupString::new("test"),
            })
        };
        state.set_persistent(0, 65535);
        apply(message(), &mut state);
        assert_eq!(state.get_persistent(0), -1);
        state.set_persistent(0, 65536);
        apply(
            command::CompiletimeCommand::EVBEGIN(command::compiletime::EVBEGIN {
                arg: NumberSpec::constant(3),
            }),
            &mut state,
        );
        assert_eq!(state.get_persistent(0), 0);
        state.set_persistent(0, 32768);
        apply(message(), &mut state);
        assert_eq!(state.get_persistent(0), 32768);
        assert_eq!(state.persist.get(0), 0);
        apply(
            command::CompiletimeCommand::EVEND(command::compiletime::EVEND {}),
            &mut state,
        );
        assert_eq!(state.get_persistent(0), -32768);
        state.set_persistent(0, 65535);
        apply(
            command::CompiletimeCommand::AUTOSAVE(command::compiletime::AUTOSAVE {}),
            &mut state,
        );
        assert_eq!(state.get_persistent(0), -1);
    }

    #[test]
    fn persistent_reads_use_full_pending_values_until_commit() {
        let mut state = VmState::new();
        state.persist.set(0, 7);
        state.set_persistent(0, 32768);
        state.set_persistent(64, -32769);
        state.set_persistent(0, 65535);
        assert_eq!(state.pending_persistent_writes, [(0, 65535), (64, -32769)]);
        assert_eq!(state.get_persistent(0), 65535);
        assert_eq!(state.get_persistent(64), -32769);
        assert_eq!(state.persist.get(0), 7);
        assert_eq!(state.get_persistent(-1), 0);
        assert_eq!(state.get_persistent(1000), 0);
        state.commit_persistent();
        assert_eq!(state.get_persistent(0), -1);
        assert_eq!(state.get_persistent(64), 32767);
        assert!(state.pending_persistent_writes.is_empty());
        state.commit_persistent();
        assert_eq!(state.get_persistent(0), -1);
    }

    #[test]
    fn route_reconstruction_keeps_pending_values_without_changing_globals() {
        let mut state = VmState::new();
        state.persist.set(0, 19);
        state.persistent_commits_enabled = false;
        state.set_persistent(0, i32::MAX);
        state.commit_persistent();
        assert_eq!(state.get_persistent(0), i32::MAX);
        assert_eq!(state.persist.get(0), 19);
        assert_eq!(state.pending_persistent_writes, [(0, i32::MAX)]);
        state.persistent_commits_enabled = true;
        state.commit_persistent();
        assert_eq!(state.get_persistent(0), -1);
    }
}
