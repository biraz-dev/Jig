use crate::Result;

pub const UNITS: [&str; 5] = ["in", "ft", "mm", "cm", "m"];
fn scale(unit: &str) -> Result<f64> {
    match unit.trim() {
        "in" | "inch" | "inches" => Ok(0.0254),
        "ft" | "foot" | "feet" => Ok(0.3048),
        "mm" => Ok(0.001),
        "cm" => Ok(0.01),
        "m" => Ok(1.0),
        _ => Err("Unit must be in, ft, mm, cm or m".into()),
    }
}
pub fn convert(input: &str, target: &str) -> Result<f64> {
    let input = input.trim();
    let split = input
        .find(|c: char| c.is_ascii_alphabetic())
        .ok_or("Include a source unit, e.g. 12in")?;
    let number = input[..split].trim();
    let source = scale(&input[split..])?;
    let mut parts = number.split_whitespace();
    let first = parts.next().ok_or("Missing length")?;
    let value = if let Some(fraction) = parts.next() {
        if parts.next().is_some() {
            return Err("Use a decimal, fraction or mixed fraction".into());
        }
        let whole = first.parse::<f64>().map_err(|_| "Invalid whole number")?;
        whole
            + if first.starts_with('-') {
                -fraction_value(fraction)?
            } else {
                fraction_value(fraction)?
            }
    } else {
        fraction_value(first)?
    };
    let result = value * source / scale(target)?;
    if !result.is_finite() {
        return Err("Length must be finite".into());
    }
    Ok(result)
}
fn fraction_value(s: &str) -> Result<f64> {
    if let Some((n, d)) = s.split_once('/') {
        let n: f64 = n.parse().map_err(|_| "Invalid numerator")?;
        let d: f64 = d.parse().map_err(|_| "Invalid denominator")?;
        if d == 0.0 {
            return Err("Zero denominator".into());
        }
        Ok(n / d)
    } else {
        s.parse().map_err(|_| "Invalid length".into())
    }
}
