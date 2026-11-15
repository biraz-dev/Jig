use chrono::{
    DateTime, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike,
    Utc,
};
use jig::task_dates;

fn instant(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

fn nine() -> NaiveTime {
    NaiveTime::from_hms_opt(9, 0, 0).unwrap()
}

#[test]
fn calendar_dates_use_local_defaults_and_keep_existing_precision() {
    let zone = FixedOffset::west_opt(5 * 3600).unwrap();
    assert_eq!(
        task_dates::resolve("2026-11-20", None, nine(), &zone).unwrap(),
        instant("2026-11-20T09:00:00-05:00")
    );
    let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
    assert_eq!(
        task_dates::resolve("2026-11-20", None, midnight, &zone).unwrap(),
        instant("2026-11-20T00:00:00-05:00")
    );
    let existing = instant("2026-11-20T17:31:42.123456789-05:00");
    for input in ["2026-11-20", "2026-11-20T17:31", "2026-11-20 17:31"] {
        assert_eq!(
            task_dates::resolve(input, Some(existing), nine(), &zone).unwrap(),
            existing
        );
    }
    for input in ["2026-11-22", "2026-11-22T17:31"] {
        assert_eq!(
            task_dates::resolve(input, Some(existing), nine(), &zone).unwrap(),
            instant("2026-11-22T17:31:42.123456789-05:00")
        );
    }
    assert_eq!(
        task_dates::resolve("2026-11-22T18:15", Some(existing), nine(), &zone).unwrap(),
        instant("2026-11-22T18:15:00-05:00")
    );
    assert_eq!(
        task_dates::resolve("2026-11-22 18:15:03.25", Some(existing), nine(), &zone).unwrap(),
        instant("2026-11-22T18:15:03.25-05:00")
    );
}

// Deterministic US Eastern transitions avoid a global process timezone or
// another timezone dependency while exercising real gap/fold semantics.
#[derive(Clone, Copy, Debug)]
struct Eastern2026;

fn offset(daylight: bool) -> FixedOffset {
    FixedOffset::west_opt(if daylight { 4 * 3600 } else { 5 * 3600 }).unwrap()
}

impl TimeZone for Eastern2026 {
    type Offset = FixedOffset;

    fn from_offset(_: &Self::Offset) -> Self {
        Self
    }

    fn offset_from_local_date(&self, value: &NaiveDate) -> LocalResult<Self::Offset> {
        self.offset_from_local_datetime(&value.and_hms_opt(12, 0, 0).unwrap())
    }

    fn offset_from_local_datetime(&self, value: &NaiveDateTime) -> LocalResult<Self::Offset> {
        if value.date() == date("2026-03-08") && value.hour() == 2 {
            return LocalResult::None;
        }
        if value.date() == date("2026-11-01") && value.hour() == 1 {
            return LocalResult::Ambiguous(offset(true), offset(false));
        }
        LocalResult::Single(offset(
            *value >= date("2026-03-08").and_hms_opt(3, 0, 0).unwrap()
                && *value < date("2026-11-01").and_hms_opt(1, 0, 0).unwrap(),
        ))
    }

    fn offset_from_utc_date(&self, value: &NaiveDate) -> Self::Offset {
        self.offset_from_utc_datetime(&value.and_hms_opt(12, 0, 0).unwrap())
    }

    fn offset_from_utc_datetime(&self, value: &NaiveDateTime) -> Self::Offset {
        offset(
            *value >= date("2026-03-08").and_hms_opt(7, 0, 0).unwrap()
                && *value < date("2026-11-01").and_hms_opt(6, 0, 0).unwrap(),
        )
    }
}

#[test]
fn local_calendar_edits_follow_dst_and_reject_nonexistent_times() {
    let existing = instant("2026-03-07T09:30:12.5-05:00");
    let next = task_dates::resolve("2026-03-08", Some(existing), nine(), &Eastern2026).unwrap();
    assert_eq!(next, instant("2026-03-08T09:30:12.5-04:00"));
    assert_eq!((next - existing).num_hours(), 23);
    for input in ["2026-03-08T02:30", "2026-03-08 02:30:00"] {
        assert!(task_dates::resolve(input, None, nine(), &Eastern2026)
            .unwrap_err()
            .contains("does not exist"));
    }
    // An explicit offset is an unambiguous instant, independent of local gaps.
    assert_eq!(
        task_dates::resolve("2026-03-08T02:30:00-05:00", None, nine(), &Eastern2026).unwrap(),
        instant("2026-03-08T02:30:00-05:00")
    );
}

#[test]
fn repeated_hours_keep_an_existing_exact_instant_and_new_times_choose_earliest() {
    let second_fold = instant("2026-11-01T01:30:45.123456789-05:00");
    for input in [
        "2026-11-01",
        "2026-11-01T01:30",
        "2026-11-01 01:30:45.123456789",
    ] {
        assert_eq!(
            task_dates::resolve(input, Some(second_fold), nine(), &Eastern2026).unwrap(),
            second_fold
        );
    }
    assert_eq!(
        task_dates::resolve("2026-11-01T01:30", None, nine(), &Eastern2026).unwrap(),
        instant("2026-11-01T01:30:00-04:00")
    );
    assert_eq!(
        task_dates::resolve("2026-11-01T01:31", Some(second_fold), nine(), &Eastern2026).unwrap(),
        instant("2026-11-01T01:31:00-04:00")
    );
}

#[test]
fn malformed_dates_and_times_have_predictable_errors() {
    for input in [
        "",
        "tomorrow",
        "2026-02-30",
        "2026-11-01T24:00",
        "2026-11-01T12:60",
        "2026-11-01T12:30junk",
    ] {
        assert!(
            task_dates::resolve(input, None, nine(), &Utc).is_err(),
            "{input}"
        );
    }
}
