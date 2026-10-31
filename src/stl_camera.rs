//! Small, portable camera helpers shared by the model view and its axis widget.
//! Coordinates are Z-up; `depth` points away from the viewer.
use crate::Result;
use std::f32::consts::{FRAC_PI_2, PI};

pub const VERTICAL_FOV_DEGREES: f32 = 45.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }

    /// Look from the selected side of an axis toward the origin.
    /// Side views keep Z up; the +Z top view places X right and Y up.
    pub fn snap_angles(self, positive: bool) -> (f32, f32) {
        match (self, positive) {
            (Self::X, true) => (-FRAC_PI_2, 0.0),
            (Self::X, false) => (FRAC_PI_2, 0.0),
            (Self::Y, true) => (PI, 0.0),
            (Self::Y, false) => (0.0, 0.0),
            (Self::Z, true) => (0.0, FRAC_PI_2),
            (Self::Z, false) => (0.0, -FRAC_PI_2),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Projection {
    #[default]
    Orthographic,
    Perspective,
}

impl Projection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Orthographic => "Orthographic",
            Self::Perspective => "Perspective",
        }
    }
}

/// Camera right, up and away vectors. The same basis must rotate both the mesh
/// and the gnomon. Screen Y is the negative of the returned up vector.
pub fn axes(yaw: f32, pitch: f32) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    (
        [cy, -sy, 0.0],
        [sp * sy, sp * cy, cp],
        [cp * sy, cp * cy, -sp],
    )
}

/// A conventional 45-degree vertical perspective projection. View coordinates
/// use positive depth away from the viewer, matching `axes` and the STL shader.
#[derive(Clone, Copy, Debug)]
pub struct Perspective {
    pub distance: f32,
    pub near: f32,
    pub far: f32,
    pub focal_x: f32,
    pub focal_y: f32,
    pub depth_scale: f32,
    pub depth_offset: f32,
}

impl Perspective {
    /// Distance that fits a bounding sphere in the narrower field of view.
    /// The radius includes physical model scaling but does not include zoom.
    pub fn fit_distance(aspect: f32, radius: f32) -> Result<f32> {
        validate_dimensions(aspect, radius)?;
        let half_vertical = VERTICAL_FOV_DEGREES.to_radians() * 0.5;
        let half_horizontal = (half_vertical.tan() * aspect).atan();
        let distance = radius / half_vertical.min(half_horizontal).sin() * 1.05;
        if !distance.is_finite() || distance <= radius {
            return Err("Perspective camera exceeds numeric limits".into());
        }
        Ok(distance)
    }

    /// Keep the camera outside the model's bounding sphere, even while zooming.
    /// Callers can request `fit_distance / zoom` without crossing a near plane.
    pub fn new(aspect: f32, radius: f32, requested_distance: f32) -> Result<Self> {
        validate_dimensions(aspect, radius)?;
        if !requested_distance.is_finite() || requested_distance <= 0.0 {
            return Err("Camera distance must be finite and greater than zero".into());
        }
        let distance = requested_distance.max(radius * 1.05);
        let near = (distance - radius) * 0.5;
        let far = distance + radius * 1.5;
        let span = far - near;
        let focal_y = (VERTICAL_FOV_DEGREES.to_radians() * 0.5).tan().recip();
        let focal_x = focal_y / aspect;
        let depth_scale = 1.0 + 2.0 * near / span;
        let depth_offset = -2.0 * near * (far / span);
        let scalars = [
            distance,
            near,
            far,
            span,
            focal_x,
            focal_y,
            depth_scale,
            depth_offset,
        ];
        if scalars.iter().any(|value| !value.is_finite())
            || near <= 0.0
            || span <= 0.0
            || far <= distance + radius
        {
            return Err("Perspective camera exceeds numeric limits".into());
        }
        Ok(Self {
            distance,
            near,
            far,
            focal_x,
            focal_y,
            depth_scale,
            depth_offset,
        })
    }

    /// Project camera-space [right, up, away] coordinates into OpenGL NDC.
    /// Returns None behind the camera or outside floating-point limits. Points
    /// outside [-1, 1] are still returned so the renderer can clip normally.
    pub fn project(self, point: [f32; 3]) -> Option<[f32; 3]> {
        let depth = self.distance + point[2];
        if depth <= 0.0 || !depth.is_finite() {
            return None;
        }
        let projected = [
            point[0] * self.focal_x / depth,
            point[1] * self.focal_y / depth,
            self.depth_scale + self.depth_offset / depth,
        ];
        projected
            .iter()
            .all(|value| value.is_finite())
            .then_some(projected)
    }
}

fn validate_dimensions(aspect: f32, radius: f32) -> Result<()> {
    if !aspect.is_finite() || aspect <= 0.0 || !radius.is_finite() || radius <= 0.0 {
        return Err("Camera aspect and model radius must be finite and greater than zero".into());
    }
    Ok(())
}
