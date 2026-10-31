use jig::stl_camera::{axes, Axis, Perspective, Projection, VERTICAL_FOV_DEGREES};

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.00002,
        "expected {expected}, got {actual}"
    );
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}

#[test]
fn camera_basis_is_orthonormal_and_keeps_the_existing_depth_convention() {
    for (yaw, pitch) in [(0.0, 0.0), (0.65, 0.55), (-2.1, 1.4), (3.0, -1.5)] {
        let (right, up, away) = axes(yaw, pitch);
        close(dot(right, right), 1.0);
        close(dot(up, up), 1.0);
        close(dot(away, away), 1.0);
        close(dot(right, up), 0.0);
        close(dot(right, away), 0.0);
        close(dot(up, away), 0.0);
        let cross = [
            right[1] * up[2] - right[2] * up[1],
            right[2] * up[0] - right[0] * up[2],
            right[0] * up[1] - right[1] * up[0],
        ];
        close(dot(cross, away), -1.0);
    }
    assert_eq!(Projection::default(), Projection::Orthographic);
}

#[test]
fn all_six_axis_snaps_look_from_the_named_side_and_keep_a_stable_up_vector() {
    for axis in Axis::ALL {
        for positive in [false, true] {
            let (yaw, pitch) = axis.snap_angles(positive);
            let (right, up, away) = axes(yaw, pitch);
            for (index, value) in away.into_iter().enumerate() {
                let expected = if index != axis.index() {
                    0.0
                } else if positive {
                    -1.0
                } else {
                    1.0
                };
                close(value, expected);
            }
            close(right[axis.index()], 0.0);
            close(up[axis.index()], 0.0);
            if axis != Axis::Z {
                close(up[2], 1.0);
            } else {
                close(right[0], 1.0);
                close(up[1], if positive { 1.0 } else { -1.0 });
            }
        }
    }
}

#[test]
fn perspective_uses_a_45_degree_vertical_field_and_correct_aspect() {
    assert_eq!(VERTICAL_FOV_DEGREES, 45.0);
    let camera = Perspective::new(2.0, 1.0, 5.0).unwrap();
    close(camera.focal_y, 2.414_213_7);
    close(camera.focal_x * 2.0, camera.focal_y);
    let edge = 5.0 * (22.5_f32.to_radians()).tan();
    close(camera.project([0.0, edge, 0.0]).unwrap()[1], 1.0);
    close(camera.project([edge * 2.0, 0.0, 0.0]).unwrap()[0], 1.0);
    let near = camera
        .project([0.0, 0.0, camera.near - camera.distance])
        .unwrap();
    let far = camera
        .project([0.0, 0.0, camera.far - camera.distance])
        .unwrap();
    close(near[2], -1.0);
    close(far[2], 1.0);
}

#[test]
fn perspective_shows_near_geometry_larger_and_fits_portrait_and_landscape() {
    for aspect in [0.4, 1.0, 2.5] {
        let radius = 3.0_f32.sqrt() * 0.5;
        let distance = Perspective::fit_distance(aspect, radius).unwrap();
        let camera = Perspective::new(aspect, radius, distance).unwrap();
        for x in [-0.5, 0.5] {
            for y in [-0.5, 0.5] {
                for z in [-0.5, 0.5] {
                    let p = camera.project([x, y, z]).unwrap();
                    assert!(p.into_iter().all(|value| value.abs() <= 1.0));
                }
            }
        }
        let near = camera.project([0.2, 0.1, -0.5]).unwrap();
        let far = camera.project([0.2, 0.1, 0.5]).unwrap();
        assert!(near[0] > far[0]);
        assert!(near[1] > far[1]);
        assert!(near[2] < far[2]);
        let zoomed = Perspective::new(aspect, radius, distance / 10000.0).unwrap();
        assert!(zoomed.distance > radius);
        assert!(zoomed.near > 0.0);
        assert!(zoomed.far > zoomed.distance + radius);
    }
}

#[test]
fn perspective_rejects_nonfinite_or_nonpositive_camera_inputs() {
    for invalid in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(Perspective::fit_distance(invalid, 1.0).is_err());
        assert!(Perspective::fit_distance(1.0, invalid).is_err());
        assert!(Perspective::new(invalid, 1.0, 3.0).is_err());
        assert!(Perspective::new(1.0, invalid, 3.0).is_err());
        assert!(Perspective::new(1.0, 1.0, invalid).is_err());
    }
    let camera = Perspective::new(1.0, 1.0, 4.0).unwrap();
    assert_eq!(camera.project([0.0, 0.0, -5.0]), None);
    assert_eq!(camera.project([f32::NAN, 0.0, 0.0]), None);
}
