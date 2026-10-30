//! Synthetic preview data. This module never reads or writes personal state.
use crate::model::{State, TaskColor};
use chrono::{DateTime, Duration, Utc};

/// Build repeatable sample data around a supplied clock value.
/// Callers keep this separate from the user's persisted state.
pub fn state(now: DateTime<Utc>) -> State {
    let mut state = State::default();
    for expression in ["1 / 3", "125 * 1.15", "200 / 0.94"] {
        state
            .calculate(expression)
            .expect("Built-in sample expression is valid");
    }
    for text in [
        "Sample: finish casting studies",
        "Sample: photograph sculptures",
        "Sample: prepare installation hardware",
        "Sample: order studio supplies",
        "Sample: pack studio tools",
    ] {
        let id = state.add_task(text).expect("Built-in sample task is valid");
        if text == "Sample: order studio supplies" {
            state.toggle_task(id).expect("Sample task was just added");
        }
    }
    for (title, offset, color) in [
        (
            "Sample: final casting",
            Duration::days(8) + Duration::hours(14),
            TaskColor::Rose,
        ),
        (
            "Sample: photography",
            Duration::days(13) + Duration::hours(15),
            TaskColor::Amber,
        ),
        (
            "Sample: exhibition installation",
            Duration::days(24)
                + Duration::hours(17)
                + Duration::minutes(30)
                + Duration::seconds(42),
            TaskColor::Sage,
        ),
    ] {
        let at = now.checked_add_signed(offset).unwrap_or(now);
        let id = state
            .set_deadline(None, title, &at.to_rfc3339())
            .expect("Built-in sample deadline is valid");
        let task = state
            .ensure_task_for_deadline(id)
            .expect("Sample deadline was just added");
        state
            .set_task_color(task, Some(color))
            .expect("Sample task has a date");
        state.feature(id).expect("Sample deadline was just added");
    }
    for title in ["Sample: studio work", "Sample: mould preparation"] {
        state
            .set_entry(None, title, &now.date_naive().to_string())
            .expect("Built-in sample calendar entry is valid");
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::countdown;
    use chrono::TimeZone;

    #[test]
    fn samples_are_deterministic_valid_and_labelled() {
        let now = Utc.with_ymd_and_hms(2026, 10, 30, 18, 45, 55).unwrap();
        let first = state(now);
        first.validate().unwrap();
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(state(now)).unwrap()
        );
        assert!(first.tasks.iter().all(|t| t.text.starts_with("Sample:")));
        assert!(first
            .deadlines
            .iter()
            .all(|d| d.title.starts_with("Sample:")));
        assert!(first.entries.iter().all(|e| e.title.starts_with("Sample:")));
        assert!(first.tasks.iter().any(|t| t.complete));
        assert!(first.tasks.iter().any(|t| !t.complete));
        assert_eq!(first.history[0].result, 1.0 / 3.0);
    }

    #[test]
    fn dates_follow_the_supplied_clock_and_feature_the_exhibition() {
        let now = Utc.with_ymd_and_hms(2026, 12, 31, 23, 59, 59).unwrap();
        let samples = state(now);
        assert_eq!(samples.deadlines.len(), 3);
        let featured = samples
            .deadlines
            .iter()
            .find(|d| Some(d.id) == samples.featured)
            .unwrap();
        assert_eq!(featured.title, "Sample: exhibition installation");
        assert_eq!(countdown(featured.at, now), "24d 17h 30m 42s");
        assert!(samples.entries.iter().all(|e| e.date == now.date_naive()));
        assert!(samples.deadlines.iter().all(|d| d.at > now));
    }
}
