//! Bounded STL parsing and geometry measurements. No rendering or filesystem APIs.
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MAX_FILE_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_TRIANGLES: usize = 200_000;
pub type Point = [f64; 3];
pub type Triangle = [Point; 3];

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min: Point,
    pub max: Point,
}
impl Bounds {
    pub fn dimensions(&self) -> Point {
        std::array::from_fn(|axis| self.max[axis] - self.min[axis])
    }
}

#[derive(Clone, Debug)]
pub struct Metrics {
    pub surface_area: f64,
    /// Available only for closed, nondegenerate meshes with coherent winding.
    /// This check does not detect geometric intersections or overlapping shells.
    pub volume: Option<f64>,
    pub boundary_edges: usize,
    pub non_manifold_edges: usize,
    pub inconsistent_edges: usize,
    pub degenerate_triangles: usize,
    pub components: usize,
    pub mixed_component_winding: bool,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub triangles: Vec<Triangle>,
    pub bounds: Bounds,
    pub metrics: Metrics,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    #[default]
    Millimeters,
    Centimeters,
    Inches,
}
impl LengthUnit {
    pub const ALL: [Self; 3] = [Self::Millimeters, Self::Centimeters, Self::Inches];

    pub fn label(self) -> &'static str {
        match self {
            Self::Millimeters => "mm",
            Self::Centimeters => "cm",
            Self::Inches => "in",
        }
    }

    pub fn millimeters_per_unit(self) -> f64 {
        match self {
            Self::Millimeters => 1.0,
            Self::Centimeters => 10.0,
            Self::Inches => 25.4,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Measurements {
    pub dimensions_mm: Point,
    pub surface_area_mm2: f64,
    pub volume_mm3: Option<f64>,
}

impl Measurements {
    /// Compact labels keep two decimals while preserving the measured values.
    /// Small positive quantities use smaller units or a bound instead of zero.
    pub fn volume_label(&self) -> Option<String> {
        self.volume_mm3.map(|volume| {
            if volume > 0.0 && volume < 10.0 {
                format!("{} mm³", measurement_number(volume, 1.0))
            } else {
                format!("{} cm³", measurement_number(volume, 1000.0))
            }
        })
    }

    pub fn area_label(&self) -> String {
        if self.surface_area_mm2 > 0.0 && self.surface_area_mm2 < 1.0 {
            format!("{} mm²", measurement_number(self.surface_area_mm2, 1.0))
        } else {
            format!("{} cm²", measurement_number(self.surface_area_mm2, 100.0))
        }
    }

    pub fn dimensions_label(&self) -> String {
        let [x, y, z] = self
            .dimensions_mm
            .map(|value| measurement_number(value, 1.0));
        format!("{x} × {y} × {z} mm")
    }

    pub fn volume_alternatives_label(&self) -> Option<String> {
        self.volume_mm3.map(|volume| {
            format!(
                "{} mL · {} in³",
                measurement_number(volume, 1000.0),
                measurement_number(volume, 25.4_f64.powi(3))
            )
        })
    }
}

fn measurement_number(value: f64, divisor: f64) -> String {
    if value > 0.0 && value < 0.01 * divisor {
        "<0.01".into()
    } else {
        format!("{:.2}", value / divisor)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Shrinkage {
    pub remaining: f64,
    pub compensation: f64,
}

/// Linear, isotropic dry-to-fired or wet-to-fired shrinkage, as specified by the user.
/// A cone label alone is not a material model and never supplies a percentage here.
pub fn shrinkage(percent: f64) -> Result<Shrinkage> {
    if !percent.is_finite() || !(0.0..100.0).contains(&percent) {
        return Err("Linear shrinkage must be from 0 to less than 100 percent".into());
    }
    let remaining = 1.0 - percent / 100.0;
    Ok(Shrinkage {
        remaining,
        compensation: remaining.recip(),
    })
}

pub fn parse(bytes: &[u8]) -> Result<Mesh> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err("STL exceeds the 20 MiB limit".into());
    }
    // Binary headers are arbitrary bytes and often begin with the word solid.
    // The exact byte count identifies them before considering ASCII syntax.
    if bytes.len() >= 84 {
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        let expected = count.checked_mul(50).and_then(|size| size.checked_add(84));
        if expected == Some(bytes.len()) {
            if count == 0 || count > MAX_TRIANGLES {
                return Err("STL must contain 1 to 200,000 triangles".into());
            }
            let mut triangles = Vec::with_capacity(count);
            for record in bytes[84..].as_chunks::<50>().0 {
                let mut coordinates = [0.0; 12];
                for (index, value) in coordinates.iter_mut().enumerate() {
                    *value =
                        f32::from_le_bytes(record[index * 4..index * 4 + 4].try_into().unwrap())
                            as f64;
                    if !value.is_finite() {
                        return Err("STL contains a non-finite coordinate or normal".into());
                    }
                }
                triangles.push(std::array::from_fn(|vertex| {
                    std::array::from_fn(|axis| coordinates[3 + vertex * 3 + axis])
                }));
            }
            return Mesh::from_triangles(triangles);
        }
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "Binary STL is truncated or its triangle count does not match its size")?;
    let mut tokens = Tokens::new(text.trim_start_matches('\u{feff}'));
    let mut triangles = Vec::new();
    while tokens.peek().is_some() {
        tokens.expect("solid")?;
        tokens.skip_line();
        loop {
            match tokens.next() {
                Some("endsolid") => {
                    tokens.skip_line();
                    break;
                }
                Some("facet") => {
                    if triangles.len() == MAX_TRIANGLES {
                        return Err("STL exceeds the 200,000 triangle limit".into());
                    }
                    tokens.expect("normal")?;
                    tokens.point()?;
                    tokens.expect("outer")?;
                    tokens.expect("loop")?;
                    let mut triangle = [[0.0; 3]; 3];
                    for vertex in &mut triangle {
                        tokens.expect("vertex")?;
                        *vertex = tokens.point()?;
                    }
                    tokens.expect("endloop")?;
                    tokens.expect("endfacet")?;
                    triangles.push(triangle);
                }
                _ => return Err("Expected an ASCII STL facet or endsolid".into()),
            }
        }
    }
    Mesh::from_triangles(triangles)
}

impl Mesh {
    pub fn from_triangles(triangles: Vec<Triangle>) -> Result<Self> {
        if triangles.is_empty() || triangles.len() > MAX_TRIANGLES {
            return Err("STL must contain 1 to 200,000 triangles".into());
        }
        let mut bounds = Bounds {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
        };
        for point in triangles.iter().flatten() {
            for (axis, coordinate) in point.iter().enumerate() {
                if !coordinate.is_finite() {
                    return Err("STL contains a non-finite coordinate".into());
                }
                bounds.min[axis] = bounds.min[axis].min(*coordinate);
                bounds.max[axis] = bounds.max[axis].max(*coordinate);
            }
        }
        let metrics = analyze(&triangles, bounds)?;
        Ok(Self {
            triangles,
            bounds,
            metrics,
        })
    }

    /// STL itself carries no units. The selected unit is an explicit assumption.
    pub fn measure(&self, unit: LengthUnit, scale: f64) -> Result<Measurements> {
        self.measure_axes(unit, [scale; 3])
    }

    /// Measure positive diagonal scaling along the model's original X/Y/Z axes.
    /// Nonuniform surface area comes from the transformed faces, not an average
    /// scale factor. The original mesh and its topology are never changed.
    pub fn measure_axes(&self, unit: LengthUnit, scale: Point) -> Result<Measurements> {
        for factor in scale {
            positive_scale(factor)?;
        }
        let factors = scale.map(|value| value * unit.millimeters_per_unit());
        let dimensions_mm: Point =
            std::array::from_fn(|axis| self.bounds.dimensions()[axis] * factors[axis]);
        if factors.iter().any(|value| !value.is_finite())
            || dimensions_mm.iter().any(|value| !value.is_finite())
        {
            return Err("Scaled measurements exceed numeric limits".into());
        }
        let uniform = factors[0] == factors[1] && factors[1] == factors[2];
        let surface_area_mm2 = if uniform {
            self.metrics.surface_area * factors[0].powi(2)
        } else {
            let mut area = Sum::default();
            for triangle in &self.triangles {
                let a = subtract(triangle[1], triangle[0]);
                let b = subtract(triangle[2], triangle[0]);
                let normal = cross(
                    std::array::from_fn(|axis| a[axis] * factors[axis]),
                    std::array::from_fn(|axis| b[axis] * factors[axis]),
                );
                area.add(normal_length(normal) * 0.5);
            }
            area.value
        };
        let volume_mm3 = self.metrics.volume.map(|value| {
            if uniform {
                value * factors[0].powi(3)
            } else {
                value * factors[0] * factors[1] * factors[2]
            }
        });
        if dimensions_mm.iter().any(|value| !value.is_finite())
            || !surface_area_mm2.is_finite()
            || volume_mm3.is_some_and(|value| !value.is_finite())
            || self
                .bounds
                .dimensions()
                .iter()
                .zip(dimensions_mm)
                .any(|(source, scaled)| *source > 0.0 && scaled == 0.0)
            || (self.metrics.surface_area > 0.0 && surface_area_mm2 == 0.0)
            || (self.metrics.volume.is_some_and(|value| value > 0.0) && volume_mm3 == Some(0.0))
        {
            return Err("Scaled measurements exceed numeric limits".into());
        }
        Ok(Measurements {
            dimensions_mm,
            surface_area_mm2,
            volume_mm3,
        })
    }

    /// Export uniformly scaled vertices in the original source units.
    /// This preserves topology and does not repair a mesh or add STL unit metadata.
    pub fn to_binary_stl(&self, scale: f64) -> Result<Vec<u8>> {
        self.to_binary_stl_axes([scale; 3])
    }

    /// Export positive diagonal scaling along the model's original X/Y/Z axes.
    /// Factors apply to coordinates directly; callers can include a conversion
    /// between source and output units. STL itself stores no unit metadata.
    pub fn to_binary_stl_axes(&self, scale: Point) -> Result<Vec<u8>> {
        for factor in scale {
            positive_scale(factor)?;
        }
        let mut bytes = Vec::with_capacity(84 + self.triangles.len() * 50);
        let expected_dimensions: Point =
            std::array::from_fn(|axis| self.bounds.dimensions()[axis] * scale[axis]);
        let mut exported_bounds = Bounds {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
        };
        bytes.resize(80, 0);
        let label = b"Jig local scaled STL";
        bytes[..label.len()].copy_from_slice(label);
        bytes.extend_from_slice(&(self.triangles.len() as u32).to_le_bytes());
        let mut exported_vertices = HashMap::new();
        for triangle in &self.triangles {
            let mut exported = [[0.0_f32; 3]; 3];
            for (point, scaled) in triangle.iter().zip(&mut exported) {
                *scaled = std::array::from_fn(|axis| (point[axis] * scale[axis]) as f32);
                if scaled.iter().any(|value| !value.is_finite())
                    || point
                        .iter()
                        .zip(scaled.iter())
                        .any(|(source, value)| *source != 0.0 && *value == 0.0)
                {
                    return Err("Scaled vertices exceed binary STL precision limits".into());
                }
                let original = point.map(|value| if value == 0.0 { 0 } else { value.to_bits() });
                let key = scaled.map(|value| if value == 0.0 { 0 } else { value.to_bits() });
                if exported_vertices
                    .insert(key, original)
                    .is_some_and(|previous| previous != original)
                {
                    return Err("Binary STL precision would merge distinct vertices".into());
                }
                for (axis, value) in scaled.iter().enumerate() {
                    exported_bounds.min[axis] = exported_bounds.min[axis].min(f64::from(*value));
                    exported_bounds.max[axis] = exported_bounds.max[axis].max(f64::from(*value));
                }
            }
            // Normals describe the actual f32 vertices written to the file.
            // Recomputing them is required when axes have different scales.
            let points = exported.map(|point| point.map(f64::from));
            let normal = cross(subtract(points[1], points[0]), subtract(points[2], points[0]));
            let length = normal_length(normal);
            let original_normal = cross(
                subtract(triangle[1], triangle[0]),
                subtract(triangle[2], triangle[0]),
            );
            if length == 0.0 && normal_length(original_normal) > 0.0 {
                return Err("Binary STL precision would collapse a triangle".into());
            }
            for coordinate in normal.map(|value| if length > 0.0 { value / length } else { 0.0 }) {
                bytes.extend_from_slice(&(coordinate as f32).to_le_bytes());
            }
            for point in exported {
                for coordinate in point {
                    bytes.extend_from_slice(&coordinate.to_le_bytes());
                }
            }
            bytes.extend_from_slice(&[0, 0]);
        }
        // Distinct vertices can still move far enough during f32 rounding to
        // change the exported size, especially far from the coordinate origin.
        if exported_bounds
            .dimensions()
            .iter()
            .zip(expected_dimensions)
            .any(|(actual, expected)| {
                !expected.is_finite() || (*actual - expected).abs() > expected * 1e-5
            })
        {
            return Err("Binary STL precision would change the model dimensions; move the model closer to the origin before exporting".into());
        }
        Ok(bytes)
    }
}

fn positive_scale(scale: f64) -> Result<()> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err("Scale must be a finite number greater than zero".into());
    }
    Ok(())
}

