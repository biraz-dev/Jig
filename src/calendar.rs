use crate::Result;
use chrono::{Datelike, NaiveDate};

pub fn month(year: i32, month: u32) -> Result<Vec<Option<NaiveDate>>> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).ok_or("Invalid month")?;
    let mut cells = vec![None; first.weekday().num_days_from_monday() as usize];
    let mut day = first;
    loop {
        cells.push(Some(day));
        match day.succ_opt() {
            Some(next) if next.month() == month => day = next,
            _ => break,
        }
    }
    cells.resize(42, None);
    Ok(cells)
}
pub fn shift(year: i32, month: u32, delta: i32) -> Result<(i32, u32)> {
    NaiveDate::from_ymd_opt(year, month, 1).ok_or("Invalid month")?;
    let index = i64::from(year) * 12 + i64::from(month) - 1 + i64::from(delta);
    let y: i32 = index
        .div_euclid(12)
        .try_into()
        .map_err(|_| "Month outside supported range")?;
    let m = index.rem_euclid(12) as u32 + 1;
    NaiveDate::from_ymd_opt(y, m, 1).ok_or("Month outside supported range")?;
    Ok((y, m))
}
