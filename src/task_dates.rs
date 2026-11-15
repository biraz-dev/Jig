//! Local calendar edits resolve to the same precise instants as typed dates.
use crate::Result;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc};

/// Resolve a date, local date/time, or an explicit RFC3339 instant.
/// Date-only edits retain an existing local time. A minute-precision edit that
/// leaves the displayed time unchanged also retains its seconds and subseconds.
/// An unchanged local date/time retains the exact instant in a repeated DST hour.
pub fn resolve<Tz: TimeZone>(
    input: &str,
    existing: Option<DateTime<Utc>>,
    default_time: NaiveTime,
    timezone: &Tz,
) -> Result<DateTime<Utc>> {
    let input = input.trim();
    if let Ok(instant) = DateTime::parse_from_rfc3339(input) {
        return Ok(instant.with_timezone(&Utc));
    }
    let previous = existing.map(|instant| instant.with_timezone(timezone));
    let local = if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        date.and_time(previous.as_ref().map_or(default_time, DateTime::time))
    } else if let Some(local) = ["%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M"]
        .into_iter()
        .find_map(|format| NaiveDateTime::parse_from_str(input, format).ok())
    {
        let time = previous
            .as_ref()
            .filter(|previous| {
                previous.hour() == local.hour() && previous.minute() == local.minute()
            })
            .map_or(local.time(), DateTime::time);
        local.date().and_time(time)
    } else {
        ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"]
            .into_iter()
            .find_map(|format| NaiveDateTime::parse_from_str(input, format).ok())
            .ok_or("Use YYYY-MM-DD, YYYY-MM-DD HH:MM, or a timestamp with a timezone")?
    };
    if let Some(previous) = previous.filter(|previous| previous.naive_local() == local) {
        return Ok(previous.with_timezone(&Utc));
    }
    timezone
        .from_local_datetime(&local)
        .earliest()
        .map(|instant| instant.with_timezone(&Utc))
        .ok_or_else(|| {
            "That time does not exist locally. Choose another time or enter a timestamp with a timezone."
                .into()
        })
}
