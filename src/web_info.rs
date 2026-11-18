//! Read-only application information and practical browser help.
use crate::web_theme;
use eframe::egui::{self, RichText, Ui};
use jig::app_settings::{Action, AppSettings};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InfoPage {
    About,
    Help,
}

impl InfoPage {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::About => "About",
            Self::Help => "Help",
        }
    }
}

pub(super) enum InfoAction {
    Done,
    Settings,
}

pub(super) fn show(
    ui: &mut Ui,
    page: InfoPage,
    settings: &AppSettings,
    focus_done: &mut bool,
) -> Option<InfoAction> {
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        ui.heading(page.label());
        let done = ui.button("Done");
        if *focus_done {
            done.request_focus();
            *focus_done = false;
            ui.ctx().request_repaint();
        }
        if done.clicked() {
            action = Some(InfoAction::Done);
        }
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt(("information-page", page.label()))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(900.0));
            match page {
                InfoPage::About => about(ui),
                InfoPage::Help => {
                    help(ui, settings);
                    if ui.button("Open Settings").clicked() {
                        action = Some(InfoAction::Settings);
                    }
                }
            }
        });
    action
}

fn about(ui: &mut Ui) {
    ui.label(
        RichText::new(format!("Jig {}", env!("CARGO_PKG_VERSION")))
            .strong()
            .color(web_theme::colors(ui.ctx()).accent),
    );
    ui.label("A local place to work things out, written in Rust.");
    section(
        ui,
        "Your work stays on your device",
        "Tasks, dates, notes, Draft, sheets and charts save in this browser profile for this site's address. Refreshing restores saved work. There are no accounts, telemetry or backend storage. Imported documents are processed on your device.",
    );
    section(
        ui,
        "Portable files",
        "Use Data → Export JSON for a copy of your complete document. Settings has backup downloads, optional backup-folder access in supported browsers, and separate settings import/export. Browser storage can be cleared; keep file backups when your work matters.",
    );
    section(
        ui,
        "Optional connections",
        "Weather uses Open-Meteo when enabled. It requests a forecast for your chosen location.",
    );
    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        ui.hyperlink_to("Source", "https://github.com/biraz-dev/Jig");
        ui.hyperlink_to("Documentation", "https://github.com/biraz-dev/Jig#readme");
        ui.hyperlink_to(
            "Font license",
            "https://github.com/biraz-dev/Jig/blob/main/web/font-license.txt",
        );
        ui.hyperlink_to(
            "OCR notices",
            "https://github.com/biraz-dev/Jig/tree/main/web/vendor/ocr",
        );
    });
}

