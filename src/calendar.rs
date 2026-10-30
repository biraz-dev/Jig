use chrono::{Datelike, NaiveDate};
use crate::Result;

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
    let index = year * 12 + month as i32 - 1 + delta;
    let (y, m) = (index.div_euclid(12), index.rem_euclid(12) as u32 + 1);
    NaiveDate::from_ymd_opt(y, m, 1).ok_or("Month outside supported range")?;
    Ok((y, m))
}
