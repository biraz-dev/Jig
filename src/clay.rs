//! Named linear-shrinkage estimates and user-owned test profiles.
//!
//! These values describe a change in length, not volume, kiln temperature or
//! warping. Published points are intentionally not interpolated between cones.
use crate::{model::title, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const STANDARD_104_SOURCE: &str = "https://www.standardclay.com/products/104-red-clay-w-grog";
pub const STANDARD_104_REVIEWED: &str = "2026-10-30";

/// The input dimensions must refer to this starting stage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// The manufacturer gives a percentage without defining its starting stage.
    #[default]
    Unspecified,
    WetToFired,
    DryToFired,
}
impl Basis {
    pub const ALL: [Self; 3] = [Self::Unspecified, Self::WetToFired, Self::DryToFired];

    pub fn label(self) -> &'static str {
        match self {
            Self::Unspecified => "Starting stage unspecified",
            Self::WetToFired => "Wet to fired",
            Self::DryToFired => "Dry to fired",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// Retain leading zeros: cone 06 is different from cone 6.
    pub cone: String,
    /// Percentage of length lost: 6.0 means six percent, not a fraction.
    pub linear_shrinkage: f64,
    #[serde(default)]
    pub basis: Basis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}
impl Profile {
    pub fn label(&self) -> String {
        format!("{} / cone {}", self.name, self.cone)
    }

    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_'))
        {
            return Err("Invalid clay profile ID".into());
        }
        title(&self.name)?;
        if self.name.chars().count() > 120 {
            return Err("Clay profile name is too long".into());
        }
        if normalize_cone(&self.cone)? != self.cone {
            return Err("Use a canonical cone such as 06, 04, 4 or 6".into());
        }
        if !self.linear_shrinkage.is_finite() || !(0.0..100.0).contains(&self.linear_shrinkage) {
            return Err("Linear shrinkage must be at least 0% and less than 100%".into());
        }
        if let Some(source) = &self.source {
            if source.len() > 2048
                || !source.starts_with("https://")
                || source.chars().any(char::is_control)
                || source.chars().any(char::is_whitespace)
                || source.len() <= "https://".len()
            {
                return Err("Clay source must be an HTTPS URL".into());
            }
        }
        Ok(())
    }
}

/// Only exact cone points provided by the manufacturer are built in.
/// The source page does not identify a wet-to-fired or dry-to-fired basis.
pub fn builtin_profiles() -> Vec<Profile> {
    [("06", 6.0), ("4", 11.0)]
        .into_iter()
        .map(|(cone, linear_shrinkage)| Profile {
            id: format!("standard-104-{cone}"),
            name: "Standard #104 Red with Grog".into(),
            cone: cone.into(),
            linear_shrinkage,
            basis: Basis::Unspecified,
            source: Some(STANDARD_104_SOURCE.into()),
        })
        .collect()
}

/// Accept ordinary pyrometric cone notation without losing leading zeros.
pub fn normalize_cone(text: &str) -> Result<String> {
    let text = text.trim();
    let value = if text
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("cone "))
    {
        text[5..].trim()
    } else {
        text
    };
    let valid = if let Some(zero_prefixed) = value.strip_prefix('0') {
        zero_prefixed
            .parse::<u8>()
            .ok()
            .is_some_and(|number| (1..=22).contains(&number) && value == format!("0{number}"))
    } else {
        value
            .parse::<u8>()
            .ok()
            .is_some_and(|number| (1..=14).contains(&number) && value == number.to_string())
    };
    if valid {
        Ok(value.to_owned())
    } else {
        Err("Use a cone from 022 to 01, or 1 to 14; cone 06 is not cone 6".into())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub selected: String,
    pub custom_profiles: Vec<Profile>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            selected: "standard-104-06".into(),
            custom_profiles: vec![],
        }
    }
}
impl Settings {
    pub fn available_profiles(&self) -> Vec<Profile> {
        let mut profiles = builtin_profiles();
        profiles.extend(self.custom_profiles.iter().cloned());
        profiles
    }

    pub fn selected_profile(&self) -> Option<Profile> {
        self.available_profiles()
            .into_iter()
            .find(|profile| profile.id == self.selected)
    }

    pub fn validate(&self) -> Result<()> {
        if self.custom_profiles.len() > 100 {
            return Err("At most 100 custom clay profiles can be saved".into());
        }
        let builtins = builtin_profiles();
        let mut ids: HashSet<_> = builtins.iter().map(|profile| profile.id.as_str()).collect();
        for profile in &self.custom_profiles {
            profile.validate()?;
            if !ids.insert(profile.id.as_str()) {
                return Err("Clay profile IDs must be unique and cannot replace built-ins".into());
            }
        }
        if !ids.contains(self.selected.as_str()) {
            return Err("Selected clay profile is missing".into());
        }
        Ok(())
    }

    /// Save a measured value as a separate profile; never rewrite published data.
    pub fn add_custom(
        &mut self,
        name: &str,
        cone: &str,
        linear_shrinkage: f64,
        basis: Basis,
    ) -> Result<String> {
        if self.custom_profiles.len() >= 100 {
            return Err("At most 100 custom clay profiles can be saved".into());
        }
        let id = (1_u64..=101)
            .map(|number| format!("custom-{number}"))
            .find(|id| !self.custom_profiles.iter().any(|profile| &profile.id == id))
            .ok_or("No available clay profile IDs")?;
        let profile = Profile {
            id: id.clone(),
            name: title(name)?,
            cone: normalize_cone(cone)?,
            linear_shrinkage,
            basis,
            source: None,
        };
        profile.validate()?;
        self.custom_profiles.push(profile);
        self.selected = id.clone();
        Ok(id)
    }

    pub fn remove_custom(&mut self, id: &str) -> Result<()> {
        let index = self
            .custom_profiles
            .iter()
            .position(|profile| profile.id == id)
            .ok_or("Custom clay profile not found")?;
        self.custom_profiles.remove(index);
        if self.selected == id {
            self.selected = Self::default().selected;
        }
        Ok(())
    }
}