fn help(ui: &mut Ui, settings: &AppSettings) {
    section(
        ui,
        "Tasks and lists",
        "Add work in Add new task. The checkbox completes it; the star features a dated task in the countdown. Drag task handles to reorder. Sort places open dated tasks first, undated tasks next and completed tasks last.",
    );
    ui.label("Keep up to five lists. + adds a list; double-click a tab name to rename it. Shift-click tabs, or use Multi on touchscreens, to show several lists together. New tasks go into the active list. Click − twice, or choose Remove after the first click, to remove a list and move its tasks into the remaining list. Cancel or Esc dismisses the question. The last list stays.");
    section(
        ui,
        "Dates and calendar",
        "Use a task's date control to set Start and Due. The calendar icon opens a date picker with local 24-hour time. You can type YYYY-MM-DD or YYYY-MM-DD HH:MM. Use date fills the draft; Save date commits it.",
    );
    ui.label("You can also keep the date editor open, choose Calendar or Dates, select Start or Due, then pick a day. Colored marks show tasks due on that day; completed tasks are subdued. Selecting a day shows its tasks and optional forecast without changing the starred countdown.");
    section(
        ui,
        "Calendar and Gantt views",
        "The view buttons are Calendar, Dates and Timeline. In Dates, drag the day strip or use horizontal/Shift-wheel scrolling to pan. + and −, Ctrl-wheel or a pinch zoom it. Widening the panel reveals more dates; Today returns to today. Timeline shows bars for tasks with start and due dates, and diamonds for due-only tasks.",
    );
    ui.label("Choose Calendar for a classic Monday-first month grid. Previous/next arrows browse months; Today returns to the current day. Calendar and Dates share date selection, task colors, details, forecast and task-date picking.");
    ui.label("Drag either end of a bar to change that date; the preview saves when released as one Undo step. The other date and existing clock times stay fixed. Esc cancels a drag. Drag the panel divider or the divider beside timeline titles to resize them; double-click resets a divider.");
    section(
        ui,
        "Notes",
        "Keep up to 81 plain-text cards. Add note, type or paste, then Save or Ctrl+Enter. Enter adds a line; Esc cancels. Click text to edit it, copy to copy all of it, and pin to keep it first. Drag handles to reorder cards within their pinned or unpinned group.",
    );
    ui.label("On Tasks, drag a note's handle onto the task list to turn it into a task at the highlighted position. The note's task button also lets you choose its list. To note in the task editor converts it back. Conversion moves the record and retains its full text; a converted scheduled task keeps its dates for restoration.");
    section(
        ui,
        "Draft",
        "Gather text in one document next to Notes. Select text to set its size, bold, italic, underline, link, alignment, bullets or numbering. Copy keeps formatting where supported, with plain text as a fallback. To note and To task copy the selection, or the whole Draft, without removing the original; transfers use the existing 16 KiB text limit.",
    );
    ui.label("Draft saves as you type and travels in JSON and Jig ODS backups. Older files open an empty Draft. Ctrl/Cmd+Z and Ctrl/Cmd+Shift+Z use the shared Undo/Redo; consecutive typing is grouped. Esc returns keyboard focus to the main app; Ctrl+Alt+R opens Draft by default. Draft allows up to 512 KiB of text within the browser document limit.");
    section(
        ui,
        "Sheets and Charts",
        "Charts sits beside Sheets in the header. Select cells in Sheets and choose Insert → Send selection to Charts… to prefill a live range. Charts' Create menu adds pie, line, bar or Gantt charts, Number input, Slider and Calculated result cards linked to your cells. Create chart or Add control saves the draft; Cancel or Esc discards it.",
    );
    ui.label("Number inputs apply with Apply or Enter. Sliders commit on release within their min/max/step; result cards display a cell or formula result without changing it. Input controls protect formula cells. Edit layout reveals control editing, up/down ordering and deletion; Save control commits configuration changes. Changes use the shared Undo/Redo. JSON and Jig ODS retain links and definitions; ODS chart images are snapshots, not native editable ODS charts.");
    section(
        ui,
        "Calculator",
        "The calculator icon opens a popover without leaving your page. Try 125 * 1.15, 72 in mm, or 1 1/2 in cm. Decimal places and Copy are inside. Choose a history result to reuse its full precision. Esc or a click outside closes it.",
    );
    section(
        ui,
        "Saving and backups",
        "Your document saves automatically in this browser. Data → Export JSON creates a portable copy; Import JSON shows a review before replacing a document. Settings can choose a backup folder in supported browsers or download a backup. CSV, ODS and PDF exports use your browser's download location.",
    );
    ui.label("If a folder backup fails, Retry backup and Download backup appear in the workspace and at the top of Settings. Reconnect the folder if access is needed. Retry uses your current document; unfinished text stays in its editor. Existing backup files are kept.");
    section(
        ui,
        "Themes",
        "Settings offers Ember (the original warm look), Paper, neutral Graphite, Nord, Dracula, Catppuccin (Mocha), Gruvbox (Dark), Tokyo Night (Night), System and Custom. System follows the browser's light/dark preference. Expand Customize colors at the end of Settings to set background, surface, text and accent with hex values or a color picker. Choose Custom to use them; selecting a preset keeps your custom palette.",
    );
    ui.label("Optional system accent uses the browser's CSS AccentColor when available, with your chosen accent as the fallback. Manual controls remain available; browsers cannot read a full desktop theme. Themes apply to all browser workspaces, including Draft, without changing stored task/list, chart series or cell colors. PDF pages stay white. Settings export carries your theme separately from document data.");
    section(
        ui,
        "Undo and text editing",
        "Application Undo and Redo apply outside text fields and inside Draft. Other text editors keep their own Undo/Redo and normal copy/paste shortcuts. Application history lasts for this session; reload clears that history, while saved documents remain. Settings controls the history limit and application shortcuts.",
    );
    ui.add_space(10.0);
    ui.label(RichText::new("Current application shortcuts").strong());
    for action in Action::ALL {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(action.label()).strong());
            ui.label(
                settings
                    .binding(action)
                    .map_or_else(|| "Unbound".into(), |chord| chord.label()),
            );
        });
    }
    ui.add_space(10.0);
    ui.label("Tab / Shift+Tab moves between controls. Esc closes a popup or returns from these pages. Settings includes the shortcut editor and a contextual cheatsheet.");
    ui.add_space(10.0);
}

fn section(ui: &mut Ui, title: &str, text: &str) {
    ui.add_space(12.0);
    ui.label(RichText::new(title).strong());
    ui.label(text);
}
