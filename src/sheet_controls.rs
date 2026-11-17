//! Small, cell-linked interface controls. Cell values remain in the workbook.
use crate::{
    spreadsheet::{CellAddr, Workbook},
    Result,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_CONTROLS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ControlTarget {
    pub sheet: u64,
    pub cell: CellAddr,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    Number,
    Slider { min: f64, max: f64, step: f64 },
    Result,
}

impl ControlKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Number => "Number input",
            Self::Slider { .. } => "Slider",
            Self::Result => "Result card",
        }
    }

    pub fn validate(self) -> Result<()> {
        if let Self::Slider { min, max, step } = self {
            let span = max - min;
            if !min.is_finite()
                || !max.is_finite()
                || !step.is_finite()
                || !span.is_finite()
                || min >= max
                || step <= 0.0
                || step > span
                || !(span / step).is_finite()
                || min + step == min
                || max - step == max
            {
                return Err("Sliders need a finite increasing range and a positive, representable step within that range".into());
            }
        }
        Ok(())
    }

    /// Shared slider preview/write math; input controls never retain a second
    /// cell value. Both exact endpoints remain available between step marks.
    pub fn normalize_value(self, value: f64) -> Result<f64> {
        self.validate()?;
        if !value.is_finite() {
            return Err("Control values must be finite numbers".into());
        }
        match self {
            Self::Number => Ok(value),
            Self::Slider { min, max, step } => {
                let value = value.clamp(min, max);
                // Both endpoints remain reachable when the span is not an
                // exact multiple of the step. Interior values snap from min.
                if value == min || value == max {
                    return Ok(value);
                }
                Ok(((value - min) / step)
                    .round()
                    .mul_add(step, min)
                    .clamp(min, max))
            }
            Self::Result => Err("Result cards do not write to cells".into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetControl {
    pub id: u64,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ControlTarget>,
    pub kind: ControlKind,
}

impl SheetControl {
    /// Validate references and configuration, not whether a cell is writable
    /// right now. A formula or merge added later must not prevent reloading.
    pub fn validate(&self, book: &Workbook) -> Result<()> {
        if self.id == 0
            || self.label.trim() != self.label
            || self.label.is_empty()
            || self.label.chars().count() > 120
            || self.label.chars().any(char::is_control)
        {
            return Err("Controls need a nonzero ID and a label of 1–120 characters".into());
        }
        self.kind.validate()?;
        if let Some(target) = self.target {
            if book.sheet(target.sheet).is_none() || !target.cell.in_bounds() {
                return Err("Control target is outside an existing sheet".into());
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_controls(book: &Workbook) -> Result<()> {
    if book.controls.len() > MAX_CONTROLS {
        return Err(format!(
            "A workbook can contain up to {MAX_CONTROLS} controls"
        ));
    }
    let mut ids = HashSet::new();
    for control in &book.controls {
        if !ids.insert(control.id) {
            return Err("Control IDs must be unique and nonzero".into());
        }
        control.validate(book)?;
    }
    Ok(())
}

impl Workbook {
    pub fn add_control(
        &mut self,
        label: &str,
        target: Option<ControlTarget>,
        kind: ControlKind,
    ) -> Result<u64> {
        let mut next = self.clone();
        let id = next
            .controls
            .iter()
            .map(|control| control.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("Control IDs exhausted")?;
        next.controls.push(SheetControl {
            id,
            label: label.trim().into(),
            target,
            kind,
        });
        next.validate()?;
        *self = next;
        Ok(id)
    }

    pub fn update_control(
        &mut self,
        id: u64,
        label: &str,
        target: Option<ControlTarget>,
        kind: ControlKind,
    ) -> Result<bool> {
        let mut next = self.clone();
        let control = next
            .controls
            .iter_mut()
            .find(|control| control.id == id)
            .ok_or("Control not found")?;
        *control = SheetControl {
            id,
            label: label.trim().into(),
            target,
            kind,
        };
        next.validate()?;
        let changed = next != *self;
        if changed {
            *self = next;
        }
        Ok(changed)
    }

    pub fn remove_control(&mut self, id: u64) -> Result<bool> {
        let before = self.controls.len();
        self.controls.retain(|control| control.id != id);
        Ok(before != self.controls.len())
    }

    pub fn move_control(&mut self, id: u64, delta: isize) -> Result<bool> {
        let from = self
            .controls
            .iter()
            .position(|control| control.id == id)
            .ok_or("Control not found")?;
        let to = from
            .saturating_add_signed(delta)
            .min(self.controls.len() - 1);
        if from == to {
            return Ok(false);
        }
        let control = self.controls.remove(from);
        self.controls.insert(to, control);
        Ok(true)
    }

    /// Resolve an input's current target on every write, including after a
    /// normal cell edit or an import has changed a literal into a formula.
    pub fn control_input_target(&self, id: u64) -> Result<ControlTarget> {
        let control = self
            .controls
            .iter()
            .find(|control| control.id == id)
            .ok_or("Control not found")?;
        control.validate(self)?;
        if control.kind == ControlKind::Result {
            return Err("Result cards do not write to cells".into());
        }
        let target = control
            .target
            .ok_or("Source removed; choose a cell to reconnect this control")?;
        if self
            .merge_at(target.sheet, target.cell)
            .is_some_and(|range| range.start != target.cell)
        {
            return Err("This cell is hidden by a merge; use its visible anchor".into());
        }
        if self
            .raw(target.sheet, target.cell)
            .trim_start()
            .starts_with('=')
        {
            return Err("This cell contains a formula; choose a literal input cell".into());
        }
        Ok(target)
    }

    pub fn set_control_value(&mut self, id: u64, value: f64) -> Result<bool> {
        let target = self.control_input_target(id)?;
        let kind = self
            .controls
            .iter()
            .find(|control| control.id == id)
            .ok_or("Control not found")?
            .kind;
        let value = kind.normalize_value(value)?;
        self.set_cell(target.sheet, target.cell, &value.to_string())
    }
}
