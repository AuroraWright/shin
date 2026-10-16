//! Original LogBuilder row protocol, independent of VM, font and IO services.
#[path = "measurement.rs"]
pub mod measurement;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Message {
        message_id: u32,
        message_arg: i32,
        text: String,
    },
    Choice {
        title: String,
        text: String,
        response: i32,
    },
    Checkpoint {
        title: String,
        text: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub boundary: u32,
    pub item: Item,
}
impl Entry {
    pub fn kind(&self) -> u8 {
        match self.item {
            Item::Message { .. } => 0,
            Item::Choice { .. } => 1,
            Item::Checkpoint { .. } => 2,
        }
    }
    pub fn message_arg(&self) -> i32 {
        match self.item {
            Item::Message { message_arg, .. } => message_arg,
            _ => 0,
        }
    }
    /// Original80958. These strings are markup passed to LogMessage, not plain
    /// labels; chosen options strip their leading authored '*... .' prefix.
    pub fn display_text(&self) -> String {
        match &self.item {
            Item::Message { text, .. } => text.clone(),
            Item::Choice { title, text, .. } => format!("@r《{title}》@r{text}"),
            Item::Checkpoint { title, text } => format!(
                "@r{title}{}{text}",
                if title.is_empty() { "" } else { "　" }
            ),
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct GeometryRow {
    pub marker: bool,
    pub voice_count: u8,
    pub top: f32,
    pub height: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    pub rows: Vec<GeometryRow>,
    pub selected: usize,
    pub allowed: std::ops::Range<usize>,
    pub initial_scroll: f32,
    pub minimum: f32,
    pub maximum: f32,
    pub bottom: f32,
}
/// Original7FC00 measures current row at0, then alternates preceding/following
/// rows. Keep that floating point origin rather than shifting the whole log.
pub fn geometry(
    entries: &[Entry],
    current: Option<usize>,
    measured: &[(f32, u8)],
) -> Result<Option<Geometry>, &'static str> {
    if measured.len() != entries.len() || measured.iter().any(|(h, _)| !h.is_finite() || *h < 0.) {
        return Err("Ger backlog measurement disagrees with history");
    }
    let Some(mut measurement) = measurement::Measurement::new(entries, current) else {
        return Ok(None);
    };
    while let Some(row) = measurement.next_row() {
        let (height, voices) = measured[row];
        measurement.publish(row, height, voices)?;
    }
    Ok(Some(measurement.geometry()))
}

/// Callback state at80530/86F3C..87C88. The VM adapter owns numeric execution,
/// recorded result codecs and current-global fallback services.
pub struct Builder {
    pub entries: Vec<Entry>,
    pub chapters: Vec<usize>,
    pub boundary: u32,
    pub current: usize,
    pub done: bool,
    saved: u32,
    event: i32,
    asynchronous: bool,
    has_text: bool,
    labels: [String; 2],
}
impl Builder {
    pub fn new(saved: u32) -> Self {
        Self {
            entries: vec![],
            chapters: vec![],
            boundary: 0,
            current: 0,
            done: saved == 0,
            saved,
            event: -1,
            asynchronous: false,
            has_text: false,
            labels: Default::default(),
        }
    }
    pub fn historical(&self) -> bool {
        self.boundary < self.saved
    }
    pub fn current_row(&self) -> Option<usize> {
        (!self.entries.is_empty()).then(|| self.current.min(self.entries.len() - 1))
    }
    pub fn message(
        &mut self,
        id: u32,
        auto_wait: bool,
        arg: i32,
        text: &str,
        disallow_unread: bool,
        was_read: bool,
    ) {
        if self.event < 0 {
            self.boundary = self.boundary.wrapping_add(1);
        }
        if self.historical() {
            self.current = self.entries.len();
        }
        self.asynchronous = !auto_wait;
        self.has_text = true;
        if !self.historical() && disallow_unread && (id as i32) >= 0 && !was_read {
            self.done = true;
            return;
        }
        self.entries.push(Entry {
            boundary: self.boundary,
            item: Item::Message {
                message_id: id,
                message_arg: arg,
                text: text.to_owned(),
            },
        });
    }
    pub fn message_wait(&mut self, signal: i32) {
        if signal < 0 {
            self.asynchronous = false;
        }
    }
    pub fn message_close(&mut self) {
        if !self.asynchronous {
            self.has_text = false;
        }
    }
    pub fn begin_interactive(&mut self) -> bool {
        if !self.has_text {
            self.boundary = self.boundary.wrapping_add(1);
        }
        self.historical()
    }
    pub fn choice(
        &mut self,
        response: Option<i32>,
        title: &str,
        variants: &[String],
    ) -> Result<(), &'static str> {
        let Some(response) = response else {
            self.done = true;
            return Ok(());
        };
        let text = usize::try_from(response)
            .ok()
            .and_then(|i| variants.get(i))
            .ok_or("Ger history SELECT result has no variant")?;
        let text = if text.starts_with('*') {
            text.split_once('.').map_or("", |(_, s)| s)
        } else {
            text
        };
        if self.historical() {
            self.current = self.entries.len();
        }
        self.chapters.push(self.entries.len());
        self.entries.push(Entry {
            boundary: self.boundary,
            item: Item::Choice {
                title: title.to_owned(),
                text: text.to_owned(),
                response,
            },
        });
        self.boundary = self.boundary.wrapping_add(1);
        Ok(())
    }
    pub fn quiz(&mut self, historical: bool) {
        if historical {
            self.boundary = self.boundary.wrapping_add(1);
        } else {
            self.done = true;
        }
    }
    pub fn save_info(&mut self, level: i32, text: &str) -> Result<(), &'static str> {
        let slot = self
            .labels
            .get_mut(usize::try_from(level).map_err(|_| "Ger SAVEINFO level")?)
            .ok_or("Ger SAVEINFO level")?;
        *slot = text.to_owned();
        Ok(())
    }
    pub fn checkpoint(&mut self) {
        self.boundary = self.boundary.wrapping_add(1);
        if self.historical() {
            self.current = self.entries.len();
        }
        self.chapters.push(self.entries.len());
        self.entries.push(Entry {
            boundary: self.boundary,
            item: Item::Checkpoint {
                title: self.labels[0].clone(),
                text: self.labels[1].clone(),
            },
        });
    }
    pub fn event_begin(&mut self, event: i32) {
        self.boundary = self.boundary.wrapping_add(1);
        self.event = event;
    }
    pub fn event_end(&mut self) {
        self.boundary = self.boundary.wrapping_add(1);
        self.event = -1;
    }
    pub fn resume_set(&mut self) {
        self.boundary = self.boundary.wrapping_add(1);
    }
    pub fn stop(&mut self) {
        self.done = true;
    }
}
pub fn future_choice(index: i32, remembered: u8, count: usize, visibility: i32) -> Option<i32> {
    if index < 0 || remembered == 0 || usize::from(remembered) > count {
        return None;
    }
    let response = i32::from(remembered) - 1;
    ((visibility as u32).wrapping_shr(response as u32) & 1 != 0).then_some(response)
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
