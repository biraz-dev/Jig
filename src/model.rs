use crate::{calculator, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Calculation {
    pub expression: String,
    pub result: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub text: String,
    pub complete: bool,
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
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            decimals: 2,
            focus: 3,
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
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err("Unsupported state version".into());
        }
        if self.preferences.decimals > 10 || self.preferences.focus > 4 {
            return Err("Invalid preferences".into());
        }
        if self.history.iter().any(|h| !h.result.is_finite()) {
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
        if self
            .featured
            .is_some_and(|id| !self.deadlines.iter().any(|d| d.id == id))
        {
            return Err("Featured deadline is missing".into());
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
        let result = calculator::evaluate(input, self.history.last().map_or(0.0, |h| h.result))?;
        self.history.push(Calculation {
            expression: input.to_owned(),
            result,
        });
        Ok(result)
    }
    pub fn add_task(&mut self, text: &str) -> Result<u64> {
        let id = self.id()?;
        self.tasks.push(Task {
            id,
            text: title(text)?,
            complete: false,
        });
        Ok(id)
    }
    pub fn edit_task(&mut self, id: u64, text: &str) -> Result<()> {
        let text = title(text)?;
        self.tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or("Task not found")?
            .text = text;
        Ok(())
    }
    pub fn toggle_task(&mut self, id: u64) -> Result<()> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or("Task not found")?;
        task.complete = !task.complete;
        Ok(())
    }
    pub fn remove_task(&mut self, id: u64) -> Result<()> {
        let index = self
            .tasks
            .iter()
            .position(|t| t.id == id)
            .ok_or("Task not found")?;
        self.tasks.remove(index);
        Ok(())
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
            let d = self
                .deadlines
                .iter_mut()
                .find(|d| d.id == id)
                .ok_or("Deadline not found")?;
            d.title = title;
            d.at = at;
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
