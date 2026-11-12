//! A day-based projection of the existing checklist and deadline records.
//! Nothing in this module changes state or maintains a second set of tasks.
use crate::{
    calendar,
    model::{State, TaskColor},
    Result,
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scale {
    Week,
    Month,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DateRange {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResizeEdge {
    Start,
    Due,
}

/// Propose one day-based edge edit for an existing task with a start and due date.
/// The opposite instant stays exact. The edited edge keeps its local clock time,
/// except when clamped against the opposite edge to keep the schedule ordered.
///
/// An unchanged local date preserves the original instant, including the second
/// occurrence of an ambiguous clock time. New ambiguous times choose the earlier
/// instant; nonexistent times and local or UTC dates outside years 1 through
/// 9999 are errors, so returned UTC instants remain portable schedule inputs.
/// Callers can apply the returned pair once through `State::set_task_schedule`.
pub fn resize_schedule<Tz: TimeZone>(
    state: &State,
    task_id: u64,
    edge: ResizeEdge,
    date: NaiveDate,
    timezone: &Tz,
) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let outside = "Task schedule is outside the supported date range";
    if !(1..=9999).contains(&date.year()) {
        return Err(outside.into());
    }
    let task = state
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or("Task not found")?;
    let ranged = "Only tasks with a start and deadline can be resized";
    let start = task.start.ok_or(ranged)?;
    let due = state.task_deadline(task_id).ok_or(ranged)?.at;
    if start > due {
        return Err("Task start must be on or before its deadline".into());
    }
    let local_time = |instant: DateTime<Utc>| -> Result<NaiveDateTime> {
        let utc = instant.naive_utc();
        let local = utc
            .checked_add_offset(timezone.offset_from_utc_datetime(&utc).fix())
            .ok_or(outside)?;
        if !(1..=9999).contains(&local.year()) {
            return Err(outside.into());
        }
        Ok(local)
    };
    let start_local = local_time(start)?;
    let due_local = local_time(due)?;
    let on_date =
        |instant: DateTime<Utc>, local: NaiveDateTime, date: NaiveDate| -> Result<DateTime<Utc>> {
            if date == local.date() {
                return Ok(instant);
            }
            timezone
                .from_local_datetime(&date.and_time(local.time()))
                .earliest()
                .map(|changed| changed.with_timezone(&Utc))
                .ok_or_else(|| "This clock time does not exist on the selected local date".into())
        };
    let resized = match edge {
        ResizeEdge::Start => {
            let changed = on_date(start, start_local, date.min(due_local.date()))?;
            (changed.min(due), due)
        }
        ResizeEdge::Due => {
            let changed = on_date(due, due_local, date.max(start_local.date()))?;
            (start, changed.max(start))
        }
    };
    if !(1..=9999).contains(&resized.0.year()) || !(1..=9999).contains(&resized.1.year()) {
        return Err(outside.into());
    }
    Ok(resized)
}

impl DateRange {
    /// Number of calendar days, including both endpoints. Invalid ranges are empty.
    pub fn days(self) -> u32 {
        if self.end < self.start {
            0
        } else {
            (self.end - self.start).num_days() as u32 + 1
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TimelineRow<'a> {
    pub task_id: Option<u64>,
    pub deadline_id: Option<u64>,
    pub title: &'a str,
    pub complete: bool,
    pub color: Option<TaskColor>,
    pub start: Option<NaiveDate>,
    pub due: Option<NaiveDate>,
}

/// Checklist order is retained, including undated and completed tasks. Legacy
/// deadline-only records follow them without being converted into tasks.
pub fn rows<'a, Tz: TimeZone>(state: &'a State, timezone: &Tz) -> Vec<TimelineRow<'a>> {
    let mut rows: Vec<_> = state
        .tasks
        .iter()
        .map(|task| TimelineRow {
            task_id: Some(task.id),
            deadline_id: task.deadline,
            title: &task.text,
            complete: task.complete,
            color: task.color,
            start: task
                .start
                .map(|start| start.with_timezone(timezone).date_naive()),
            due: state
                .task_deadline(task.id)
                .map(|deadline| deadline.at.with_timezone(timezone).date_naive()),
        })
        .collect();
    for deadline in &state.deadlines {
        if state
            .tasks
            .iter()
            .any(|task| task.deadline == Some(deadline.id))
        {
            continue;
        }
        rows.push(TimelineRow {
            task_id: None,
            deadline_id: Some(deadline.id),
            title: &deadline.title,
            complete: false,
            color: None,
            start: None,
            due: Some(deadline.at.with_timezone(timezone).date_naive()),
        });
    }
    rows
}

/// Monday through Sunday, or the full calendar month containing the anchor.
pub fn window(anchor: NaiveDate, scale: Scale) -> Result<DateRange> {
    let outside = "Timeline window is outside the supported date range";
    let (start, end) = match scale {
        Scale::Week => {
            let start = anchor
                .checked_sub_signed(Duration::days(i64::from(
                    anchor.weekday().num_days_from_monday(),
                )))
                .ok_or(outside)?;
            let end = start.checked_add_signed(Duration::days(6)).ok_or(outside)?;
            (start, end)
        }
        Scale::Month => {
            let start = anchor.with_day(1).ok_or(outside)?;
            let (year, month) = calendar::shift(anchor.year(), anchor.month(), 1)?;
            let end = NaiveDate::from_ymd_opt(year, month, 1)
                .and_then(|date| date.pred_opt())
                .ok_or(outside)?;
            (start, end)
        }
    };
    Ok(DateRange { start, end })
}

/// Return the first day of the shifted window. Arithmetic is checked at the
/// calendar bounds so repeated navigation never panics.
pub fn shift(anchor: NaiveDate, scale: Scale, delta: i32) -> Result<NaiveDate> {
    let start = window(anchor, scale)?.start;
    let shifted = match scale {
        Scale::Week => start
            .checked_add_signed(Duration::days(i64::from(delta) * 7))
            .ok_or("Timeline window is outside the supported date range")?,
        Scale::Month => {
            let (year, month) = calendar::shift(start.year(), start.month(), delta)?;
            NaiveDate::from_ymd_opt(year, month, 1)
                .ok_or("Timeline window is outside the supported date range")?
        }
    };
    Ok(window(shifted, scale)?.start)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisibleSpan {
    /// Zero-based day offset from the window's first date.
    pub offset: u32,
    /// Inclusive number of visible days. A deadline-only marker occupies one day.
    pub days: u32,
    pub milestone: bool,
    pub clipped_start: bool,
    pub clipped_end: bool,
}

/// Clip a scheduled bar or deadline-only marker to the visible day range.
/// An undated task or a row outside the window has no visible span.
pub fn clipped(row: &TimelineRow<'_>, range: DateRange) -> Option<VisibleSpan> {
    if range.days() == 0 {
        return None;
    }
    let due = row.due?;
    let start = row.start.unwrap_or(due);
    if start > due || due < range.start || start > range.end {
        return None;
    }
    let visible_start = start.max(range.start);
    let visible_end = due.min(range.end);
    Some(VisibleSpan {
        offset: (visible_start - range.start).num_days() as u32,
        days: (visible_end - visible_start).num_days() as u32 + 1,
        milestone: row.start.is_none(),
        clipped_start: start < range.start,
        clipped_end: due > range.end,
    })
}
