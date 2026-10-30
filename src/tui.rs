use crate::storage::Store;
use chrono::{Datelike, Local, Utc};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use jig::{
    calendar,
    model::{countdown, State},
    units,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{io, time::Duration};

const BG: Color = Color::Rgb(12, 19, 27);
const CREAM: Color = Color::Rgb(246, 218, 174);
const PINK: Color = Color::Rgb(234, 133, 173);
const MUTED: Color = Color::Rgb(144, 161, 202);
const ACCENT: Color = Color::Rgb(109, 185, 238);
const NAMES: [&str; 5] = [
    "CALCULATOR",
    "LENGTH",
    "TASKS",
    "MAJOR DEADLINE",
    "CALENDAR",
];
const TOOL_ORDER: [usize; 5] = [3, 2, 4, 0, 1];

#[derive(Clone)]
enum FormKind {
    Calc,
    Units,
    Task(Option<u64>),
    Deadline(Option<u64>),
    Entry(Option<u64>),
}
struct Form {
    kind: FormKind,
    labels: Vec<&'static str>,
    values: Vec<String>,
    field: usize,
}
struct App {
    focus: usize,
    expanded: bool,
    selected: [usize; 5],
    year: i32,
    month: u32,
    form: Option<Form>,
    length: String,
    status: String,
    help: bool,
}
impl App {
    fn new(state: &State) -> Self {
        let today = Local::now().date_naive();
        Self {
            focus: state.preferences.focus,
            expanded: false,
            selected: [state.history.len().saturating_sub(1), 0, 0, 0, 0],
            year: today.year(),
            month: today.month(),
            form: None,
            length: "12in".into(),
            status: "Ready · data stays on this computer".into(),
            help: false,
        }
    }
    fn begin(&mut self, state: &State, edit: bool) {
        let form = match self.focus {
            0 => Form {
                kind: FormKind::Calc,
                labels: vec!["Expression (ans = last result)"],
                values: vec![String::new()],
                field: 0,
            },
            1 => Form {
                kind: FormKind::Units,
                labels: vec!["Length (e.g. 1 1/2in)"],
                values: vec![self.length.clone()],
                field: 0,
            },
            2 => {
                let task = if edit {
                    state.tasks.get(self.selected[2])
                } else {
                    None
                };
                Form {
                    kind: FormKind::Task(task.map(|t| t.id)),
                    labels: vec!["Task"],
                    values: vec![task.map_or(String::new(), |t| t.text.clone())],
                    field: 0,
                }
            }
            3 => {
                let d = if edit {
                    state.deadlines.get(self.selected[3])
                } else {
                    None
                };
                Form {
                    kind: FormKind::Deadline(d.map(|d| d.id)),
                    labels: vec!["Title", "RFC3339 time with timezone"],
                    values: vec![
                        d.map_or(String::new(), |d| d.title.clone()),
                        d.map_or(String::new(), |d| d.at.with_timezone(&Local).to_rfc3339()),
                    ],
                    field: 0,
                }
            }
            _ => {
                let e = if edit {
                    state.entries.get(self.selected[4])
                } else {
                    None
                };
                Form {
                    kind: FormKind::Entry(e.map(|e| e.id)),
                    labels: vec!["Title", "Date YYYY-MM-DD"],
                    values: vec![
                        e.map_or(String::new(), |e| e.title.clone()),
                        e.map_or(format!("{:04}-{:02}-01", self.year, self.month), |e| {
                            e.date.to_string()
                        }),
                    ],
                    field: 0,
                }
            }
        };
        self.form = Some(form);
    }
    fn submit(&mut self, state: &mut State) -> Result<bool, String> {
        let form = self.form.as_ref().ok_or("No input")?;
        let dirty = match form.kind {
            FormKind::Calc => {
                let n = state.calculate(&form.values[0])?;
                self.status = format!("= {}", state.number(n));
                self.selected[0] = state.history.len() - 1;
                true
            }
            FormKind::Units => {
                units::convert(&form.values[0], "m")?;
                self.length = form.values[0].clone();
                false
            }
            FormKind::Task(id) => {
                if let Some(id) = id {
                    state.edit_task(id, &form.values[0])?;
                } else {
                    state.add_task(&form.values[0])?;
                    self.selected[2] = state.tasks.len() - 1;
                }
                true
            }
            FormKind::Deadline(id) => {
                state.set_deadline(id, &form.values[0], &form.values[1])?;
                if id.is_none() {
                    self.selected[3] = state.deadlines.len() - 1;
                }
                true
            }
            FormKind::Entry(id) => {
                state.set_entry(id, &form.values[0], &form.values[1])?;
                if id.is_none() {
                    self.selected[4] = state.entries.len() - 1;
                }
                true
            }
        };
        Ok(dirty)
    }
    fn key(&mut self, key: KeyEvent, state: &mut State) -> Result<bool, String> {
        if let Some(form) = &mut self.form {
            match key.code {
                KeyCode::Esc => self.form = None,
                KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % form.values.len(),
                KeyCode::BackTab | KeyCode::Up => {
                    form.field = (form.field + form.values.len() - 1) % form.values.len()
                }
                KeyCode::Backspace => {
                    form.values[form.field].pop();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    form.values[form.field].clear()
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    if form.values[form.field].len() < 4096 {
                        form.values[form.field].push(c);
                    }
                }
                KeyCode::Enter => {
                    if form.field + 1 < form.values.len() {
                        form.field += 1;
                    } else {
                        return self.submit(state);
                    }
                }
                _ => {}
            }
            return Ok(false);
        }
        if self.help {
            self.help = false;
            return Ok(false);
        }
        match key.code {
            KeyCode::Tab => {
                let position = TOOL_ORDER
                    .iter()
                    .position(|i| *i == self.focus)
                    .unwrap_or(0);
                self.focus = TOOL_ORDER[(position + 1) % 5];
            }
            KeyCode::BackTab => {
                let position = TOOL_ORDER
                    .iter()
                    .position(|i| *i == self.focus)
                    .unwrap_or(0);
                self.focus = TOOL_ORDER[(position + 4) % 5];
            }
            KeyCode::Char(c @ '1'..='5') => self.focus = TOOL_ORDER[c as usize - '1' as usize],
            KeyCode::F(2) => self.expanded = !self.expanded,
            KeyCode::Esc => self.expanded = false,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Enter | KeyCode::Char('i' | 'a') => self.begin(state, false),
            KeyCode::Char('e') => self.begin(state, true),
            KeyCode::Char('r') if self.focus == 0 => {
                if let Some(h) = state.history.get(self.selected[0]) {
                    self.begin(state, false);
                    if let Some(form) = &mut self.form {
                        form.values[0] = h.result.to_string();
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected[self.focus] = self.selected[self.focus].saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = match self.focus {
                    0 => state.history.len(),
                    2 => state.tasks.len(),
                    3 => state.deadlines.len(),
                    4 => state.entries.len(),
                    _ => 0,
                };
                self.selected[self.focus] =
                    (self.selected[self.focus] + 1).min(len.saturating_sub(1));
            }
            KeyCode::Char(' ') if self.focus == 2 => {
                if let Some(t) = state.tasks.get(self.selected[2]) {
                    state.toggle_task(t.id)?;
                    return Ok(true);
                }
            }
            KeyCode::Char('K' | 'J') if self.focus == 2 => {
                let i = self.selected[2];
                let next = if key.code == KeyCode::Char('K') {
                    i.saturating_sub(1)
                } else {
                    (i + 1).min(state.tasks.len().saturating_sub(1))
                };
                if let Some(t) = state.tasks.get(i) {
                    state.move_task(t.id, next + 1)?;
                    self.selected[2] = next;
                    return Ok(true);
                }
            }
            KeyCode::Char('f') if self.focus == 3 => {
                if let Some(d) = state.deadlines.get(self.selected[3]) {
                    state.feature(d.id)?;
                    return Ok(true);
                }
            }
            KeyCode::Delete | KeyCode::Char('d') => match self.focus {
                2 => {
                    if let Some(t) = state.tasks.get(self.selected[2]) {
                        state.remove_task(t.id)?;
                        self.selected[2] =
                            self.selected[2].min(state.tasks.len().saturating_sub(1));
                        return Ok(true);
                    }
                }
                3 => {
                    if let Some(d) = state.deadlines.get(self.selected[3]) {
                        state.remove_deadline(d.id)?;
                        self.selected[3] =
                            self.selected[3].min(state.deadlines.len().saturating_sub(1));
                        return Ok(true);
                    }
                }
                4 => {
                    if let Some(e) = state.entries.get(self.selected[4]) {
                        state.remove_entry(e.id)?;
                        self.selected[4] =
                            self.selected[4].min(state.entries.len().saturating_sub(1));
                        return Ok(true);
                    }
                }
                _ => {}
            },
            KeyCode::Left | KeyCode::Right if self.focus == 4 => {
                (self.year, self.month) = calendar::shift(
                    self.year,
                    self.month,
                    if key.code == KeyCode::Left { -1 } else { 1 },
                )?;
            }
            KeyCode::Char('t') if self.focus == 4 => {
                let now = Local::now();
                self.year = now.year();
                self.month = now.month();
            }
            _ => {}
        }
        state.preferences.focus = self.focus;
        Ok(false)
    }
}

struct Screen;
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}
pub fn run(state: &mut State, store: &Store) -> Result<(), Box<dyn std::error::Error>> {
    let _screen = Screen;
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let mut app = App::new(state);
    loop {
        terminal.draw(|frame| draw(frame, state, &mut app))?;
        if !event::poll(Duration::from_secs(1))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            let quitting = (app.form.is_none() && !app.help && key.code == KeyCode::Char('q'))
                || (key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL));
            if quitting {
                match store.save(state) {
                    Ok(()) => break,
                    Err(e) => {
                        app.status = format!("Save failed: {e}. Fix the path and retry q.");
                        continue;
                    }
                }
            }
            let before = state.clone();
            let submitted = app
                .form
                .as_ref()
                .is_some_and(|f| key.code == KeyCode::Enter && f.field + 1 == f.values.len());
            match app.key(key, state) {
                Ok(dirty) => {
                    if dirty {
                        if let Err(e) = store.save(state) {
                            *state = before;
                            app.status = format!("Save failed; change rolled back: {e}");
                            continue;
                        }
                        if app.focus != 0 {
                            app.status = "Saved locally".into();
                        }
                    }
                    if submitted {
                        app.form = None;
                    }
                }
                Err(e) => {
                    *state = before;
                    app.status = e;
                }
            }
        }
    }
    Ok(())
}
fn panel(title: String, focused: bool) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { PINK } else { MUTED }))
        .title_style(Style::default().fg(PINK))
}
fn draw(frame: &mut Frame, state: &State, app: &mut App) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(BG).fg(CREAM)),
        area,
    );
    if area.width < 32 || area.height < 10 {
        frame.render_widget(
            Paragraph::new("JIG\nTerminal too small. Resize to 32×10 or larger.\nq quit")
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " JIG ",
                Style::default().fg(CREAM).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" / artist's workbench", Style::default().fg(MUTED)),
            Span::raw(format!(
                "   {}   • local",
                Local::now().format("%a %d %b %Y  %H:%M:%S")
            )),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(MUTED)),
        ),
        rows[0],
    );
    if app.help {
        frame.render_widget(Paragraph::new("Tab / Shift-Tab or 1–5: focus tool\nF2: expand focused tool · Esc: return\nEnter / i / a: input or add · e: edit selected item\n↑↓ / j k: select and scroll · d / Delete: delete\nCalculator: r reuse selected result · ans uses latest result\nTasks: Space toggle · J / K reorder\nDeadlines: f feature selected · time requires explicit timezone\nCalendar: ← → month · t today · a/e/d manage dated notes\nInput: Tab next field · Ctrl-U clear · Backspace · Enter submit · Esc cancel\nq quit · Ctrl-C save and quit · any key closes help\nDates and calendar markers display in your computer's local timezone.").block(panel(" KEYBOARD ".into(), true)).wrap(Wrap { trim: true }), rows[1]);
    } else if app.expanded || area.width < 100 || area.height < 36 {
        tool(frame, rows[1], app.focus, state, app);
    } else {
        let columns = Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
            .split(rows[1]);
        let left = Layout::vertical([
            Constraint::Min(11),
            Constraint::Length(10),
            Constraint::Length(9),
        ])
        .split(columns[0]);
        let right =
            Layout::vertical([Constraint::Min(10), Constraint::Length(12)]).split(columns[1]);
        tool(frame, left[0], 3, state, app);
        tool(frame, left[1], 4, state, app);
        tool(frame, left[2], 0, state, app);
        tool(frame, right[0], 2, state, app);
        tool(frame, right[1], 1, state, app);
    }
    let hint = if app.form.is_some() {
        "Tab field · Ctrl-U clear · Enter accept · Esc cancel"
    } else {
        "Tab / 1–5 focus · F2 expand · Enter add/input · ? help · q quit"
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(hint, Style::default().fg(MUTED)),
            Line::styled(&app.status, Style::default().fg(ACCENT)),
        ])
        .wrap(Wrap { trim: true }),
        rows[2],
    );
}
fn tool(frame: &mut Frame, area: Rect, index: usize, state: &State, app: &App) {
    let block = panel(
        format!(
            " {:02} — {} ",
            TOOL_ORDER.iter().position(|i| *i == index).unwrap_or(0) + 1,
            NAMES[index]
        ),
        app.focus == index,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut body = inner;
    if app.focus == index {
        if let Some(form) = &app.form {
            let height = (form.values.len() * 2 + 1) as u16;
            let sections = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(height.min(inner.height)),
            ])
            .split(inner);
            body = sections[0];
            let mut lines = Vec::new();
            for (i, (label, value)) in form.labels.iter().zip(&form.values).enumerate() {
                lines.push(Line::styled(*label, Style::default().fg(PINK)));
                // Show the tail so newly typed text stays visible even in a narrow panel.
                let chars: Vec<char> = value.chars().collect();
                let tail: String = chars
                    .iter()
                    .skip(
                        chars
                            .len()
                            .saturating_sub(inner.width.saturating_sub(3) as usize),
                    )
                    .collect();
                lines.push(Line::from(format!(
                    "{} {}{}",
                    if i == form.field { ">" } else { " " },
                    tail,
                    if i == form.field { "▏" } else { "" }
                )));
            }
            frame.render_widget(Paragraph::new(lines), sections[1]);
        }
    }
    match index {
        0 => {
            let mut history_area = body;
            if body.width >= 46 && body.height >= 3 {
                let columns =
                    Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
                        .split(body);
                history_area = columns[1];
                let selected = state.history.get(app.selected[0]);
                let expression = selected.map_or("Enter to calculate", |h| h.expression.as_str());
                let result = selected.map_or_else(|| "0.00".into(), |h| state.number(h.result));
                frame.render_widget(
                    Paragraph::new(expression).style(Style::default().fg(MUTED)),
                    Rect {
                        height: 1,
                        ..columns[0]
                    },
                );
                let result_area = Rect {
                    y: columns[0].y + 1,
                    height: columns[0].height.saturating_sub(1),
                    ..columns[0]
                };
                if result_area.height >= 5 && large_width(&result) <= result_area.width as usize {
                    frame.render_widget(
                        Paragraph::new(large_number(&result))
                            .style(Style::default().fg(Color::Rgb(164, 223, 144))),
                        result_area,
                    );
                } else {
                    frame.render_widget(
                        Paragraph::new(format!("= {result}")).style(
                            Style::default()
                                .fg(Color::Rgb(164, 223, 144))
                                .add_modifier(Modifier::BOLD),
                        ),
                        result_area,
                    );
                }
            }
            let items: Vec<_> = state
                .history
                .iter()
                .map(|h| {
                    ListItem::new(vec![
                        Line::styled(h.expression.clone(), Style::default().fg(MUTED)),
                        Line::from(format!("= {}", state.number(h.result))),
                    ])
                })
                .collect();
            if items.is_empty() {
                frame.render_widget(
                    Paragraph::new("Enter to calculate\n125 * 1.15\n(12 + 8) * 50%\nans / 2")
                        .style(Style::default().fg(MUTED)),
                    history_area,
                );
            } else {
                draw_list(frame, history_area, items, app.selected[0]);
            }
        }
        1 => {
            let mut lines = vec![
                Line::styled(&app.length, Style::default().fg(ACCENT)),
                Line::raw(""),
            ];
            for unit in units::UNITS {
                if let Ok(n) = units::convert(&app.length, unit) {
                    lines.push(Line::from(format!("{:>14} {unit}", state.number(n))));
                }
            }
            if body.width >= 44 {
                let columns =
                    Layout::horizontal([Constraint::Percentage(57), Constraint::Percentage(43)])
                        .split(body);
                frame.render_widget(Paragraph::new(lines), columns[0]);
                frame.render_widget(
                    Paragraph::new(
                        "Quick refs\n\n1 in = 25.40 mm\n1 ft = 30.48 cm\n1 m = 100.00 cm",
                    )
                    .style(Style::default().fg(MUTED)),
                    columns[1],
                );
            } else {
                frame.render_widget(Paragraph::new(lines), body);
            }
        }
        2 => {
            let items = state
                .tasks
                .iter()
                .map(|t| {
                    ListItem::new(format!(
                        "[{}] {}",
                        if t.complete { "x" } else { " " },
                        t.text
                    ))
                    .style(Style::default().fg(if t.complete {
                        MUTED
                    } else {
                        CREAM
                    }))
                })
                .collect();
            draw_list(frame, body, items, app.selected[2]);
        }
        3 => {
            let featured = state
                .deadlines
                .iter()
                .find(|d| Some(d.id) == state.featured);
            let mut text = vec![
                Line::raw("No featured deadline"),
                Line::styled("a add · f feature", Style::default().fg(MUTED)),
            ];
            let mut height = 3;
            if let Some(d) = featured {
                let remaining = (d.at - Utc::now()).num_seconds();
                let n = remaining.unsigned_abs();
                let clock = format!(
                    "{:02}:{:02}:{:02}:{:02}",
                    n / 86400,
                    n / 3600 % 24,
                    n / 60 % 60,
                    n % 60
                );
                if body.height >= 12 && large_width(&clock) <= body.width as usize {
                    text = vec![Line::styled(
                        d.title.to_uppercase(),
                        Style::default().fg(CREAM),
                    )];
                    text.extend(large_number(&clock));
                    text.push(Line::styled(
                        " DAYS     HOURS     MINUTES   SECONDS",
                        Style::default().fg(ACCENT),
                    ));
                    text.push(Line::styled(
                        format!(
                            "{}{}",
                            if remaining < 0 { "OVERDUE · " } else { "" },
                            d.at.with_timezone(&Local).format("%a %d %b %Y  %H:%M %:z")
                        ),
                        Style::default().fg(MUTED),
                    ));
                    height = 9;
                } else {
                    text = vec![
                        Line::styled(&d.title, Style::default().fg(ACCENT)),
                        Line::from(countdown(d.at, Utc::now())),
                    ];
                }
            }
            text.push(Line::styled(
                "Upcoming deadlines",
                Style::default().fg(ACCENT),
            ));
            let rows = Layout::vertical([
                Constraint::Length(height.min(body.height)),
                Constraint::Min(0),
            ])
            .split(body);
            frame.render_widget(Paragraph::new(text).alignment(Alignment::Center), rows[0]);
            let items = state
                .deadlines
                .iter()
                .map(|d| {
                    ListItem::new(vec![
                        Line::raw(format!(
                            "{} {}",
                            if state.featured == Some(d.id) {
                                "*"
                            } else {
                                " "
                            },
                            d.title
                        )),
                        Line::styled(
                            d.at.with_timezone(&Local)
                                .format("%d %b %Y %H:%M %:z")
                                .to_string(),
                            Style::default().fg(MUTED),
                        ),
                    ])
                })
                .collect();
            draw_list(frame, rows[1], items, app.selected[3]);
        }
        _ => {
            let rows = if body.width >= 48 {
                Layout::horizontal([Constraint::Length(29), Constraint::Min(0)]).split(body)
            } else {
                Layout::vertical([Constraint::Length(9.min(body.height)), Constraint::Min(0)])
                    .split(body)
            };
            let today = Local::now().date_naive();
            let mut lines = vec![
                Line::styled(
                    format!("{:04} / {:02}   ← →", app.year, app.month),
                    Style::default().fg(ACCENT),
                ),
                Line::raw(" Mo  Tu  We  Th  Fr  Sa  Su"),
            ];
            if let Ok(cells) = calendar::month(app.year, app.month) {
                for week in cells.chunks(7) {
                    let spans: Vec<_> = week
                        .iter()
                        .map(|day| {
                            if let Some(day) = day {
                                let deadline = state
                                    .deadlines
                                    .iter()
                                    .any(|d| d.at.with_timezone(&Local).date_naive() == *day);
                                let entry = state.entries.iter().any(|e| e.date == *day);
                                let style = if *day == today {
                                    Style::default()
                                        .fg(BG)
                                        .bg(CREAM)
                                        .add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default().fg(if deadline {
                                        PINK
                                    } else if entry {
                                        ACCENT
                                    } else {
                                        CREAM
                                    })
                                };
                                Span::styled(
                                    format!(
                                        "{:3}{}",
                                        day.day(),
                                        if deadline {
                                            '*'
                                        } else if entry {
                                            '+'
                                        } else {
                                            ' '
                                        }
                                    ),
                                    style,
                                )
                            } else {
                                Span::raw("    ")
                            }
                        })
                        .collect();
                    lines.push(Line::from(spans));
                }
            }
            lines.push(Line::styled(
                "* deadline  + note",
                Style::default().fg(MUTED),
            ));
            frame.render_widget(Paragraph::new(lines), rows[0]);
            let items = state
                .entries
                .iter()
                .map(|e| ListItem::new(format!("{} {}", e.date, e.title)))
                .collect();
            draw_list(frame, rows[1], items, app.selected[4]);
        }
    }
}
fn glyph(c: char, row: usize) -> &'static str {
    let rows = match c {
        '0' => ["███", "█ █", "█ █", "█ █", "███"],
        '1' => [" ██", "  █", "  █", "  █", "  █"],
        '2' => ["███", "  █", "███", "█  ", "███"],
        '3' => ["███", "  █", "███", "  █", "███"],
        '4' => ["█ █", "█ █", "███", "  █", "  █"],
        '5' => ["███", "█  ", "███", "  █", "███"],
        '6' => ["███", "█  ", "███", "█ █", "███"],
        '7' => ["███", "  █", "  █", "  █", "  █"],
        '8' => ["███", "█ █", "███", "█ █", "███"],
        '9' => ["███", "█ █", "███", "  █", "███"],
        ':' => [" ", "█", " ", "█", " "],
        '.' => [" ", " ", " ", " ", "█"],
        '-' => ["   ", "   ", "███", "   ", "   "],
        _ => [" ", " ", " ", " ", " "],
    };
    rows[row]
}
fn large_width(text: &str) -> usize {
    text.chars()
        .map(|c| glyph(c, 0).chars().count() + 1)
        .sum::<usize>()
        .saturating_sub(1)
}
fn large_number(text: &str) -> Vec<Line<'static>> {
    (0..5)
        .map(|row| {
            Line::from(
                text.chars()
                    .map(|c| glyph(c, row))
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        })
        .collect()
}