#[derive(Default)]
struct Sum {
    value: f64,
    correction: f64,
}
impl Sum {
    fn add(&mut self, value: f64) {
        let adjusted = value - self.correction;
        let total = self.value + adjusted;
        self.correction = (total - self.value) - adjusted;
        self.value = total;
    }
}

struct Edge {
    count: usize,
    direction: i32,
    triangle: usize,
}

struct Component {
    origin: Point,
    volume: Sum,
}

fn analyze(triangles: &[Triangle], bounds: Bounds) -> Result<Metrics> {
    let dimensions = bounds.dimensions();
    if dimensions.iter().any(|value| !value.is_finite()) {
        return Err("STL dimensions exceed numeric limits".into());
    }
    let mut vertices = HashMap::new();
    let mut edges: HashMap<(usize, usize), Edge> = HashMap::new();
    let mut parents: Vec<_> = (0..triangles.len()).collect();
    let mut ranks = vec![0_u8; triangles.len()];
    let mut surface = Sum::default();
    let mut degenerate_triangles = 0;
    for (index, triangle) in triangles.iter().enumerate() {
        let normal = cross(
            subtract(triangle[1], triangle[0]),
            subtract(triangle[2], triangle[0]),
        );
        let area = dot(normal, normal).sqrt() * 0.5;
        if !area.is_finite() {
            return Err("STL geometry exceeds numeric limits".into());
        }
        if area == 0.0 {
            degenerate_triangles += 1;
        }
        surface.add(area);
        let ids = triangle.map(|point| {
            // Exact vertex matching avoids silently welding genuine gaps.
            let key = point.map(|value| if value == 0.0 { 0 } else { value.to_bits() });
            let next = vertices.len();
            *vertices.entry(key).or_insert(next)
        });
        for (a, b) in [(ids[0], ids[1]), (ids[1], ids[2]), (ids[2], ids[0])] {
            let key = (a.min(b), a.max(b));
            let direction = if a < b { 1 } else { -1 };
            if let Some(edge) = edges.get_mut(&key) {
                edge.count += 1;
                edge.direction += direction;
                union(&mut parents, &mut ranks, index, edge.triangle);
            } else {
                edges.insert(
                    key,
                    Edge {
                        count: 1,
                        direction,
                        triangle: index,
                    },
                );
            }
        }
    }
    let boundary_edges = edges.values().filter(|edge| edge.count == 1).count();
    let non_manifold_edges = edges.values().filter(|edge| edge.count > 2).count();
    let inconsistent_edges = edges
        .values()
        .filter(|edge| edge.count == 2 && edge.direction != 0)
        .count();
    let mut components: HashMap<usize, Component> = HashMap::new();
    for (index, triangle) in triangles.iter().enumerate() {
        let component = components
            .entry(root(&mut parents, index))
            .or_insert_with(|| Component {
                origin: triangle[0],
                volume: Sum::default(),
            });
        // Each connected shell gets its own nearby reference point. This avoids
        // cancellation for translated objects and widely separated components.
        let [a, b, c] = triangle.map(|point| subtract(point, component.origin));
        let volume = dot(a, cross(b, c)) / 6.0;
        if !volume.is_finite() {
            return Err("STL geometry exceeds numeric limits".into());
        }
        component.volume.add(volume);
    }
    let positive = components
        .values()
        .any(|component| component.volume.value > 0.0);
    let negative = components
        .values()
        .any(|component| component.volume.value < 0.0);
    let mixed_component_winding = positive && negative;
    let closed = boundary_edges == 0
        && non_manifold_edges == 0
        && inconsistent_edges == 0
        && degenerate_triangles == 0
        && !mixed_component_winding
        && components
            .values()
            .all(|component| component.volume.value != 0.0);
    let mut volume = Sum::default();
    for component in components.values() {
        volume.add(component.volume.value.abs());
    }
    if !surface.value.is_finite() || !volume.value.is_finite() {
        return Err("STL measurements exceed numeric limits".into());
    }
    Ok(Metrics {
        surface_area: surface.value,
        volume: closed.then_some(volume.value),
        boundary_edges,
        non_manifold_edges,
        inconsistent_edges,
        degenerate_triangles,
        components: components.len(),
        mixed_component_winding,
    })
}

