//! One input for ordinary arithmetic or a simple length conversion.
use crate::{calculator, units, Result};

#[derive(Clone, Debug)]
pub struct Evaluation {
    pub value: f64,
    pub unit: Option<String>,
}

/// Accepts `72 in mm`, `72in to mm`, and arrow-separated conversions.
/// `ans` remains the numeric value of the latest result, without dimensional algebra.
pub fn evaluate(input: &str, ans: f64) -> Result<Evaluation> {
    if input.len() > 4096 {
        return Err("Expression is too long (maximum 4096 bytes)".into());
    }
    if input
        .chars()
        .any(|c| c.is_control() && !c.is_ascii_whitespace())
    {
        return Err("Input must not contain control characters".into());
    }
    let normalized = input.replace("->", " to ").replace('→', " to ");
    // Arithmetic has no names except `ans`. A unit or conversion separator
    // therefore selects the length grammar without guessing at numeric values.
    let has_unit = normalized
        .replace("ans", "")
        .chars()
        .any(|c| c.is_ascii_alphabetic());
    if !has_unit {
        return Ok(Evaluation {
            value: calculator::evaluate(input, ans)?,
            unit: None,
        });
    }

    let mut words: Vec<_> = normalized.split_whitespace().collect();
    let target = words
        .pop()
        .ok_or("Use a source and target unit, e.g. 72 in mm")?;
    let target = units::canonical(target)?;
    if words
        .last()
        .is_some_and(|word| word.eq_ignore_ascii_case("to"))
    {
        words.pop();
    }
    let source = words.join(" ");
    let split = source
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_ascii_alphabetic())
        .map_or(0, |(index, c)| index + c.len_utf8());
    let number = source[..split].trim();
    let source_unit = units::canonical(&source[split..])?;
    if number.is_empty() {
        return Err("Include a length, e.g. 72 in mm".into());
    }
    let number = if number == "ans" {
        ans.to_string()
    } else {
        number.to_owned()
    };
    let value = units::convert(&format!("{number}{source_unit}"), target)?;
    Ok(Evaluation {
        value,
        unit: Some(target.to_owned()),
    })
}