fn draw_list(frame: &mut Frame, area: Rect, items: Vec<ListItem<'_>>, selected: usize) {
    if items.is_empty() {
        frame.render_widget(
            Paragraph::new("No items · a add").style(Style::default().fg(MUTED)),
            area,
        );
        return;
    }
    let mut selection = ListState::default().with_selected(Some(selected.min(items.len() - 1)));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::default().bg(Color::Rgb(45, 51, 64)))
            .highlight_symbol("› "),
        area,
        &mut selection,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    #[test]
    fn dashboard_renders_tiles_and_degrades_without_panics() {
        let mut state = State::default();
        for text in [
            "Sample: finish casting figures",
            "Sample: photograph sculptures",
            "Sample: prepare installation",
            "Sample: pack studio tools",
        ] {
            state.add_task(text).unwrap();
        }
        state.calculate("125 * 1.15").unwrap();
        state.calculate("200 / 0.94").unwrap();
        state
            .set_entry(
                None,
                "Sample: studio work",
                &Local::now().date_naive().to_string(),
            )
            .unwrap();
        state
            .set_deadline(
                None,
                "Sample: exhibition installation",
                "2026-12-01T18:00:00Z",
            )
            .unwrap();
        let mut app = App::new(&state);
        for (width, height) in [(120, 40), (100, 36), (90, 24), (60, 18), (32, 10), (10, 4)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| draw(frame, &state, &mut app))
                .unwrap();
            if width >= 100 && height >= 36 {
                let buffer = terminal.backend().buffer();
                let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
                for name in NAMES {
                    assert!(text.contains(name), "Missing {name}");
                }
            }
            if width == 120 {
                let svg = sample_svg(terminal.backend().buffer(), width, height);
                let output = std::env::var_os("CARGO_TARGET_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| std::path::PathBuf::from("target"));
                std::fs::create_dir_all(&output).unwrap();
                std::fs::write(output.join("sample-dashboard.svg"), &svg).unwrap();
            }
            app.begin(&state, false);
            terminal
                .draw(|frame| draw(frame, &state, &mut app))
                .unwrap();
            app.form = None;
        }
    }
    // Render the real Ratatui buffer as an inspectable, explicitly synthetic preview.
    fn sample_svg(buffer: &ratatui::buffer::Buffer, width: u16, height: u16) -> String {
        use std::fmt::Write;
        let color = |c: Color, fallback: &str| match c {
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            _ => fallback.to_owned(),
        };
        let mut svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\"><title>Jig dashboard — sample data</title><rect width=\"100%\" height=\"100%\" fill=\"#0c131b\"/><g font-family=\"IBM Plex Mono,DejaVu Sans Mono,monospace\" font-size=\"16\">\n", width * 10, height * 20, width * 10, height * 20);
        for (y, row) in buffer.content.chunks(width as usize).enumerate() {
            let mut x = 0;
            while x < row.len() {
                let start = x;
                let fg = row[x].fg;
                let bg = row[x].bg;
                let mut text = String::new();
                while x < row.len() && row[x].fg == fg && row[x].bg == bg {
                    text.push_str(row[x].symbol());
                    x += 1;
                }
                if bg != BG && bg != Color::Reset {
                    writeln!(
                        svg,
                        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"20\" fill=\"{}\"/>",
                        start * 10,
                        y * 20,
                        (x - start) * 10,
                        color(bg, "#0c131b")
                    )
                    .unwrap();
                }
                let text = text
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                writeln!(svg, "<text x=\"{}\" y=\"{}\" fill=\"{}\" xml:space=\"preserve\" textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\">{text}</text>", start * 10, y * 20 + 16, color(fg, "#f6daae"), (x - start) * 10).unwrap();
            }
        }
        svg.push_str("</g></svg>");
        svg
    }
    #[test]
    fn keyboard_edits_tasks_and_reuses_history() {
        let mut state = State::default();
        let mut app = App::new(&state);
        let press = |code| KeyEvent::new(code, KeyModifiers::NONE);
        app.key(press(KeyCode::Char('2')), &mut state).unwrap();
        app.key(press(KeyCode::Char('a')), &mut state).unwrap();
        for c in "Sample task".chars() {
            app.key(press(KeyCode::Char(c)), &mut state).unwrap();
        }
        assert!(app.key(press(KeyCode::Enter), &mut state).unwrap());
        app.form = None;
        app.key(press(KeyCode::Char(' ')), &mut state).unwrap();
        assert!(state.tasks[0].complete);
        app.key(press(KeyCode::Char('e')), &mut state).unwrap();
        app.key(press(KeyCode::Char('!')), &mut state).unwrap();
        app.key(press(KeyCode::Enter), &mut state).unwrap();
        app.form = None;
        assert_eq!(state.tasks[0].text, "Sample task!");
        state.calculate("1 / 3").unwrap();
        app.focus = 0;
        app.key(press(KeyCode::Char('r')), &mut state).unwrap();
        assert_eq!(
            app.form.as_ref().unwrap().values[0].parse::<f64>().unwrap(),
            1.0 / 3.0
        );
    }
}