fn root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}

fn union(parents: &mut [usize], ranks: &mut [u8], a: usize, b: usize) {
    let mut a = root(parents, a);
    let mut b = root(parents, b);
    if a == b {
        return;
    }
    if ranks[a] < ranks[b] {
        std::mem::swap(&mut a, &mut b);
    }
    parents[b] = a;
    if ranks[a] == ranks[b] {
        ranks[a] += 1;
    }
}

fn subtract(a: Point, b: Point) -> Point {
    std::array::from_fn(|axis| a[axis] - b[axis])
}

fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normal_length(normal: Point) -> f64 {
    normal[0].hypot(normal[1]).hypot(normal[2])
}

struct Tokens<'a> {
    text: &'a str,
    position: usize,
}
impl<'a> Tokens<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, position: 0 }
    }

    fn next(&mut self) -> Option<&'a str> {
        let bytes = self.text.as_bytes();
        while self.position < bytes.len() && bytes[self.position].is_ascii_whitespace() {
            self.position += 1;
        }
        let start = self.position;
        while self.position < bytes.len() && !bytes[self.position].is_ascii_whitespace() {
            self.position += 1;
        }
        (start != self.position).then(|| &self.text[start..self.position])
    }

    fn peek(&mut self) -> Option<&'a str> {
        let position = self.position;
        let token = self.next();
        self.position = position;
        token
    }

    fn expect(&mut self, expected: &str) -> Result<()> {
        if self.next() != Some(expected) {
            return Err(format!("Invalid ASCII STL: expected {expected}"));
        }
        Ok(())
    }

    fn point(&mut self) -> Result<Point> {
        let mut point = [0.0; 3];
        for value in &mut point {
            *value = self
                .next()
                .and_then(|token| token.parse::<f64>().ok())
                .filter(|value| value.is_finite())
                .ok_or("Invalid ASCII STL: expected a finite coordinate or normal")?;
        }
        Ok(point)
    }

    fn skip_line(&mut self) {
        let bytes = self.text.as_bytes();
        while self.position < bytes.len() && !matches!(bytes[self.position], b'\r' | b'\n') {
            self.position += 1;
        }
    }
}
