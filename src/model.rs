use crate::{units, weather::WeatherConfig, workbench, Result};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Calculation {
    pub expression: String,
    pub result: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}
impl Calculation {
    pub fn formatted(&self, decimals: usize) -> String {
        let number = format!("{:.*}", decimals, self.result);
        match &self.unit {
            Some(unit) => format!("{number} {unit}"),
            None => number,
        }
    }

    /// Preserve the stored precision and, for lengths, the source unit.
    pub fn reusable_input(&self) -> String {
        match &self.unit {
            Some(unit) => format!("{} {unit}", self.result),
            None => self.result.to_string(),
        }
    }
}
/// A small named palette; rendering adapters choose its display colors.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskColor {
    Rose,
    Amber,
    Sage,
    Blue,
    Lilac,
}
impl TaskColor {
    pub const ALL: [Self; 5] = [Self::Rose, Self::Amber, Self::Sage, Self::Blue, Self::Lilac];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rose => "Rose",
            Self::Amber => "Amber",
            Self::Sage => "Sage",
            Self::Blue => "Blue",
            Self::Lilac => "Lilac",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub text: String,
    pub complete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<TaskColor>,
}
/// A read-only calendar view, derived from existing task/deadline records.
#[derive(Clone, Copy, Debug)]
pub struct CalendarTask<'a> {
    pub task_id: Option<u64>,
    pub deadline_id: u64,
    pub title: &'a str,
    pub complete: bool,
    pub color: Option<TaskColor>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Deadline {
    pub id: u64,
    pub title: String,
    pub at: DateTime<Utc>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub title: String,
    pub date: NaiveDate,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub decimals: usize,
    pub focus: usize,
    pub weather: Option<WeatherConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clay: Option<crate::clay::Settings>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            decimals: 2,
            focus: 3,
            weather: None,
            clay: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub version: u32,
    pub preferences: Preferences,
    pub history: Vec<Calculation>,
    pub tasks: Vec<Task>,
    pub deadlines: Vec<Deadline>,
    pub entries: Vec<Entry>,
    pub featured: Option<u64>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            version: 1,
            preferences: Preferences::default(),
            history: vec![],
            tasks: vec![],
            deadlines: vec![],
            entries: vec![],
            featured: None,
        }
    }
}
pub fn title(text: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err("Text must be nonempty and contain no control characters".into());
    }
    Ok(text.to_owned())
}
pub fn parse_deadline(text: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|_| "Use RFC3339 with timezone, e.g. 2026-12-01T18:00:00+01:00".into())
}
pub fn countdown(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (at - now).num_seconds();
    let n = seconds.unsigned_abs();
    format!(
        "{}{}d {:02}h {:02}m {:02}s",
        if seconds < 0 { "Overdue " } else { "" },
        n / 86400,
        n / 3600 % 24,
        n / 60 % 60,
        n % 60
    )
}
impl State {
    /// Decode the portable file format. Unrelated JSON must not become empty state.
    pub fn from_json(text: &str) -> Result<Self> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("Invalid state file: {e}"))?;
        let fields = value.as_object().ok_or("Expected a Jig JSON object")?;
        for key in [
            "version",
            "preferences",
            "history",
            "tasks",
            "deadlines",
            "entries",
            "featured",
        ] {
            if !fields.contains_key(key) {
                return Err(format!("Not a complete Jig state file: missing {key}"));
            }
        }
        let state: Self =
            serde_json::from_value(value).map_err(|e| format!("Invalid state file: {e}"))?;
        state.validate()?;
        Ok(state)
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err("Unsupported state version".into());
        }
        if self.preferences.decimals > 10 || self.preferences.focus > 4 {
            return Err("Invalid preferences".into());
        }
        if let Some(config) = &self.preferences.weather {
            config.validate()?;
        }
        if let Some(settings) = &self.preferences.clay {
            settings.validate()?;
        }
        if self.history.iter().any(|h| {
            !h.result.is_finite()
                || h.expression.len() > 4096
                || h.unit
                    .as_deref()
                    .is_some_and(|unit| !units::UNITS.contains(&unit))
                || h.expression
                    .chars()
                    .any(|c| c.is_control() && !c.is_ascii_whitespace())
        }) {
            return Err("Invalid calculation history".into());
        }
        let ids: Vec<_> = self
            .tasks
            .iter()
            .map(|t| t.id)
            .chain(self.deadlines.iter().map(|d| d.id))
            .chain(self.entries.iter().map(|e| e.id))
            .collect();
        let mut unique = std::collections::HashSet::new();
        if ids.iter().any(|id| *id == 0 || !unique.insert(*id)) {
            return Err("Invalid or duplicate item IDs".into());
        }
        for text in self
            .tasks
            .iter()
            .map(|t| &t.text)
            .chain(self.deadlines.iter().map(|d| &d.title))
            .chain(self.entries.iter().map(|e| &e.title))
        {
            title(text)?;
        }
        let mut linked = std::collections::HashSet::new();
        for task in &self.tasks {
            if let Some(deadline) = task.deadline {
                let item = self
                    .deadlines
                    .iter()
                    .find(|item| item.id == deadline)
                    .ok_or("Task deadline is missing or linked to more than one task")?;
                if !linked.insert(deadline) {
                    return Err("Task deadline is missing or linked to more than one task".into());
                }
                if task.start.is_some_and(|start| start > item.at) {
                    return Err("Task start must be on or before its deadline".into());
                }
            } else if task.start.is_some() {
                return Err("Add a deadline before setting a task start".into());
            }
        }
        if self.featured.is_some_and(|id| !self.is_deadline_active(id)) {
            return Err("Featured deadline is missing or completed".into());
        }
        Ok(())
    }
    fn id(&self) -> Result<u64> {
        self.tasks
            .iter()
            .map(|t| t.id)
            .chain(self.deadlines.iter().map(|d| d.id))
            .chain(self.entries.iter().map(|e| e.id))
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("No available IDs".into())
    }
    pub fn number(&self, n: f64) -> String {
        format!("{:.*}", self.preferences.decimals, n)
    }
    pub fn calculate(&mut self, input: &str) -> Result<f64> {
        let evaluated = workbench::evaluate(input, self.history.last().map_or(0.0, |h| h.result))?;
        let result = evaluated.value;
        self.history.push(Calculation {
            expression: input.to_owned(),
            result,
            unit: evaluated.unit,
        });
        Ok(result)
    }
    pub fn add_task(&mut self, text: &str) -> Result<u64> {
        let id = self.id()?;
        self.tasks.push(Task {
            id,
            text: title(text)?,
            complete: false,
            deadline: None,
            start: None,
            color: None,
        });
        Ok(id)
    }
    pub fn edit_task(&mut self, id: u64, text: &str) -> Result<()> {
        let text = title(text)?;
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or("Task not found")?;
        task.text = text.clone();
        if let Some(deadline) = task.deadline {
            if let Some(item) = self.deadlines.iter_mut().find(|item| item.id == deadline) {
                item.title = text;
            }
        }
        Ok(())
    }
    pub fn toggle_task(&mut self, id: u64) -> Result<()> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or("Task not found")?;
        task.complete = !task.complete;
        if task.complete && task.deadline.is_some() && self.featured == task.deadline {
            self.featured = None;
        }
        Ok(())
    }
    pub fn remove_task(&mut self, id: u64) -> Result<()> {
        let index = self
            .tasks
            .iter()
            .position(|t| t.id == id)
            .ok_or("Task not found")?;
        if let Some(deadline) = self.tasks[index].deadline {
            self.remove_deadline(deadline)?;
        }
        self.tasks.remove(index);
        Ok(())
    }
    pub fn remove_completed_tasks(&mut self) -> usize {
        let before = self.tasks.len();
        let removed: Vec<_> = self
            .tasks
            .iter()
            .filter(|task| task.complete)
            .filter_map(|task| task.deadline)
            .collect();
        self.tasks.retain(|task| !task.complete);
        self.deadlines
            .retain(|deadline| !removed.contains(&deadline.id));
        if self.featured.is_some_and(|id| removed.contains(&id)) {
            self.featured = None;
        }
        before - self.tasks.len()
    }
    pub fn task_deadline(&self, id: u64) -> Option<&Deadline> {
        let deadline = self.tasks.iter().find(|task| task.id == id)?.deadline?;
        self.deadlines.iter().find(|item| item.id == deadline)
    }
    /// Color belongs to the task and survives completion or temporarily removing its date.
    pub fn set_task_color(&mut self, id: u64, color: Option<TaskColor>) -> Result<()> {
        let index = self
            .tasks
            .iter()
            .position(|task| task.id == id)
            .ok_or("Task not found")?;
        if color.is_some() && self.task_deadline(id).is_none() {
            return Err("Add a date before assigning a task color".into());
        }
        self.tasks[index].color = color;
        Ok(())
    }
    /// Only the due date is represented, including completed tasks and legacy deadlines.
    /// This projection never materializes tasks or changes the featured countdown.
    pub fn tasks_on_date<Tz: TimeZone>(
        &self,
        date: NaiveDate,
        timezone: &Tz,
    ) -> Vec<CalendarTask<'_>> {
        self.deadlines
            .iter()
            .filter(|deadline| deadline.at.with_timezone(timezone).date_naive() == date)
            .map(|deadline| {
                let task = self
                    .tasks
                    .iter()
                    .find(|task| task.deadline == Some(deadline.id));
                CalendarTask {
                    task_id: task.map(|task| task.id),
                    deadline_id: deadline.id,
                    title: task.map_or(deadline.title.as_str(), |task| task.text.as_str()),
                    complete: task.is_some_and(|task| task.complete),
                    color: task.and_then(|task| task.color),
                }
            })
            .collect()
    }
    /// A date belongs to its checklist task; assigning one does not feature it.
    pub fn set_task_deadline(&mut self, id: u64, at: Option<&str>) -> Result<()> {
        let task = self
            .tasks
            .iter()
            .find(|task| task.id == id)
            .ok_or("Task not found")?;
        let start = at.and(task.start).map(|start| start.to_rfc3339());
        self.set_task_schedule(id, start.as_deref(), at)
    }
    /// Set a start without replacing or copying the existing linked deadline.
    pub fn set_task_start(&mut self, id: u64, start: Option<&str>) -> Result<()> {
        if !self.tasks.iter().any(|task| task.id == id) {
            return Err("Task not found".into());
        }
        let due = self
            .task_deadline(id)
            .map(|deadline| deadline.at.to_rfc3339());
        self.set_task_schedule(id, start, due.as_deref())
    }
    /// Validate both instants before changing either part of a task's schedule.
    /// UI adapters can expose dates while retaining an existing deadline's time.
    pub fn set_task_schedule(
        &mut self,
        id: u64,
        start: Option<&str>,
        at: Option<&str>,
    ) -> Result<()> {
        let start = start.map(parse_deadline).transpose()?;
        let at = at.map(parse_deadline).transpose()?;
        if let Some(start) = start {
            let due = at.ok_or("Add a deadline before setting a task start")?;
            if start > due {
                return Err("Task start must be on or before its deadline".into());
            }
        }
        let index = self
            .tasks
            .iter()
            .position(|task| task.id == id)
            .ok_or("Task not found")?;
        if let Some(at) = at {
            let title = self.tasks[index].text.clone();
            if let Some(deadline) = self.tasks[index].deadline {
                let item = self
                    .deadlines
                    .iter_mut()
                    .find(|item| item.id == deadline)
                    .ok_or("Task deadline not found")?;
                item.title = title;
                item.at = at;
            } else {
                let deadline = self.id()?;
                self.deadlines.push(Deadline {
                    id: deadline,
                    title,
                    at,
                });
                self.tasks[index].deadline = Some(deadline);
            }
        } else if let Some(deadline) = self.tasks[index].deadline {
            self.remove_deadline(deadline)?;
        }
        self.tasks[index].start = start;
        Ok(())
    }
    pub fn feature_task(&mut self, id: u64) -> Result<()> {
        let task = self
            .tasks
            .iter()
            .find(|task| task.id == id)
            .ok_or("Task not found")?;
        if task.complete {
            return Err("Completed tasks cannot be featured".into());
        }
        let deadline = task
            .deadline
            .ok_or("Add a date before featuring this task")?;
        self.feature(deadline)
    }
    /// Keep old deadline-only files intact until the user acts on that row.
    pub fn ensure_task_for_deadline(&mut self, id: u64) -> Result<u64> {
        if let Some(task) = self.tasks.iter().find(|task| task.deadline == Some(id)) {
            return Ok(task.id);
        }
        let text = self
            .deadlines
            .iter()
            .find(|deadline| deadline.id == id)
            .ok_or("Deadline not found")?
            .title
            .clone();
        let task = self.add_task(&text)?;
        self.tasks.last_mut().ok_or("Task not found")?.deadline = Some(id);
        Ok(task)
    }
    pub fn is_deadline_active(&self, id: u64) -> bool {
        self.deadlines.iter().any(|deadline| deadline.id == id)
            && !self
                .tasks
                .iter()
                .any(|task| task.deadline == Some(id) && task.complete)
    }
    pub fn move_task(&mut self, id: u64, position: usize) -> Result<()> {
        if position == 0 || position > self.tasks.len() {
            return Err("Position is 1-based and must be in the task list".into());
        }
        let index = self
            .tasks
            .iter()
            .position(|t| t.id == id)
            .ok_or("Task not found")?;
        let task = self.tasks.remove(index);
        self.tasks.insert(position - 1, task);
        Ok(())
    }
    pub fn set_deadline(&mut self, id: Option<u64>, text: &str, at: &str) -> Result<u64> {
        let title = title(text)?;
        let at = parse_deadline(at)?;
        if let Some(id) = id {
            if self
                .tasks
                .iter()
                .any(|task| task.deadline == Some(id) && task.start.is_some_and(|start| start > at))
            {
                return Err("Task start must be on or before its deadline".into());
            }
            let d = self
                .deadlines
                .iter_mut()
                .find(|d| d.id == id)
                .ok_or("Deadline not found")?;
            d.title = title.clone();
            d.at = at;
            if let Some(task) = self.tasks.iter_mut().find(|task| task.deadline == Some(id)) {
                task.text = title;
            }
            Ok(id)
        } else {
            let id = self.id()?;
            self.deadlines.push(Deadline { id, title, at });
            if self.featured.is_none() {
                self.featured = Some(id);
            }
            Ok(id)
        }
    }
    pub fn feature(&mut self, id: u64) -> Result<()> {
        if !self.deadlines.iter().any(|d| d.id == id) {
            return Err("Deadline not found".into());
        }
        if !self.is_deadline_active(id) {
            return Err("Completed tasks cannot be featured".into());
        }
        self.featured = Some(id);
        Ok(())
    }
    pub fn remove_deadline(&mut self, id: u64) -> Result<()> {
        let i = self
            .deadlines
            .iter()
            .position(|d| d.id == id)
            .ok_or("Deadline not found")?;
        self.deadlines.remove(i);
        for task in &mut self.tasks {
            if task.deadline == Some(id) {
                task.deadline = None;
                task.start = None;
            }
        }
        if self.featured == Some(id) {
            self.featured = None;
        }
        Ok(())
    }
    pub fn set_entry(&mut self, id: Option<u64>, text: &str, date: &str) -> Result<u64> {
        let title = title(text)?;
        let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| "Use YYYY-MM-DD")?;
        if let Some(id) = id {
            let e = self
                .entries
                .iter_mut()
                .find(|e| e.id == id)
                .ok_or("Entry not found")?;
            e.title = title;
            e.date = date;
            Ok(id)
        } else {
            let id = self.id()?;
            self.entries.push(Entry { id, title, date });
            Ok(id)
        }
    }
    pub fn remove_entry(&mut self, id: u64) -> Result<()> {
        let i = self
            .entries
            .iter()
            .position(|e| e.id == id)
            .ok_or("Entry not found")?;
        self.entries.remove(i);
        Ok(())
    }
}
