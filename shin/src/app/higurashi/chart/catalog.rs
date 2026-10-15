//! Updated Scenario babd8/bbd1c..bbf6c typed Chart records.
//!
//! The native count word reserves capacity; record traversal ends at the table's
//! declared byte boundary. Unknown kinds leave stale native fields and therefore
//! remain an explicit unsupported adapter boundary. The authored updated table
//! has only kinds0..5 and consumes exactly18,760 bytes into1,449 records.
use anyhow::{Context, Result, ensure};
use bytes::Bytes;
use shin_core::format::scenario::higurashi::HigurashiScenario;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Label {
    /// Native record+a, authored values0..6. Interpretation belongs to Chart.
    pub extra: u16,
    /// All source bytes in the native u8-sized string, including its terminator.
    pub encoded: Bytes,
    /// Offset of the first string byte within the Chart record payload.
    pub source_offset: usize,
}
impl Label {
    /// Actual Chart486bc/4afb4 consume the source through strlen/zero termination.
    pub fn c_string_bytes(&self) -> &[u8] {
        let end = self
            .encoded
            .iter()
            .position(|&byte| byte == 0)
            .expect("Checked Chart label terminator");
        &self.encoded[..end]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Record {
    pub kind: u16,
    pub x: i16,
    pub y: i16,
    /// Source fourth word, retained at native record+8. Chart uses its signed
    /// form at selected-node publication; this adapter preserves all16 bits.
    pub value: u16,
    pub label: Option<Label>,
}

#[derive(Clone, Debug)]
pub(crate) struct Catalog {
    #[allow(dead_code, reason = "Preserve the native count word independently of decoded record length.")]
    pub declared_count: u32,
    pub records: Vec<Record>,
}
impl Catalog {
    pub fn from_scenario(scenario: &HigurashiScenario) -> Result<Self> {
        Self::decode(scenario.chart_data.clone(), scenario.chart_record_count)
    }

    pub fn decode(payload: Bytes, declared_count: u32) -> Result<Self> {
        let mut at = 0usize;
        let mut records = Vec::with_capacity((declared_count as usize).min(payload.len() / 8));
        while at < payload.len() {
            let record_start = at;
            let kind = word(&payload, &mut at)?;
            let x = word(&payload, &mut at)? as i16;
            let y = word(&payload, &mut at)? as i16;
            ensure!(
                kind <= 5,
                "Unsupported older Chart kind{kind} at payload byte{record_start}"
            );
            let value = word(&payload, &mut at)?;
            let label = if kind == 1 {
                let extra = word(&payload, &mut at)?;
                let size = usize::from(*payload.get(at).context("Truncated Chart label length")?);
                at += 1;
                let end = at.checked_add(size).context("Chart label size overflow")?;
                ensure!(end <= payload.len(), "Truncated Chart label bytes");
                ensure!(size != 0, "Chart label has no checked terminator");
                ensure!(
                    payload[end - 1] == 0,
                    "Chart label lacks its source terminator"
                );
                let label = Label {
                    extra,
                    encoded: payload.slice(at..end),
                    source_offset: at,
                };
                at = end;
                Some(label)
            } else {
                None
            };
            records.push(Record {
                kind,
                x,
                y,
                value,
                label,
            });
        }
        Ok(Self {
            declared_count,
            records,
        })
    }

    /// Actual bc7bc rejects negative indices and compares against vector length.
    #[cfg(test)]
    pub fn get(&self, index: i32) -> Option<&Record> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.records.get(index))
    }
}

fn word(payload: &[u8], at: &mut usize) -> Result<u16> {
    let end = at.checked_add(2).context("Chart word offset overflow")?;
    let bytes = payload
        .get(*at..end)
        .context("Truncated Chart record word")?;
    *at = end;
    Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
}

#[cfg(test)]
mod tests;
