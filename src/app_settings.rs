//! Portable application preferences, separate from a user's task document.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_UNDO_LEVELS: usize = 100;
pub const MAX_SETTINGS_BYTES: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Graphite,
    Light,
    System,
}

impl Theme {
    pub const ALL: [Self; 3] = [Self::Graphite, Self::Light, Self::System];

    pub fn label(self) -> &'static str {
        match self {
            Self::Graphite => "Graphite",
            Self::Light => "Light",
            Self::System => "System",
        }
    }
}

/// Canonical names also understood by egui's `Key::from_name`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum ShortcutKey {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    #[serde(rename = "0")]
    Num0,
    #[serde(rename = "1")]
    Num1,
    #[serde(rename = "2")]
    Num2,
    #[serde(rename = "3")]
    Num3,
    #[serde(rename = "4")]
    Num4,
    #[serde(rename = "5")]
    Num5,
    #[serde(rename = "6")]
    Num6,
    #[serde(rename = "7")]
    Num7,
    #[serde(rename = "8")]
    Num8,
    #[serde(rename = "9")]
    Num9,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Comma,
    Period,
    Slash,
    Backslash,
    Minus,
    Equals,
    Semicolon,
    Quote,
    Backtick,
    OpenBracket,
    CloseBracket,
}

impl ShortcutKey {
    pub const ALL: [Self; 59] = [
        Self::A,
        Self::B,
        Self::C,
        Self::D,
        Self::E,
        Self::F,
        Self::G,
        Self::H,
        Self::I,
        Self::J,
        Self::K,
        Self::L,
        Self::M,
        Self::N,
        Self::O,
        Self::P,
        Self::Q,
        Self::R,
        Self::S,
        Self::T,
        Self::U,
        Self::V,
        Self::W,
        Self::X,
        Self::Y,
        Self::Z,
        Self::Num0,
        Self::Num1,
        Self::Num2,
        Self::Num3,
        Self::Num4,
        Self::Num5,
        Self::Num6,
        Self::Num7,
        Self::Num8,
        Self::Num9,
        Self::F1,
        Self::F2,
        Self::F3,
        Self::F4,
        Self::F5,
        Self::F6,
        Self::F7,
        Self::F8,
        Self::F9,
        Self::F10,
        Self::F11,
        Self::F12,
        Self::Comma,
        Self::Period,
        Self::Slash,
        Self::Backslash,
        Self::Minus,
        Self::Equals,
        Self::Semicolon,
        Self::Quote,
        Self::Backtick,
        Self::OpenBracket,
        Self::CloseBracket,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
            Self::F => "F",
            Self::G => "G",
            Self::H => "H",
            Self::I => "I",
            Self::J => "J",
            Self::K => "K",
            Self::L => "L",
            Self::M => "M",
            Self::N => "N",
            Self::O => "O",
            Self::P => "P",
            Self::Q => "Q",
            Self::R => "R",
            Self::S => "S",
            Self::T => "T",
            Self::U => "U",
            Self::V => "V",
            Self::W => "W",
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
            Self::Num0 => "0",
            Self::Num1 => "1",
            Self::Num2 => "2",
            Self::Num3 => "3",
            Self::Num4 => "4",
            Self::Num5 => "5",
            Self::Num6 => "6",
            Self::Num7 => "7",
            Self::Num8 => "8",
            Self::Num9 => "9",
            Self::F1 => "F1",
            Self::F2 => "F2",
            Self::F3 => "F3",
            Self::F4 => "F4",
            Self::F5 => "F5",
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::F10 => "F10",
            Self::F11 => "F11",
            Self::F12 => "F12",
            Self::Comma => "Comma",
            Self::Period => "Period",
            Self::Slash => "Slash",
            Self::Backslash => "Backslash",
            Self::Minus => "Minus",
            Self::Equals => "Equals",
            Self::Semicolon => "Semicolon",
            Self::Quote => "Quote",
            Self::Backtick => "Backtick",
            Self::OpenBracket => "OpenBracket",
            Self::CloseBracket => "CloseBracket",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.name() == name)
    }

    fn is_function(self) -> bool {
        matches!(
            self,
            Self::F1
                | Self::F2
                | Self::F3
                | Self::F4
                | Self::F5
                | Self::F6
                | Self::F7
                | Self::F8
                | Self::F9
                | Self::F10
                | Self::F11
                | Self::F12
        )
    }

    fn display(self) -> &'static str {
        match self {
            Self::Comma => ",",
            Self::Period => ".",
            Self::Slash => "/",
            Self::Backslash => "\\",
            Self::Minus => "-",
            Self::Equals => "=",
            Self::Semicolon => ";",
            Self::Quote => "'",
            Self::Backtick => "`",
            Self::OpenBracket => "[",
            Self::CloseBracket => "]",
            key => key.name(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chord {
    pub key: ShortcutKey,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub meta: bool,
}

impl Default for Chord {
    fn default() -> Self {
        Self {
            key: ShortcutKey::Comma,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }
}

impl Chord {
    pub fn new(key: ShortcutKey) -> Self {
        Self {
            key,
            ..Self::default()
        }
    }

    pub fn ctrl(key: ShortcutKey) -> Self {
        Self {
            key,
            ctrl: true,
            ..Self::default()
        }
    }

    pub fn ctrl_alt(key: ShortcutKey) -> Self {
        Self {
            key,
            ctrl: true,
            alt: true,
            ..Self::default()
        }
    }

    pub fn ctrl_shift(key: ShortcutKey) -> Self {
        Self {
            key,
            ctrl: true,
            shift: true,
            ..Self::default()
        }
    }

    pub fn label(self) -> String {
        let mut parts = Vec::with_capacity(5);
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.meta {
            parts.push("Meta");
        }
        parts.push(self.key.display());
        parts.join("+")
    }

    fn validate(self, action: Action) -> Result<()> {
        if !self.ctrl && !self.alt && !self.meta && !self.key.is_function() {
            return Err(
                "Use Ctrl, Alt or Meta with a printable key so ordinary typing stays available"
                    .into(),
            );
        }
        if self == Self::new(ShortcutKey::F2) && action != Action::RenameTaskList {
            return Err("F2 is reserved for contextual list, panel and cell editing".into());
        }
        let command = (self.ctrl || self.meta) && !self.alt;
        let standard_undo = command && self.key == ShortcutKey::Z && !self.shift;
        let standard_redo = command && self.key == ShortcutKey::Z && self.shift;
        if standard_undo && action != Action::Undo || standard_redo && action != Action::Redo {
            return Err(
                "Standard Undo and Redo shortcuts are reserved for their own actions".into(),
            );
        }
        if standard_undo || standard_redo {
            return Ok(());
        }
        if command
            && matches!(
                self.key,
                ShortcutKey::A
                    | ShortcutKey::B
                    | ShortcutKey::C
                    | ShortcutKey::D
                    | ShortcutKey::F
                    | ShortcutKey::G
                    | ShortcutKey::H
                    | ShortcutKey::I
                    | ShortcutKey::J
                    | ShortcutKey::K
                    | ShortcutKey::L
                    | ShortcutKey::N
                    | ShortcutKey::O
                    | ShortcutKey::P
                    | ShortcutKey::Q
                    | ShortcutKey::R
                    | ShortcutKey::S
                    | ShortcutKey::T
                    | ShortcutKey::U
                    | ShortcutKey::V
                    | ShortcutKey::W
                    | ShortcutKey::X
                    | ShortcutKey::Y
                    | ShortcutKey::Num0
                    | ShortcutKey::Num1
                    | ShortcutKey::Num2
                    | ShortcutKey::Num3
                    | ShortcutKey::Num4
                    | ShortcutKey::Num5
                    | ShortcutKey::Num6
                    | ShortcutKey::Num7
                    | ShortcutKey::Num8
                    | ShortcutKey::Num9
                    | ShortcutKey::Minus
                    | ShortcutKey::Equals
            )
        {
            return Err("That shortcut is reserved for text editing, files or the browser".into());
        }
        if matches!(
            self.key,
            ShortcutKey::F1
                | ShortcutKey::F3
                | ShortcutKey::F5
                | ShortcutKey::F6
                | ShortcutKey::F7
                | ShortcutKey::F11
                | ShortcutKey::F12
        ) || self.alt
            && !self.ctrl
            && !self.meta
            && matches!(self.key, ShortcutKey::D | ShortcutKey::F | ShortcutKey::F4)
        {
            return Err("That shortcut is reserved by the browser or operating system".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    ToggleWorkspace,
    Tasks,
    Notes,
    Calculator,
    Calendar,
    Data,
    Settings,
    Undo,
    Redo,
    Maximize,
    RenameTaskList,
    OpenLocal,
}

impl Action {
    pub const ALL: [Self; 12] = [
        Self::ToggleWorkspace,
        Self::Tasks,
        Self::Notes,
        Self::Calculator,
        Self::Calendar,
        Self::Data,
        Self::Settings,
        Self::Undo,
        Self::Redo,
        Self::Maximize,
        Self::RenameTaskList,
        Self::OpenLocal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ToggleWorkspace => "Sheet / Tasks",
            Self::Tasks => "Tasks",
            Self::Notes => "Notes",
            Self::Calculator => "Calculator",
            Self::Calendar => "Calendar / Timeline",
            Self::Data => "Data",
            Self::Settings => "Settings",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Maximize => "Maximize panel",
            Self::RenameTaskList => "Rename list / expand panel",
            Self::OpenLocal => "Open local data",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ToggleWorkspace => "Switch between Sheet and Tasks.",
            Self::Tasks => "Open the Tasks workspace.",
            Self::Notes => "Open the Notes workspace.",
            Self::Calculator => "Open the calculator without leaving this workspace.",
            Self::Calendar => "Switch between the calendar and timeline.",
            Self::Data => "Show or hide data import, export and dataset controls.",
            Self::Settings => "Show or hide application settings.",
            Self::Undo => "Undo the last committed change; text editors keep their own Undo.",
            Self::Redo => "Redo the last undone change; text editors keep their own Redo.",
            Self::Maximize => {
                "Expand or restore the focused Tasks or calendar panel outside text editors."
            }
            Self::RenameTaskList => {
                "Rename a focused task-list tab, otherwise expand or restore its panel."
            }
            Self::OpenLocal => "Reload the saved document from this browser.",
        }
    }

    pub fn default_binding(self) -> Chord {
        match self {
            Self::ToggleWorkspace => Chord::ctrl_alt(ShortcutKey::W),
            Self::Tasks => Chord::ctrl_alt(ShortcutKey::Num2),
            Self::Notes => Chord::ctrl_alt(ShortcutKey::N),
            Self::Calculator => Chord::ctrl_alt(ShortcutKey::Num4),
            Self::Calendar => Chord::ctrl_alt(ShortcutKey::T),
            Self::Data => Chord::ctrl_alt(ShortcutKey::D),
            Self::Settings => Chord::ctrl(ShortcutKey::Comma),
            Self::Undo => Chord::ctrl(ShortcutKey::Z),
            Self::Redo => Chord::ctrl_shift(ShortcutKey::Z),
            Self::Maximize => Chord::ctrl(ShortcutKey::M),
            Self::RenameTaskList => Chord::new(ShortcutKey::F2),
            Self::OpenLocal => Chord::ctrl_alt(ShortcutKey::O),
        }
    }
}

/// Editor/navigation keys stay fixed so typing, clipboard and file controls
/// work consistently even after application shortcuts are customized.
pub const FIXED_SHORTCUTS: &[(&str, &str)] = &[
    ("Tab / Shift+Tab", "Move between controls."),
    (
        "Enter / Space",
        "Activate a focused control; Enter adds a task.",
    ),
    (
        "Esc",
        "Close a popup, cancel a gesture or return from an expanded panel.",
    ),
    (
        "Arrows",
        "Select panels outside editors; move dates in the focused calendar.",
    ),
    ("Shift+click task-list tabs", "Show several lists together."),
    ("Ctrl+Enter", "Save a note."),
    (
        "Ctrl+A / C / X / V",
        "Select all, copy, cut and paste inside editors.",
    ),
    (
        "Ctrl+Z / Ctrl+Shift+Z",
        "Local text Undo and Redo while editing text.",
    ),
    (
        "Ctrl+S / O / P",
        "Save ODS, open ODS or export PDF in Sheet and Tasks; save notes in Notes.",
    ),
    ("F2 / Ctrl+G", "Edit a sheet cell or go to a sheet address."),
    ("Shift+F11", "Add a sheet."),
    ("Alt+PageUp / Alt+PageDown", "Switch sheets."),
    (
        "Delete / Backspace in Sheet",
        "Clear selected cell contents.",
    ),
    (
        "Ctrl+Home / Ctrl+End in Sheet",
        "Go to the first or last used cell.",
    ),
    ("Shift+arrows in Sheet", "Extend the selected cell range."),
    (
        "Shift+F10 on sheet headers",
        "Open the row or column context menu; touch-hold also works.",
    ),
    ("Alt+arrows on a note grip", "Move a note within the grid."),
    (
        "PageUp / PageDown / Home",
        "Pan the focused calendar, or return to today.",
    ),
    (
        "+ / − / Ctrl+wheel / pinch",
        "Zoom the calendar; drag or Shift+wheel pans it.",
    ),
    (
        "Left / Right on dividers",
        "Resize; hold Shift for larger steps, double-click to reset.",
    ),
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppSettings {
    pub version: u8,
    pub theme: Theme,
    pub undo_levels: usize,
    pub auto_backup: bool,
    /// Missing means the default binding; an explicit null disables an action.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub shortcuts: BTreeMap<Action, Option<Chord>>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: 1,
            theme: Theme::Graphite,
            undo_levels: 20,
            auto_backup: false,
            shortcuts: BTreeMap::new(),
        }
    }
}

impl AppSettings {
    pub fn binding(&self, action: Action) -> Option<Chord> {
        self.shortcuts
            .get(&action)
            .copied()
            .unwrap_or_else(|| Some(action.default_binding()))
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err("Unsupported application settings version".into());
        }
        if self.undo_levels > MAX_UNDO_LEVELS {
            return Err(format!(
                "Undo levels must be between 0 and {MAX_UNDO_LEVELS}"
            ));
        }
        let mut used = BTreeMap::new();
        for action in Action::ALL {
            if let Some(chord) = self.binding(action) {
                chord.validate(action)?;
                if let Some(previous) = used.insert(chord, action) {
                    return Err(format!(
                        "{} is already assigned to {}",
                        chord.label(),
                        previous.label()
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn from_json(text: &str) -> Result<Self> {
        if text.len() > MAX_SETTINGS_BYTES {
            return Err("Application settings exceed the 32 KiB limit".into());
        }
        let settings: Self = serde_json::from_str(text).map_err(|error| error.to_string())?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|error| error.to_string())
    }

    pub fn set_binding(&mut self, action: Action, chord: Option<Chord>) -> Result<()> {
        let mut next = self.clone();
        if chord == Some(action.default_binding()) {
            next.shortcuts.remove(&action);
        } else {
            next.shortcuts.insert(action, chord);
        }
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn reset_binding(&mut self, action: Action) -> Result<()> {
        self.set_binding(action, Some(action.default_binding()))
    }

    pub fn reset_shortcuts(&mut self) {
        self.shortcuts.clear();
    }
}
