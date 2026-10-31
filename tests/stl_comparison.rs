use jig::stl_comparison::{
    silhouette_mm, ComparisonLayout, COMPARISON_GAP_MM, HUMAN_HEIGHT_MM, HUMAN_WIDTH_MM,
    PADDING_FRACTION,
};

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= expected.abs().max(1.0) * 1e-12,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn reference_is_exactly_five_feet_nine_with_flat_grounded_feet() {
    close(HUMAN_HEIGHT_MM, 1752.6);
    let polygons = silhouette_mm();
    assert_eq!(polygons.len(), 10);
    let points: Vec<_> = polygons.iter().flatten().copied().collect();
    let min_x = points
        .iter()
        .map(|point| point[0])
        .fold(f64::INFINITY, f64::min);
    let max_x = points
        .iter()
        .map(|point| point[0])
        .fold(f64::NEG_INFINITY, f64::max);
    let min_z = points
        .iter()
        .map(|point| point[1])
        .fold(f64::INFINITY, f64::min);
    let max_z = points
        .iter()
        .map(|point| point[1])
        .fold(f64::NEG_INFINITY, f64::max);
    close(min_x, -HUMAN_WIDTH_MM * 0.5);
    close(max_x, HUMAN_WIDTH_MM * 0.5);
    assert_eq!(min_z, 0.0);
    close(max_z, HUMAN_HEIGHT_MM);
    for foot in &polygons[8..] {
        assert_eq!(foot.iter().filter(|point| point[1] == 0.0).count(), 2);
    }
    for polygon in polygons {
        assert!(polygon.len() >= 3);
        assert!(polygon.iter().flatten().all(|value| value.is_finite()));
        let mut sign = 0.0_f64;
        for index in 0..polygon.len() {
            let a = polygon[index];
            let b = polygon[(index + 1) % polygon.len()];
            let c = polygon[(index + 2) % polygon.len()];
            let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
            if cross.abs() > 1e-9 {
                if sign != 0.0 {
                    assert_eq!(cross.signum(), sign);
                }
                sign = cross.signum();
            }
        }
    }
}

#[test]
fn model_and_person_share_the_ground_and_one_physical_scale() {
    for dimensions in [[5.0; 3], [5000.0, 1000.0, 5000.0]] {
        let layout = ComparisonLayout::new(dimensions, 1.6).unwrap();
        close(layout.object_center_xz_mm[1] - dimensions[2] * 0.5, 0.0);
        close(layout.baseline_z_mm, 0.0);
        close(
            layout.person_center_x_mm - HUMAN_WIDTH_MM * 0.5 - dimensions[0],
            COMPARISON_GAP_MM,
        );
        let ground = layout.ndc([0.0, 0.0])[1];
        let object_height = layout.ndc([0.0, dimensions[2]])[1] - ground;
        let person_height = layout.ndc([0.0, HUMAN_HEIGHT_MM])[1] - ground;
        close(object_height / person_height, dimensions[2] / HUMAN_HEIGHT_MM);
        assert_eq!(
            layout.object_offset_ndc(),
            layout.ndc(layout.object_center_xz_mm)
        );
        let scale = layout.mm_to_ndc();
        close(scale[0] * layout.view_size_mm[0], 2.0);
        close(scale[1] * layout.view_size_mm[1], 2.0);
    }
}

#[test]
fn portrait_and_wide_views_fit_both_subjects_with_the_same_padding() {
    for aspect in [0.2, 0.5, 1.0, 1.6, 4.0] {
        for dimensions in [
            [5.0; 3],
            [5000.0, 500.0, 8000.0],
            [12000.0, 2000.0, 1000.0],
        ] {
            let layout = ComparisonLayout::new(dimensions, aspect).unwrap();
            close(layout.view_size_mm[0] / layout.view_size_mm[1], aspect);
            let bound = 1.0 - 2.0 * PADDING_FRACTION;
            for x in [0.0, dimensions[0]] {
                for z in [0.0, dimensions[2]] {
                    let point = layout.ndc([x, z]);
                    assert!(point.into_iter().all(|value| value.abs() <= bound + 1e-12));
                }
            }
            for point in silhouette_mm().into_iter().flatten() {
                let screen = layout.ndc([
                    point[0] + layout.person_center_x_mm,
                    point[1] + layout.baseline_z_mm,
                ]);
                assert!(screen.into_iter().all(|value| value.abs() <= bound + 1e-12));
            }
        }
    }
}

#[test]
fn zero_width_or_flat_models_still_have_a_finite_grounded_comparison() {
    for dimensions in [[0.0, 50.0, 100.0], [100.0, 50.0, 0.0], [0.0; 3]] {
        let layout = ComparisonLayout::new(dimensions, 1.0).unwrap();
        assert!(layout
            .view_size_mm
            .into_iter()
            .all(|value| value.is_finite() && value > 0.0));
        assert!(layout.object_offset_ndc().into_iter().all(f64::is_finite));
        close(
            layout.object_center_xz_mm[1] - dimensions[2] * 0.5,
            layout.baseline_z_mm,
        );
    }
}

#[test]
fn invalid_dimensions_aspects_and_overflow_are_rejected() {
    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        for axis in 0..3 {
            let mut dimensions = [10.0; 3];
            dimensions[axis] = invalid;
            assert!(ComparisonLayout::new(dimensions, 1.0).is_err());
        }
    }
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(ComparisonLayout::new([10.0; 3], invalid).is_err());
    }
    assert!(ComparisonLayout::new([1e308; 3], 2.0).is_err());
    assert!(ComparisonLayout::new([10.0; 3], f64::MIN_POSITIVE).is_err());
    assert!(ComparisonLayout::new([10.0; 3], f64::MAX).is_err());
}
