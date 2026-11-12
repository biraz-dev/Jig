use crate::Result;

pub const UNITS: [&str; 5] = ["in", "ft", "mm", "cm", "m"];

/// Normalize a supported length unit for display and portable history.
pub fn canonical(unit: &str) -> Result<&'static str> {
    match unit.trim().to_ascii_lowercase().as_str() {
        "in" | "inch" | "inches" => Ok("in"),
        "ft" | "foot" | "feet" => Ok("ft"),
        "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => Ok("mm"),
        "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => Ok("cm"),
        "m" | "meter" | "meters" | "metre" | "metres" => Ok("m"),
        _ => Err("Unit must be in, ft, mm, cm or m".into()),
    }
}

fn scale(unit: &str) -> Result<f64> {
    match canonical(unit)? {
        "in" => Ok(0.0254),
        "ft" => Ok(0.3048),
        "mm" => Ok(0.001),
        "cm" => Ok(0.01),
        "m" => Ok(1.0),
        _ => Err("Unit must be in, ft, mm, cm or m".into()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversionError {
    UnknownUnit,
    IncompatibleDimensions,
    NonFinite,
}

/// Convert a numeric length, area or volume without rounding.
///
/// Units use the same trimmed, case-insensitive names as [`canonical`]. Areas
/// append `2`, `^2` or `²`; volumes append `3`, `^3` or `³`. Word forms such as
/// `square centimeters`, `sq in`, `cubic meters` and `cu ft` are also accepted.
/// Units of different dimensions cannot be converted into one another.
pub fn convert_value(
    value: f64,
    source: &str,
    target: &str,
) -> std::result::Result<f64, ConversionError> {
    if !value.is_finite() {
        return Err(ConversionError::NonFinite);
    }
    let (source_dimension, source_scale) = dimensional_scale(source)?;
    let (target_dimension, target_scale) = dimensional_scale(target)?;
    if source_dimension != target_dimension {
        return Err(ConversionError::IncompatibleDimensions);
    }
    // Form the ratio first so a finite result does not overflow while passing
    // through base units (in particular, very large identity conversions).
    let result = value * (source_scale / target_scale).powi(i32::from(source_dimension));
    if result.is_finite() {
        Ok(result)
    } else {
        Err(ConversionError::NonFinite)
    }
}

fn dimensional_scale(unit: &str) -> std::result::Result<(u8, f64), ConversionError> {
    let unit = unit.trim().to_ascii_lowercase();
    let (base, dimension) = if let Some(base) = unit
        .strip_prefix("square ")
        .or_else(|| unit.strip_prefix("sq "))
    {
        (base, 2)
    } else if let Some(base) = unit
        .strip_prefix("cubic ")
        .or_else(|| unit.strip_prefix("cu "))
    {
        (base, 3)
    } else {
        [("^2", 2), ("²", 2), ("2", 2), ("^3", 3), ("³", 3), ("3", 3)]
            .into_iter()
            .find_map(|(suffix, dimension)| unit.strip_suffix(suffix).map(|base| (base, dimension)))
            .unwrap_or((&unit, 1))
    };
    let scale = scale(base).map_err(|_| ConversionError::UnknownUnit)?;
    Ok((dimension, scale))
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

#[cfg(test)]
mod tests {
    use super::{convert_value, ConversionError};

    #[test]
    fn numeric_conversions_reject_nonfinite_values_and_invalid_units() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                convert_value(value, "cm", "in"),
                Err(ConversionError::NonFinite)
            );
        }
        assert_eq!(
            convert_value(f64::MAX, "m3", "mm3"),
            Err(ConversionError::NonFinite)
        );
        assert_eq!(
            convert_value(1.0, "cm2", "in3"),
            Err(ConversionError::IncompatibleDimensions)
        );
        assert_eq!(
            convert_value(1.0, "cm", "parsecs"),
            Err(ConversionError::UnknownUnit)
        );
    }
}
