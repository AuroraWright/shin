//! Optional code-defined UI text overrides, scoped to one asset server.
use std::{borrow::Cow, collections::BTreeMap};

use anyhow::{Context, bail, ensure};

use super::system::AssetIo;

#[derive(Default)]
pub struct TextOverrides {
    exact: BTreeMap<String, String>,
    templates: Vec<Template>,
}

#[derive(Clone)]
enum Part {
    Literal(String),
    Slot(String),
}

struct Template {
    key: String,
    source: Vec<Part>,
    target: Vec<Part>,
    fixed: usize,
    translate_captures: bool,
}

fn parts(text: &str) -> Option<Vec<Part>> {
    let mut result = Vec::new();
    let mut literal = String::new();
    let mut chars = text.chars().peekable();
    let mut anonymous = 0;
    while let Some(ch) = chars.next() {
        if ch == '{' {
            if chars.peek() == Some(&'{') {
                chars.next();
                literal.push('{');
                continue;
            }
            if !literal.is_empty() {
                result.push(Part::Literal(std::mem::take(&mut literal)));
            }
            let mut slot = String::new();
            loop {
                match chars.next()? {
                    '}' => break,
                    '{' => return None,
                    ch => slot.push(ch),
                }
            }
            let name = slot.split(':').next()?;
            let name = if name.is_empty() {
                let name = anonymous.to_string();
                anonymous += 1;
                name
            } else {
                if !name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                {
                    return None;
                }
                name.to_owned()
            };
            result.push(Part::Slot(name));
        } else if ch == '}' {
            if chars.next()? != '}' {
                return None;
            }
            literal.push('}');
        } else {
            literal.push(ch);
        }
    }
    if !literal.is_empty() {
        result.push(Part::Literal(literal));
    }
    Some(result)
}

impl Template {
    fn new(key: &str, value: &str) -> anyhow::Result<Option<Self>> {
        let Some(source) = parts(key) else {
            return Ok(None);
        };
        if !source.iter().any(|p| matches!(p, Part::Slot(_))) {
            return Ok(None);
        }
        ensure!(
            !source
                .windows(2)
                .any(|p| matches!(p, [Part::Slot(_), Part::Slot(_)])),
            "adjacent placeholders in {key:?}"
        );
        let target = parts(value).context("Invalid replacement placeholders")?;
        for part in &target {
            if let Part::Slot(name) = part {
                ensure!(
                    source
                        .iter()
                        .any(|p| matches!(p, Part::Slot(n) if n == name)),
                    "Unknown placeholder {name:?} in replacement for {key:?}"
                );
            }
        }
        let fixed = source
            .iter()
            .map(|p| match p {
                Part::Literal(s) => s.len(),
                _ => 0,
            })
            .sum();
        // A catch-all template would match narrative and recursively match itself.
        ensure!(fixed > 0, "A template must contain literal text: {key:?}");
        Ok(Some(Self {
            key: key.to_owned(),
            source,
            target,
            fixed,
            // Other runtime values, including scenario chart titles, stay raw.
            translate_captures: key.starts_with("@r"),
        }))
    }

    fn captures<'a>(&self, text: &'a str) -> Option<BTreeMap<String, &'a str>> {
        let mut remaining = text;
        let mut captures = BTreeMap::new();
        for (index, part) in self.source.iter().enumerate() {
            match part {
                Part::Literal(literal) => remaining = remaining.strip_prefix(literal)?,
                Part::Slot(name) => {
                    let end = match self.source.get(index + 1) {
                        Some(Part::Literal(literal)) => remaining.find(literal)?,
                        None => remaining.len(),
                        _ => return None,
                    };
                    let (value, rest) = remaining.split_at(end);
                    if let Some(previous) = captures.insert(name.clone(), value) {
                        if previous != value {
                            return None;
                        }
                    }
                    remaining = rest;
                }
            }
        }
        remaining.is_empty().then_some(captures)
    }
}

impl TextOverrides {
    pub(crate) fn load(io: &AssetIo) -> Self {
        let Ok(data) = io.read_file("/localized.toml") else {
            return Self::default();
        };
        let result = data
            .read_all_sync()
            .context("Reading localized.toml")
            .and_then(|bytes| {
                Self::parse(std::str::from_utf8(&bytes).context("localized.toml must be UTF-8")?)
            });
        match result {
            Ok(table) => table,
            Err(error) => {
                tracing::warn!(error = %error, "Ignoring invalid localized.toml");
                Self::default()
            }
        }
    }

    pub(crate) fn parse(text: &str) -> anyhow::Result<Self> {
        let table = text
            .parse::<toml::Table>()
            .context("Parsing localized.toml")?;
        let mut result = Self::default();
        for (key, value) in table {
            let toml::Value::String(value) = value else {
                bail!("Expected a string value for {key:?}")
            };
            ensure!(!key.is_empty(), "Empty translation key");
            ensure!(
                value.chars().all(|ch| u32::from(ch) <= 0xffff),
                "Translation for {key:?} exceeds the game font's Unicode range"
            );
            if key == value {
                continue;
            }
            if let Some(template) = Template::new(&key, &value)? {
                result.templates.push(template);
            }
            result.exact.insert(key, value);
        }
        if !result.exact.is_empty() {
            // These code-defined wrappers compose UI labels. Only complete,
            // anchored matches may translate their independently authored parts.
            for key in [
                "@r《{title}》@r{text}",
                "@r《{title}》",
                "@r{title}@r{text}",
                "@r{title} {text}",
                "@r{title}",
            ] {
                if !result.exact.contains_key(key) {
                    result.templates.push(Template::new(key, key)?.unwrap());
                }
            }
        }
        result
            .templates
            .sort_by(|a, b| b.fixed.cmp(&a.fixed).then(a.key.cmp(&b.key)));
        Ok(result)
    }

