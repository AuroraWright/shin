//! Actual dcf00/dcf30 arguments and15c64 completion/continuation selection.
use super::{Model, RuntimeGameData};
use anyhow::{Result, ensure};
use shin_core::format::{
    save::higurashi::SaveVectors,
    scenario::higurashi::{HigurashiScenario, HigurashiVariant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app::higurashi) struct Request {
    pub data: Option<RuntimeGameData>,
    pub selected: i32,
    /// dcf00's bit0;dcf30 supplies0 and the complete data chooses its variant.
    pub alternate: u32,
}
impl Request {
    pub fn preflight(&self, scenario: &HigurashiScenario, vectors: &SaveVectors) -> Result<()> {
        Model::new(
            scenario,
            self.data.clone(),
            self.selected,
            self.alternate,
            vectors,
        )?;
        Ok(())
    }
}
pub(in crate::app::higurashi) struct ExitReturn {
    pub request: Request,
    pub save: bool,
}
/// The committed current data is supplied after EXIT's real close/commit gates.
/// A matching scenario marks only its odd completion bit. Unknown IDs select0.
pub(in crate::app::higurashi) fn from_exit(
    data: &RuntimeGameData,
    scenario: &HigurashiScenario,
    vectors: &mut SaveVectors,
    unlock: u32,
) -> Result<ExitReturn> {
    ensure!(
        scenario.variant == HigurashiVariant::Updated,
        "TIPS return requires updated Higurashi"
    );
    let id = data.progress.active_entry().scenario_id;
    let found = scenario
        .tips_info
        .iter()
        .position(|tip| tip.updated_field.map(i32::from) == Some(id));
    let selected = found.unwrap_or(0) as i32;
    if let Some(index) = found.filter(|_| unlock & 1 != 0) {
        let bit = 2 * index + 1;
        let word = bit / 32;
        if word >= vectors.extra_words_0.len() {
            vectors
                .extra_words_0
                .resize((word + 1).next_multiple_of(256), 0);
        }
        vectors.extra_words_0[word] |= 1 << (bit % 32);
    }
    let mut copy = data.clone();
    let save = copy.progress.active_index() == 0;
    let request = if save {
        Request {
            data: None,
            selected,
            alternate: u32::from(id > 9999),
        }
    } else {
        copy.progress.set_active_index(0)?;
        Request {
            data: Some(copy),
            selected,
            alternate: 0,
        }
    };
    Ok(ExitReturn { request, save })
}

#[cfg(test)]
mod tests;
