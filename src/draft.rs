//! Portable formatted text for Draft. Browser adapters render these fields;
//! pasted HTML and executable markup are never part of the document format.
use crate::Result;
use serde::{Deserialize, Serialize};

pub const MAX_TEXT_BYTES: usize = 512 * 1024;
pub const MAX_BLOCKS: usize = 4096;
pub const MAX_RUNS: usize = 16_384;
pub const MAX_LINK_BYTES: usize = 8192;
pub const DEFAULT_SIZE: u8 = 16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListKind {
    #[default]
    None,
    Bullet,
    Number,
}

fn is_left(value: &Alignment) -> bool {
    *value == Alignment::Left
}
fn is_no_list(value: &ListKind) -> bool {
    *value == ListKind::None
}
fn is_default_size(value: &u8) -> bool {
    *value == DEFAULT_SIZE
}
fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Draft {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Block {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<Run>,
    #[serde(skip_serializing_if = "is_left")]
    pub alignment: Alignment,
    #[serde(skip_serializing_if = "is_no_list")]
    pub list: ListKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Run {
    pub text: String,
    #[serde(skip_serializing_if = "is_default_size")]
    pub size: u8,
    #[serde(skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            text: String::new(),
            size: DEFAULT_SIZE,
            bold: false,
            italic: false,
            underline: false,
            link: None,
        }
    }
}

impl Draft {
    /// A deliberately blank paragraph still carries spacing and is retained.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn validate(&self) -> Result<()> {
        if self.blocks.len() > MAX_BLOCKS {
            return Err("Draft supports at most 4096 paragraphs or list items".into());
        }
        let mut text_bytes = 0_usize;
        let mut runs = 0_usize;
        for block in &self.blocks {
            runs = runs.saturating_add(block.runs.len());
            if runs > MAX_RUNS {
                return Err("Draft supports at most 16384 text runs".into());
            }
            for run in &block.runs {
                text_bytes = text_bytes.saturating_add(run.text.len());
                if text_bytes > MAX_TEXT_BYTES {
                    return Err("Draft text must be at most 512 KiB".into());
                }
                if !(8..=72).contains(&run.size) {
                    return Err("Draft text size must be between 8 and 72".into());
                }
                if run.text.chars().any(|character| {
                    character.is_control() && !matches!(character, '\n' | '\r' | '\t')
                }) {
                    return Err(
                        "Draft text may contain newlines and tabs, but no other control characters"
                            .into(),
                    );
                }
                if let Some(link) = &run.link {
                    validate_link(link)?;
                }
            }
        }
        Ok(())
    }

    /// Copyable text for email, Notes and Tasks; formatting remains in Draft.
    pub fn plain_text(&self) -> String {
        let mut text = String::new();
        let mut number = 0_usize;
        for (index, block) in self.blocks.iter().enumerate() {
            if index > 0 {
                text.push('\n');
            }
            match block.list {
                ListKind::Number => {
                    number += 1;
                    text.push_str(&number.to_string());
                    text.push_str(". ");
                }
                ListKind::Bullet => {
                    number = 0;
                    text.push_str("• ");
                }
                ListKind::None => number = 0,
            }
            for run in &block.runs {
                text.push_str(&run.text);
            }
        }
        text
    }
}

fn validate_link(link: &str) -> Result<()> {
    let invalid = || {
        "Draft links must be http://, https:// or mailto: URLs without spaces, and at most 8192 bytes".to_owned()
    };
    if link.len() > MAX_LINK_BYTES
        || link
            .chars()
            .any(|value| value.is_control() || value.is_whitespace())
    {
        return Err(invalid());
    }
    let Some((scheme, rest)) = link.split_once(':') else {
        return Err(invalid());
    };
    let valid = match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => rest
            .strip_prefix("//")
            .and_then(|value| value.split(['/', '?', '#']).next())
            .is_some_and(|authority| !authority.is_empty()),
        "mailto" => !rest.is_empty(),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid())
    }
}