    pub(crate) fn translate<'a>(&self, text: &'a str) -> Cow<'a, str> {
        self.translate_inner(text, 0)
    }

    fn translate_inner<'a>(&self, text: &'a str, depth: usize) -> Cow<'a, str> {
        if let Some(value) = self.exact.get(text) {
            return Cow::Owned(value.clone());
        }
        if depth >= 8 {
            return Cow::Borrowed(text);
        }
        for template in &self.templates {
            let Some(captures) = template.captures(text) else {
                continue;
            };
            let mut output = String::new();
            for part in &template.target {
                match part {
                    Part::Literal(literal) => output.push_str(literal),
                    Part::Slot(name) => {
                        let value = captures[name];
                        if template.translate_captures && value.len() < text.len() {
                            output.push_str(&self.translate_inner(value, depth + 1));
                        } else {
                            output.push_str(value);
                        }
                    }
                }
            }
            if output != text {
                return Cow::Owned(output);
            }
        }
        Cow::Borrowed(text)
    }
}

#[cfg(test)]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localization_exact_utf8_fallback_and_no_substrings_or_recursive_output() {
        let table = TextOverrides::parse(
            "\"頭\" = \"Head\"\n\"確認\" = \"OK\"\n\"OK\" = \"Other\"\n\"同じ\" = \"同じ\"\n",
        )
        .unwrap();
        assert_eq!(table.translate("確認"), "OK");
        assert_eq!(table.translate("頭"), "Head");
        for text in ["頭痛", "彼の頭", "同じ", "未設定", ""] {
            assert!(matches!(table.translate(text), Cow::Borrowed(value) if value == text));
        }
        assert!(matches!(
            TextOverrides::default().translate("確認"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn localization_templates_preserve_runtime_values_and_translate_code_wrappers() {
        let table = TextOverrides::parse("\"達成率:{percent}％\" = \"Completion: {percent}%\"\n\"{}時間 {}分\" = \"{1} min ({0} h)\"\n\"頭\" = \"Head\"\n\"ヒロイン選択\" = \"Heroine\"\n").unwrap();
        assert_eq!(table.translate("達成率:12.5％"), "Completion: 12.5%");
        assert_eq!(table.translate("3時間 21分"), "21 min (3 h)");
        assert_eq!(
            table.translate("@r《ヒロイン選択》@r頭"),
            "@r《Heroine》@rHead"
        );
        assert_eq!(table.translate("@r《その他》@r頭"), "@r《その他》@rHead");
        let prompt =
            TextOverrides::parse("\"『{text}』へ移動\" = \"Jump to {text}\"\n\"頭\" = \"Head\"")
                .unwrap();
        assert_eq!(prompt.translate("『頭』へ移動"), "Jump to 頭");
    }

    #[test]
    fn localization_invalid_tables_are_rejected_atomically() {
        for text in [
            "bad toml",
            "\"確認\" = 1",
            "\"確認\" = true",
            "\"\" = \"Empty\"",
            "\"確認\" = \"A\"\n\"確認\" = \"B\"",
            "\"{n}分\" = \"{unknown}\"",
            "\"確認\" = \"😀\"",
        ] {
            assert!(TextOverrides::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn localization_reference_templates_parse_as_unchanged_defaults() {
        for source in [
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../localization/higurashi.localized.toml.example")),
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../localization/umineko.localized.toml.example")),
        ] {
            let table = TextOverrides::parse(source).unwrap();
            assert!(table.exact.is_empty());
            assert!(table.templates.is_empty());
        }
    }

    #[test]
    fn localization_loose_root_precedes_layers_with_missing_and_invalid_fallback() {
        use crate::asset::system::LayeredAssetIo;
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        struct Directory(std::path::PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
        let directory = Directory(std::env::temp_dir().join(format!(
            "shin-localization-io-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )));
        std::fs::create_dir(&directory.0).unwrap();
        let layer = directory.0.join("layer");
        std::fs::create_dir(&layer).unwrap();
        std::fs::write(layer.join("localized.toml"), "\"確認\" = \"Layer\"").unwrap();
        let root = directory.0.join("localized.toml");
        let mut layered = LayeredAssetIo::new();
        layered.try_with_dir(&layer).unwrap();
        layered.with_localized_file(root.clone());
        let io: AssetIo = layered.into();
        assert_eq!(TextOverrides::load(&io).translate("確認"), "Layer");
        std::fs::write(&root, "\"確認\" = \"Root\"").unwrap();
        let first = TextOverrides::load(&io);
        assert_eq!(first.translate("確認"), "Root");
        std::fs::write(&root, "invalid toml").unwrap();
        assert_eq!(TextOverrides::load(&io).translate("確認"), "確認");
        assert_eq!(first.translate("確認"), "Root");
        std::fs::remove_file(&root).unwrap();
        std::fs::remove_file(layer.join("localized.toml")).unwrap();
        assert_eq!(TextOverrides::load(&io).translate("確認"), "確認");
    }
}
