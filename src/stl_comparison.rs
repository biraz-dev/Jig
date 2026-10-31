//! Physical front-view comparison with a fixed-height human reference.
//! Both the model and silhouette use millimeters and the same X/Z projection.
use crate::Result;

pub const HUMAN_HEIGHT_MM: f64 = 69.0 * 25.4;
pub const HUMAN_WIDTH_MM: f64 = HUMAN_HEIGHT_MM * 0.30;
pub const COMPARISON_GAP_MM: f64 = HUMAN_HEIGHT_MM * 0.15;
pub const PADDING_FRACTION: f64 = 0.08;
pub type Point2 = [f64; 2];

#[derive(Clone, Copy, Debug)]
pub struct ComparisonLayout {
    /// Translation for a model already centered on its physical bounds.
    pub object_center_xz_mm: Point2,
    pub person_center_x_mm: f64,
    /// Rebased ground plane: the model's minimum Z and the person's feet.
    pub baseline_z_mm: f64,
    pub content_size_mm: Point2,
    pub view_size_mm: Point2,
    pub center_xz_mm: Point2,
}

impl ComparisonLayout {
    /// Fit the complete model and reference into an orthographic X/Z view.
    /// Input dimensions must include source units, axis scaling and shrinkage.
    /// Original file translation is irrelevant: a bounds-centered model is
    /// placed at `object_center_xz_mm`, rebasing its minimum Z to ground zero.
    pub fn new(dimensions_mm: [f64; 3], viewport_aspect: f64) -> Result<Self> {
        if dimensions_mm
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
            || !viewport_aspect.is_finite()
            || viewport_aspect <= 0.0
        {
            return Err(
                "Use finite nonnegative dimensions and a positive finite viewport aspect".into(),
            );
        }
        let [width, _, height] = dimensions_mm;
        let content_size_mm = [
            width + COMPARISON_GAP_MM + HUMAN_WIDTH_MM,
            height.max(HUMAN_HEIGHT_MM),
        ];
        let view_height = content_size_mm[1].max(content_size_mm[0] / viewport_aspect)
            / (1.0 - 2.0 * PADDING_FRACTION);
        let view_size_mm = [view_height * viewport_aspect, view_height];
        if content_size_mm
            .iter()
            .chain(&view_size_mm)
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err("Comparison view exceeds numeric limits".into());
        }
        Ok(Self {
            object_center_xz_mm: [width * 0.5, height * 0.5],
            person_center_x_mm: width + COMPARISON_GAP_MM + HUMAN_WIDTH_MM * 0.5,
            baseline_z_mm: 0.0,
            content_size_mm,
            view_size_mm,
            center_xz_mm: content_size_mm.map(|value| value * 0.5),
        })
    }

    /// Convert physical [X, Z] millimeters to OpenGL clip X/Y, with Z up.
    pub fn ndc(self, point_mm: Point2) -> Point2 {
        std::array::from_fn(|axis| {
            (point_mm[axis] - self.center_xz_mm[axis]) * 2.0 / self.view_size_mm[axis]
        })
    }

    pub fn object_offset_ndc(self) -> Point2 {
        self.ndc(self.object_center_xz_mm)
    }

    /// Multiply centered physical model coordinates by these factors, then add
    /// `object_offset_ndc`. Use the same factors for the silhouette's vertices.
    pub fn mm_to_ndc(self) -> Point2 {
        self.view_size_mm.map(|value| 2.0 / value)
    }
}

/// A neutral, stylized adult reference assembled from convex filled polygons.
/// These are local [X, Z] millimeters, centered on X=0, with feet at Z=0 and the
/// top of the head at exactly 5 ft 9 in. It is a size reference, not a claim
/// about a demographic average or a measurement of an individual person.
pub fn silhouette_mm() -> Vec<Vec<Point2>> {
    let mut head = Vec::with_capacity(16);
    for index in 0..16 {
        let angle = std::f64::consts::TAU * index as f64 / 16.0;
        head.push([
            angle.cos() * 0.0475 * HUMAN_HEIGHT_MM,
            (0.935 + angle.sin() * 0.065) * HUMAN_HEIGHT_MM,
        ]);
    }
    let mut polygons = vec![head];
    for polygon in [
        [
            [-0.025, 0.845],
            [0.025, 0.845],
            [0.025, 0.89],
            [-0.025, 0.89],
        ],
        [[-0.11, 0.85], [0.11, 0.85], [0.075, 0.51], [-0.075, 0.51]],
        [[-0.11, 0.84], [-0.135, 0.82], [-0.15, 0.43], [-0.115, 0.43]],
        [[0.11, 0.84], [0.135, 0.82], [0.15, 0.43], [0.115, 0.43]],
        [[-0.08, 0.56], [0.08, 0.56], [0.08, 0.45], [-0.08, 0.45]],
        [
            [-0.08, 0.49],
            [-0.005, 0.49],
            [-0.017, 0.025],
            [-0.075, 0.025],
        ],
        [[0.08, 0.49], [0.005, 0.49], [0.017, 0.025], [0.075, 0.025]],
        [
            [-0.092, 0.035],
            [-0.015, 0.035],
            [-0.015, 0.0],
            [-0.092, 0.0],
        ],
        [[0.092, 0.035], [0.015, 0.035], [0.015, 0.0], [0.092, 0.0]],
    ] {
        polygons.push(
            polygon
                .map(|point| point.map(|value| value * HUMAN_HEIGHT_MM))
                .to_vec(),
        );
    }
    polygons
}
