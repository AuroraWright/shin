//! MessageBrowser7db60's private LogBuilder76684/7dcf0..7ecd8.
//! Replays from scenario start, then extends the recorded route with current
//! settings/remembered choices. It never constructs ADV effects or commits saves.
use super::{
    autosave::SaveProgress, choice::ChoiceHistory, progress::ProgressMemory,
    read_history::MessageReadHistory, variables::HigurashiVariables,
};
use anyhow::{Context, Result, ensure};
use shin_core::{
    format::scenario::{
        higurashi::{HigurashiScenario, HigurashiVariant},
        instruction_elements::CodeAddress,
    },
    vm::{
        command::CommandResult,
        higurashi::{HigurashiScripter, command::HigurashiRuntimeCommand as Cmd},
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Message {
        text: String,
        style: i32,
        argument: Option<i32>,
    },
    Choice {
        title: String,
        text: String,
    },
    Checkpoint {
        title: String,
        text: String,
    },
    CharacterChoice {
        response: i32,
    },
}
impl Item {
    /// Original76a38 markup, before the backlog parser/layout consumes it.
    pub fn display_text(&self) -> Result<String> {
        Ok(match self {
            Self::Message { text, .. } => text.clone(),
            Self::Choice { title, text } => format!("@r《{title}》@r{text}"),
            Self::Checkpoint { title, text } => {
                if text.is_empty() {
                    format!("@r{title}")
                } else {
                    format!("@r{title} {text}")
                }
            }
            Self::CharacterChoice { response } => {
                let name = ["レナ", "魅音", "沙都子", "夏美"]
                    .get(*response as usize)
                    .context("Browser character response exceeds native four-name table")?;
                format!("@r《ヒロイン選択》@r{name}")
            }
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub boundary: u32,
    pub index: u32,
    pub item: Item,
}

pub struct BrowserHistory {
    pub rows: Vec<Row>,
    /// Original indices of checkpoint/choice rows, used for chapter navigation.
    pub markers: Vec<u32>,
    pub current_row: Option<usize>,
    /// Worker75b28 anchors measurement before clamping the selected row. At
    /// the history end this can equal rows.len(), placing the last row below0.
    pub measurement_anchor: u32,
    /// Private extended GameData responses/byte variables for row reconstruction.
    pub jump_source: ProgressMemory,
}

pub struct LogBuilder {
    progress: SaveProgress,
    variables: HigurashiVariables,
    pub rows: Vec<Row>,
    pub markers: Vec<u32>,
    pub boundary: u32,
    pub row_count: u32,
    pub current_row: u32,
    pub style: i32,
    pub event: i32,
    pub layer_back: bool,
    message_present: bool,
    labels: [String; 2],
    done: bool,
}
impl LogBuilder {
    pub fn new(source: &ProgressMemory) -> Result<Self> {
        let progress = source.progress().clone();
        let done = progress.active_entry().save_position == 0;
        let mut variables =
            HigurashiVariables::new(source.persistent().clone(), source.bytes().to_vec());
        variables.restore_route(progress.active_entry().response_data.clone(), 0)?;
        Ok(Self {
            progress,
            variables,
            rows: Vec::new(),
            markers: Vec::new(),
            boundary: 0,
            row_count: 0,
            current_row: 0,
            style: 0,
            event: -1,
            layer_back: false,
            message_present: false,
            labels: ["?".into(), "?".into()],
            done,
        })
    }
    pub fn done(&self) -> bool {
        self.done
    }
    pub fn route(&self) -> (&[u8], usize) {
        self.variables.route()
    }
    pub fn private_persistent(&self) -> &shin_core::format::save::PersistData {
        self.variables.persistent()
    }
    pub fn private_bytes(&self) -> &[u8] {
        self.variables.bytes()
    }
    fn historical(&self) -> bool {
        self.boundary < self.progress.active_entry().save_position
    }
    fn before_row(&mut self) {
        if self.historical() {
            self.current_row = self.row_count;
        }
    }
    fn append(&mut self, item: Item, marker: bool) {
        if marker {
            self.markers.push(self.row_count);
        }
        self.rows.push(Row {
            boundary: self.boundary,
            index: self.row_count,
            item,
        });
        self.row_count = self.row_count.wrapping_add(1);
    }
    fn remembered(
        &mut self,
        index: i32,
        visibility: i32,
        history: &ChoiceHistory,
    ) -> Result<Option<i32>> {
        if self.historical() {
            Ok(Some(self.variables.response_choice(true, 0)?))
        } else {
            let last = usize::try_from(index)
                .ok()
                .and_then(|i| history.last().get(i))
                .copied()
                .unwrap_or(0);
            let value = i32::from(last) - 1;
            if last == 0 || (visibility as u32).wrapping_shr(value as u32) & 1 == 0 {
                self.done = true;
                return Ok(None);
            }
            // Native347d0 writes low4 but publishes the full remembered byte-1.
            self.variables.response_choice(false, value)?;
            Ok(Some(value))
        }
    }
    /// Execute the original listener contract. The scripter owns arithmetic,
    /// branches and feedback; this listener owns only private history state.
    pub fn command(
        &mut self,
        command: &Cmd,
        history: &MessageReadHistory,
        choices: &ChoiceHistory,
        read_only: bool,
    ) -> Result<CommandResult> {
        ensure!(!self.done, "MessageBrowser builder already stopped");
        let mut result = CommandResult::None;
        match command {
            Cmd::EXIT { arg1: 0, .. } => self.done = true,
            Cmd::SGET { dest, slot }
            | Cmd::OpcodeDA {
                dest,
                argument: slot,
            } => {
                let byte = matches!(command, Cmd::OpcodeDA { .. });
                let value = if self.historical() {
                    self.variables.response(byte, true, 0)?
                } else {
                    let value = if byte {
                        self.variables
                            .bytes()
                            .get(*slot as u32 as usize)
                            .copied()
                            .unwrap_or(0) as i32
                    } else {
                        self.variables.persistent().get(*slot as u32 as usize)
                    };
                    //7ec88 intentionally uses34764 (integer), even for DA's future byte lookup.
                    self.variables.response(false, false, value)?;
                    value
                };
                result = CommandResult::WriteMemory(*dest, value);
            }
            Cmd::SSET { .. } | Cmd::OpcodeDB { .. } if !self.historical() => {
                self.variables
                    .dispatch(command, super::variables::Execution::Live)?;
                self.variables.commit_pending(true)?;
            }
            Cmd::MSGINIT { style, .. } => self.style = *style,
            Cmd::MSGSET {
                msg_id,
                argument,
                text,
                ..
            } => {
                if self.event < 0 {
                    self.boundary = self.boundary.wrapping_add(1);
                }
                self.before_row();
                self.message_present = true;
                let id = super::read_state::native_message_id(msg_id.0);
                if !self.historical() && read_only && (id as i32) >= 0 && !history.contains(id) {
                    self.done = true;
                } else {
                    self.append(
                        Item::Message {
                            text: text.clone(),
                            style: self.style,
                            argument: Some(if *argument < 0 { 9 } else { *argument }),
                        },
                        false,
                    );
                }
            }
            Cmd::MSGCLOSE { .. } => self.message_present = false,
            Cmd::LOGSET { text } => {
                self.before_row();
                //7e008 does not initialize row+18; keep that field unspecified.
                self.append(
                    Item::Message {
                        text: text.clone(),
                        style: self.style,
                        argument: None,
                    },
                    false,
                );
            }
            Cmd::SELECT {
                index,
                dest,
                visibility,
                title,
                variants,
                ..
            } => {
                if !self.message_present {
                    self.boundary = self.boundary.wrapping_add(1);
                }
                if let Some(value) = self.remembered(i32::from(*index) - 1, *visibility, choices)? {
                    self.before_row();
                    let text = variants
                        .get(value as usize)
                        .context("Browser SELECT response exceeds supplied variants")?;
                    let text = if let Some(suffix) = text.strip_prefix('*') {
                        suffix.split_once('.').map_or("", |(_, tail)| tail)
                    } else {
                        text
                    };
                    self.append(
                        Item::Choice {
                            title: title.clone(),
                            text: text.into(),
                        },
                        true,
                    );
                    self.boundary = self.boundary.wrapping_add(1);
                    result = CommandResult::WriteMemory(*dest, value);
                }
            }
            Cmd::SAVEINFO { level, info } => {
                let slot = usize::try_from(*level)
                    .ok()
                    .filter(|&i| i < 2)
                    .context("Browser SAVEINFO level exceeds two labels")?;
                self.labels[slot] = info.clone();
            }
            Cmd::AUTOSAVE {} => {
                self.boundary = self.boundary.wrapping_add(1);
                self.before_row();
                self.append(
                    Item::Checkpoint {
                        title: self.labels[0].clone(),
                        text: self.labels[1].clone(),
                    },
                    true,
                );
            }
            Cmd::EVBEGIN { argument } => {
                self.boundary = self.boundary.wrapping_add(1);
                self.event = *argument;
            }
            Cmd::EVEND {} => {
                self.boundary = self.boundary.wrapping_add(1);
                self.event = -1;
            }
            Cmd::WIPE { .. } => self.layer_back = false,
            Cmd::LAYERBACK { .. } => self.layer_back = true,
            Cmd::TIPSGET { .. } => self.boundary = self.boundary.wrapping_add(2),
            Cmd::CHARSELECT {
                index,
                dest,
                argument,
            } => {
                self.boundary = self.boundary.wrapping_add(1);
                if let Some(value) = self.remembered(i32::from(*index) - 1, *argument, choices)? {
                    self.before_row();
                    self.append(Item::CharacterChoice { response: value }, true);
                    self.boundary = self.boundary.wrapping_add(1);
                    result = CommandResult::WriteMemory(*dest, value);
                }
            }
            Cmd::SNRSEL { .. } | Cmd::KAKERA {} | Cmd::QUIZ { .. } => {
                self.boundary = self.boundary.wrapping_add(1);
                if !self.historical() {
                    self.done = true;
                } else if let Cmd::QUIZ { dest, .. } = command {
                    result =
                        CommandResult::WriteMemory(*dest, self.variables.response_choice(true, 0)?);
                }
            }
            _ => {}
        }
        Ok(result)
    }
    pub fn finish(mut self, source: &ProgressMemory) -> BrowserHistory {
        let active = self.progress.active_index();
        self.progress.entries[active].response_data = self.variables.route().0.to_vec();
        let jump_source = ProgressMemory::new(
            source.persistent().clone(),
            self.progress,
            self.variables.bytes().to_vec(),
        ).with_trailing_bytes(source.trailing_bytes().to_vec())
            .with_chart_unlocks(source.chart_unlocks.clone());
        let current_row =
            (!self.rows.is_empty()).then(|| (self.current_row as usize).min(self.rows.len() - 1));
        BrowserHistory {
            measurement_anchor: self.current_row,
            rows: self.rows,
            markers: self.markers,
            current_row,
            jump_source,
        }
    }
}

fn browser_text(
    scenario: &HigurashiScenario,
    position: CodeAddress,
    command: &mut Cmd,
) -> Result<()> {
    use shin_core::format::scenario::higurashi::browser_text::BrowserText;
    match (scenario.browser_text(position)?, command) {
        (Some(BrowserText::Message(source)), Cmd::MSGSET { text, .. } | Cmd::LOGSET { text }) => {
            *text = source
        }
        (
            Some(BrowserText::Choice {
                title: source_title,
                variants: source_variants,
            }),
            Cmd::SELECT {
                title, variants, ..
            },
        ) => {
            *title = source_title;
            *variants = source_variants;
        }
        _ => {}
    }
    Ok(())
}
impl BrowserHistory {
    fn run(
        scenario: &HigurashiScenario,
        source: &ProgressMemory,
        history: &MessageReadHistory,
        choices: &ChoiceHistory,
        read_only: bool,
        command_limit: usize,
        probe: bool,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<LogBuilder> {
        ensure!(
            scenario.variant == HigurashiVariant::Updated,
            "Browser requires updated Higurashi"
        );
        let active = source.progress().active_entry();
        let mut builder = LogBuilder::new(source)?;
        let mut vm = HigurashiScripter::new(scenario, active.scenario_id, active.random_seed)?;
        let mut result = CommandResult::None;
        for _ in 0..command_limit {
            ensure!(!cancelled(), "Browser history cancelled");
            if builder.done() || (probe && builder.row_count > 0) {
                return Ok(builder);
            }
            let mut command = vm
                .run(result, 100_000)
                .context("Executing Higurashi browser history")?;
            browser_text(scenario, vm.position(), &mut command)?;
            result = builder.command(&command, history, choices, read_only)?;
        }
        ensure!(
            builder.done() || (probe && builder.row_count > 0),
            "Browser command budget exhausted"
        );
        Ok(builder)
    }
    pub fn collect(
        scenario: &HigurashiScenario,
        source: &ProgressMemory,
        history: &MessageReadHistory,
        choices: &ChoiceHistory,
        read_only: bool,
        command_limit: usize,
    ) -> Result<Self> {
        Ok(Self::run(
            scenario,
            source,
            history,
            choices,
            read_only,
            command_limit,
            false,
            || false,
        )?
        .finish(source))
    }
    /// The native history worker checks its active byte between commands.
    /// Cancellation leaves the supplied progress/settings snapshot untouched.
    pub fn collect_cancellable(
        scenario: &HigurashiScenario,
        source: &ProgressMemory,
        history: &MessageReadHistory,
        choices: &ChoiceHistory,
        read_only: bool,
        command_limit: usize,
        cancelled: impl FnMut() -> bool,
    ) -> Result<Self> {
        Ok(Self::run(
            scenario,
            source,
            history,
            choices,
            read_only,
            command_limit,
            false,
            cancelled,
        )?
        .finish(source))
    }
    /// Native76448 stops as soon as the first eligible row is available.
    pub fn available(
        scenario: &HigurashiScenario,
        source: &ProgressMemory,
        history: &MessageReadHistory,
        choices: &ChoiceHistory,
        read_only: bool,
        command_limit: usize,
    ) -> Result<bool> {
        let builder = Self::run(
            scenario,
            source,
            history,
            choices,
            read_only,
            command_limit,
            true,
            || false,
        )?;
        Ok(!builder.done() && builder.row_count > 0)
    }
}

#[cfg(test)]
mod tests;
