use jig::stl::{self, LengthUnit, Mesh, Point, Triangle};

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-10 * expected.abs().max(1.0),
        "expected {expected}, got {actual}"
    );
}

fn cube() -> Vec<Triangle> {
    let points: [Point; 8] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];
    [
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ]
    .map(|face| face.map(|index| points[index]))
    .to_vec()
}

fn translated(mut triangles: Vec<Triangle>, offset: Point) -> Vec<Triangle> {
    for triangle in &mut triangles {
        for point in triangle {
            for (coordinate, delta) in point.iter_mut().zip(offset) {
                *coordinate += delta;
            }
        }
    }
    triangles
}

fn ascii(triangles: &[Triangle], name: &str) -> Vec<u8> {
    let mut text = format!("solid {name}\n");
    for triangle in triangles {
        text.push_str("  facet normal 0 0 0\n    outer loop\n");
        for [x, y, z] in triangle {
            text.push_str(&format!("      vertex {x} {y} {z}\n"));
        }
        text.push_str("    endloop\n  endfacet\n");
    }
    text.push_str(&format!("endsolid {name}\n"));
    text.into_bytes()
}

fn binary(triangles: &[Triangle], header: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 80];
    bytes[..header.len()].copy_from_slice(header);
    bytes.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for triangle in triangles {
        for value in [0.0_f32; 3] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for point in triangle {
            for coordinate in point {
                bytes.extend_from_slice(&(*coordinate as f32).to_le_bytes());
            }
        }
        bytes.extend_from_slice(&0_u16.to_le_bytes());
    }
    bytes
}

#[test]
fn cube_geometry_matches_analytic_measurements() {
    let mesh = Mesh::from_triangles(cube()).unwrap();
    assert_eq!(mesh.triangles.len(), 12);
    assert_eq!(mesh.bounds.min, [0.0; 3]);
    assert_eq!(mesh.bounds.max, [1.0; 3]);
    assert_eq!(mesh.bounds.dimensions(), [1.0; 3]);
    close(mesh.metrics.surface_area, 6.0);
    close(mesh.metrics.volume.unwrap(), 1.0);

    assert_eq!(mesh.metrics.boundary_edges, 0);
    assert_eq!(mesh.metrics.non_manifold_edges, 0);
    assert_eq!(mesh.metrics.inconsistent_edges, 0);
    assert_eq!(mesh.metrics.degenerate_triangles, 0);
    assert_eq!(mesh.metrics.components, 1);
    assert!(!mesh.metrics.mixed_component_winding);
}

#[test]
fn small_models_have_readable_measurements_without_losing_precision() {
    let mesh = Mesh::from_triangles(cube()).unwrap();
    let one_mm = mesh.measure(LengthUnit::Millimeters, 1.0).unwrap();
    assert_eq!(one_mm.volume_label().as_deref(), Some("1.00 mm³"));
    assert_eq!(one_mm.area_label(), "0.06 cm²");
    assert_eq!(one_mm.dimensions_label(), "1.00 × 1.00 × 1.00 mm");
    assert_eq!(
        one_mm.volume_alternatives_label().as_deref(),
        Some("<0.01 mL · <0.01 in³")
    );
    close(one_mm.volume_mm3.unwrap(), 1.0);

    let tiny = mesh.measure(LengthUnit::Millimeters, 0.1).unwrap();
    assert_eq!(tiny.volume_label().as_deref(), Some("<0.01 mm³"));
    assert_eq!(tiny.area_label(), "0.06 mm²");
    assert_eq!(tiny.dimensions_label(), "0.10 × 0.10 × 0.10 mm");
    close(tiny.volume_mm3.unwrap(), 0.001);

    let smaller = mesh.measure(LengthUnit::Millimeters, 0.001).unwrap();
    assert_eq!(smaller.area_label(), "<0.01 mm²");
    assert_eq!(smaller.dimensions_label(), "<0.01 × <0.01 × <0.01 mm");
}

#[test]
fn measurement_labels_keep_normal_units_and_distinguish_zero_from_unavailable() {
    let mesh = Mesh::from_triangles(cube()).unwrap();
    let centimeter = mesh.measure(LengthUnit::Centimeters, 1.0).unwrap();
    assert_eq!(centimeter.volume_label().as_deref(), Some("1.00 cm³"));
    assert_eq!(centimeter.area_label(), "6.00 cm²");
    assert_eq!(
        centimeter.volume_alternatives_label().as_deref(),
        Some("1.00 mL · 0.06 in³")
    );
    let threshold = stl::Measurements {
        dimensions_mm: [0.0, 0.01, 1.0],
        surface_area_mm2: 1.0,
        volume_mm3: Some(10.0),
    };
    assert_eq!(threshold.volume_label().as_deref(), Some("0.01 cm³"));
    assert_eq!(threshold.area_label(), "0.01 cm²");
    assert_eq!(threshold.dimensions_label(), "0.00 × 0.01 × 1.00 mm");

    let empty = stl::Measurements {
        dimensions_mm: [0.0; 3],
        surface_area_mm2: 0.0,
        volume_mm3: None,
    };
    assert_eq!(empty.volume_label(), None);
    assert_eq!(empty.volume_alternatives_label(), None);
    assert_eq!(empty.area_label(), "0.00 cm²");
    let zero = stl::Measurements {
        volume_mm3: Some(0.0),
        ..empty
    };
    assert_eq!(zero.volume_label().as_deref(), Some("0.00 cm³"));
    assert_eq!(
        zero.volume_alternatives_label().as_deref(),
        Some("0.00 mL · 0.00 in³")
    );
    let subnormal = stl::Measurements {
        volume_mm3: Some(f64::from_bits(1)),
        ..empty
    };
    assert_eq!(subnormal.volume_label().as_deref(), Some("<0.01 mm³"));
    assert_eq!(
        subnormal.volume_alternatives_label().as_deref(),
        Some("<0.01 mL · <0.01 in³")
    );
}

#[test]
fn tetrahedron_and_globally_reversed_winding_have_correct_volume() {
    let a = [0.0, 0.0, 0.0];
    let b = [1.0, 0.0, 0.0];
    let c = [0.0, 1.0, 0.0];
    let d = [0.0, 0.0, 1.0];
    let triangles = vec![[a, c, b], [a, b, d], [a, d, c], [b, c, d]];
    let mesh = Mesh::from_triangles(triangles.clone()).unwrap();
    close(mesh.metrics.volume.unwrap(), 1.0 / 6.0);
    close(mesh.metrics.surface_area, 1.5 + 3.0_f64.sqrt() / 2.0);
    let reversed = triangles.into_iter().map(|[a, b, c]| [a, c, b]).collect();
    let mesh = Mesh::from_triangles(reversed).unwrap();
    close(mesh.metrics.volume.unwrap(), 1.0 / 6.0);
    assert_eq!(mesh.metrics.inconsistent_edges, 0);
}

#[test]
fn ascii_and_binary_stl_produce_the_same_geometry() {
    let triangles = cube();
    let ascii_mesh = stl::parse(&ascii(&triangles, "Sample cube")).unwrap();
    // A binary header beginning with "solid" must still be read as binary.
    let binary_mesh = stl::parse(&binary(&triangles, b"solid binary fixture")).unwrap();
    assert_eq!(ascii_mesh.triangles, binary_mesh.triangles);
    close(ascii_mesh.metrics.surface_area, 6.0);
    close(binary_mesh.metrics.volume.unwrap(), 1.0);
}

#[test]
fn ascii_body_whitespace_and_multiple_solids_are_supported() {
    let first = ascii(&cube(), "first");
    let first = String::from_utf8(first)
        .unwrap()
        .replace("facet normal 0 0 0", "facet\t normal\n0  0\t0")
        .replace("outer loop", "outer\nloop");
    let mut source = first.into_bytes();
    source.extend(ascii(&translated(cube(), [3.0, 0.0, 0.0]), "second"));
    let mesh = stl::parse(&source).unwrap();
    assert_eq!(mesh.triangles.len(), 24);
    assert_eq!(mesh.metrics.components, 2);
    assert_eq!(mesh.bounds.dimensions(), [4.0, 1.0, 1.0]);
    close(mesh.metrics.surface_area, 12.0);
    close(mesh.metrics.volume.unwrap(), 2.0);
}

#[test]
fn malformed_empty_truncated_and_nonfinite_stl_is_rejected() {
    let bytes = binary(&cube(), b"Sample cube");
    for length in [0, 3, 79, 80, 83, 84, bytes.len() - 1] {
        assert!(stl::parse(&bytes[..length]).is_err(), "length {length}");
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(stl::parse(&trailing).is_err());
    let mut bad_coordinate = bytes.clone();
    bad_coordinate[96..100].copy_from_slice(&f32::INFINITY.to_le_bytes());
    assert!(stl::parse(&bad_coordinate).is_err());
    let mut bad_count = bytes;
    bad_count[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(stl::parse(&bad_count).is_err());
    for bad in [
        "solid empty\nendsolid empty\n",
        "solid bad\nfacet normal 0 0 0\nendsolid bad\n",
        "solid bad\nfacet normal 0 0 0\nouter loop\nvertex NaN 0 0\n",
        "solid bad\nnot a facet\nendsolid bad\n",
    ] {
        assert!(stl::parse(bad.as_bytes()).is_err(), "{bad}");
    }
    let mut unterminated = ascii(&cube(), "missing end");
    unterminated.truncate(unterminated.len() - "endsolid missing end\n".len());
    assert!(stl::parse(&unterminated).is_err());
    let mut trailing_ascii = ascii(&cube(), "trailing");
    trailing_ascii.extend_from_slice(b"unexpected trailing data\n");
    assert!(stl::parse(&trailing_ascii).is_err());
    assert!(Mesh::from_triangles(Vec::new()).is_err());
    let mut invalid = cube();
    invalid[0][0][0] = f64::NAN;
    assert!(Mesh::from_triangles(invalid).is_err());
}

#[test]
fn open_nonmanifold_and_inconsistently_wound_meshes_do_not_claim_volume() {
    let mut open = cube();
    open.remove(3);
    open.remove(2);
    let mesh = Mesh::from_triangles(open).unwrap();
    close(mesh.metrics.surface_area, 5.0);
    assert_eq!(mesh.metrics.boundary_edges, 4);
    assert_eq!(mesh.metrics.volume, None);

    let mut duplicate = cube();
    duplicate.push(duplicate[0]);
    let mesh = Mesh::from_triangles(duplicate).unwrap();
    assert!(mesh.metrics.non_manifold_edges > 0);
    assert_eq!(mesh.metrics.volume, None);

    let mut inconsistent = cube();
    inconsistent[0].swap(1, 2);
    let mesh = Mesh::from_triangles(inconsistent).unwrap();
    assert_eq!(mesh.metrics.boundary_edges, 0);
    assert!(mesh.metrics.inconsistent_edges > 0);
    assert_eq!(mesh.metrics.volume, None);
}

#[test]
fn degenerate_triangles_and_mixed_shell_winding_are_reported() {
    let mut degenerate = cube();
    degenerate.push([[0.0; 3]; 3]);
    let mesh = Mesh::from_triangles(degenerate).unwrap();
    assert_eq!(mesh.metrics.degenerate_triangles, 1);
    assert_eq!(mesh.metrics.volume, None);

    let mut shells = cube();
    shells.extend(
        translated(cube(), [3.0, 0.0, 0.0])
            .into_iter()
            .map(|[a, b, c]| [a, c, b]),
    );
    let mesh = Mesh::from_triangles(shells).unwrap();
    assert_eq!(mesh.metrics.components, 2);
    assert!(mesh.metrics.mixed_component_winding);
    assert_eq!(mesh.metrics.volume, None);
}

#[test]
fn translated_models_and_signed_zero_keep_topology_and_volume() {
    let mesh = Mesh::from_triangles(translated(cube(), [1e9, -1e9, 1e9])).unwrap();
    assert_eq!(mesh.bounds.dimensions(), [1.0; 3]);
    close(mesh.metrics.surface_area, 6.0);
    close(mesh.metrics.volume.unwrap(), 1.0);

    let mut separated = translated(cube(), [1e9, 1e9, 1e9]);
    separated.extend(translated(cube(), [-1e9, -1e9, -1e9]));
    let mesh = Mesh::from_triangles(separated).unwrap();
    assert_eq!(mesh.metrics.components, 2);
    close(mesh.metrics.volume.unwrap(), 2.0);

    let mut triangles = cube();
    for (index, triangle) in triangles.iter_mut().enumerate() {
        if index % 2 == 0 {
            for point in triangle {
                for coordinate in point {
                    if *coordinate == 0.0 {
                        *coordinate = -0.0;
                    }
                }
            }
        }
    }
    let mesh = Mesh::from_triangles(triangles).unwrap();
    assert_eq!(mesh.metrics.boundary_edges, 0);
    close(mesh.metrics.volume.unwrap(), 1.0);
}

#[test]
fn source_units_and_scale_use_squared_area_and_cubed_volume() {
    let mesh = Mesh::from_triangles(cube()).unwrap();
    for (unit, factor) in [
        (LengthUnit::Millimeters, 1.0_f64),
        (LengthUnit::Centimeters, 10.0),
        (LengthUnit::Inches, 25.4),
    ] {
        let measurements = mesh.measure(unit, 2.0).unwrap();
        for dimension in measurements.dimensions_mm {
            close(dimension, factor * 2.0);
        }
        close(measurements.surface_area_mm2, 6.0 * (factor * 2.0).powi(2));
        close(measurements.volume_mm3.unwrap(), (factor * 2.0).powi(3));
    }
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e308, 1e-200] {
        assert!(mesh.measure(LengthUnit::Millimeters, invalid).is_err());
    }
}

#[test]
fn linear_shrinkage_forecasts_and_inverse_compensation_are_distinct() {
    let mesh = Mesh::from_triangles(cube()).unwrap();
    let factors = stl::shrinkage(12.0).unwrap();
    close(factors.remaining, 0.88);
    close(factors.compensation, 1.0 / 0.88);
    let fired = mesh
        .measure(LengthUnit::Millimeters, 100.0 * factors.remaining)
        .unwrap();
    assert_eq!(fired.dimensions_mm, [88.0; 3]);
    close(fired.surface_area_mm2, 60_000.0 * 0.88_f64.powi(2));
    close(fired.volume_mm3.unwrap(), 1_000_000.0 * 0.88_f64.powi(3));
    let restored = mesh
        .measure(
            LengthUnit::Millimeters,
            100.0 * factors.compensation * factors.remaining,
        )
        .unwrap();
    for dimension in restored.dimensions_mm {
        close(dimension, 100.0);
    }
    let no_shrinkage = stl::shrinkage(0.0).unwrap();
    assert_eq!(no_shrinkage.remaining, 1.0);
    assert_eq!(no_shrinkage.compensation, 1.0);
    for invalid in [-1.0, 100.0, 101.0, f64::NAN, f64::INFINITY] {
        assert!(stl::shrinkage(invalid).is_err());
    }
}

#[test]
fn scaled_binary_export_roundtrips_without_changing_original_geometry() {
    let mesh = Mesh::from_triangles(cube()).unwrap();
    let exported = mesh.to_binary_stl(2.0).unwrap();
    assert_eq!(exported.len(), 84 + 50 * mesh.triangles.len());
    let restored = stl::parse(&exported).unwrap();
    assert_eq!(restored.bounds.dimensions(), [2.0; 3]);
    close(restored.metrics.surface_area, 24.0);
    close(restored.metrics.volume.unwrap(), 8.0);
    close(mesh.metrics.volume.unwrap(), 1.0);
    assert_eq!(mesh.bounds.dimensions(), [1.0; 3]);
    assert!(mesh.to_binary_stl(1e100).is_err());
    assert!(mesh.to_binary_stl(1e-50).is_err());
    let translated = Mesh::from_triangles(translated(cube(), [1e9; 3])).unwrap();
    assert!(translated.to_binary_stl(1.0).is_err());
}

#[test]
fn independent_axis_scaling_matches_a_rectangular_box_in_each_source_unit() {
    let mesh = Mesh::from_triangles(cube()).unwrap();
    for unit in [
        LengthUnit::Millimeters,
        LengthUnit::Centimeters,
        LengthUnit::Inches,
    ] {
        let measured = mesh.measure_axes(unit, [2.0, 3.0, 4.0]).unwrap();
        let factor = unit.millimeters_per_unit();
        for (dimension, expected) in measured.dimensions_mm.into_iter().zip([2.0, 3.0, 4.0]) {
            close(dimension, expected * factor);
        }
        close(measured.surface_area_mm2, 52.0 * factor.powi(2));
        close(measured.volume_mm3.unwrap(), 24.0 * factor.powi(3));
    }
    assert_eq!(mesh.bounds.dimensions(), [1.0; 3]);
    close(mesh.metrics.surface_area, 6.0);
    close(mesh.metrics.volume.unwrap(), 1.0);
}

#[test]
fn anisotropic_scaling_uses_oblique_face_geometry_and_exports_matching_normals() {
    let triangle = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mesh = Mesh::from_triangles(vec![triangle]).unwrap();
    let measured = mesh
        .measure_axes(LengthUnit::Millimeters, [2.0, 3.0, 4.0])
        .unwrap();
    assert_eq!(measured.dimensions_mm, [2.0, 3.0, 4.0]);
    close(measured.surface_area_mm2, 61.0_f64.sqrt());
    assert_eq!(measured.volume_mm3, None);

    let bytes = mesh.to_binary_stl_axes([2.0, 3.0, 4.0]).unwrap();
    let normal: [f64; 3] = std::array::from_fn(|axis| {
        f32::from_le_bytes(bytes[84 + axis * 4..88 + axis * 4].try_into().unwrap()) as f64
    });
    let expected = [6.0, 4.0, 3.0].map(|value| value / 61.0_f64.sqrt());
    for (actual, expected) in normal.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-7);
    }
    let restored = stl::parse(&bytes).unwrap();
    assert_eq!(
        restored.triangles,
        vec![[[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]]]
    );
    close(restored.metrics.surface_area, 61.0_f64.sqrt());
    assert_eq!(restored.metrics.volume, None);
}

#[test]
fn independent_axis_export_roundtrips_actual_vertices_dimensions_area_and_volume() {
    let mesh = Mesh::from_triangles(translated(cube(), [-3.0, 5.0, 7.0])).unwrap();
    let bytes = mesh.to_binary_stl_axes([2.0, 3.0, 4.0]).unwrap();
    let restored = stl::parse(&bytes).unwrap();
    assert_eq!(bytes.len(), 84 + 50 * mesh.triangles.len());
    assert_eq!(restored.bounds.min, [-6.0, 15.0, 28.0]);
    assert_eq!(restored.bounds.max, [-4.0, 18.0, 32.0]);
    assert_eq!(restored.bounds.dimensions(), [2.0, 3.0, 4.0]);
    close(restored.metrics.surface_area, 52.0);
    close(restored.metrics.volume.unwrap(), 24.0);
    assert_eq!(restored.metrics.boundary_edges, 0);
    assert_eq!(restored.metrics.inconsistent_edges, 0);
    assert_eq!(mesh.bounds.min, [-3.0, 5.0, 7.0]);
    assert_eq!(mesh.bounds.dimensions(), [1.0; 3]);
}

#[test]
fn uniform_axis_factors_preserve_the_existing_scalar_interface() {
    let mesh = Mesh::from_triangles(cube()).unwrap();
    for unit in [
        LengthUnit::Millimeters,
        LengthUnit::Centimeters,
        LengthUnit::Inches,
    ] {
        for factor in [0.25, 1.0, 2.5] {
            let scalar = mesh.measure(unit, factor).unwrap();
            let axes = mesh.measure_axes(unit, [factor; 3]).unwrap();
            assert_eq!(scalar.dimensions_mm, axes.dimensions_mm);
            assert_eq!(scalar.surface_area_mm2, axes.surface_area_mm2);
            assert_eq!(scalar.volume_mm3, axes.volume_mm3);
            assert_eq!(
                mesh.to_binary_stl(factor).unwrap(),
                mesh.to_binary_stl_axes([factor; 3]).unwrap()
            );
        }
    }
}

#[test]
fn independent_axis_factors_reject_invalid_or_unrepresentable_geometry() {
    let mesh = Mesh::from_triangles(cube()).unwrap();
    for axis in 0..3 {
        for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut factors = [1.0; 3];
            factors[axis] = invalid;
            assert!(mesh.measure_axes(LengthUnit::Millimeters, factors).is_err());
            assert!(mesh.to_binary_stl_axes(factors).is_err());
        }
        let mut overflow = [1.0; 3];
        overflow[axis] = 1e100;
        assert!(mesh.to_binary_stl_axes(overflow).is_err());
        let mut underflow = [1.0; 3];
        underflow[axis] = 1e-50;
        assert!(mesh.to_binary_stl_axes(underflow).is_err());
    }
    assert!(mesh
        .measure_axes(LengthUnit::Millimeters, [1e308; 3])
        .is_err());
    assert!(mesh
        .measure_axes(LengthUnit::Millimeters, [1e-200; 3])
        .is_err());
    let distant = Mesh::from_triangles(translated(cube(), [1e9; 3])).unwrap();
    assert!(distant.to_binary_stl_axes([2.0, 3.0, 4.0]).is_err());
}

#[test]
fn binary_export_rejects_precision_that_changes_dimensions_without_merging_vertices() {
    let large_cube: Vec<Triangle> = cube()
        .into_iter()
        .map(|triangle| triangle.map(|point| point.map(|coordinate| coordinate * 100.0)))
        .collect();
    let distant = Mesh::from_triangles(translated(large_cube.clone(), [1e9; 3])).unwrap();
    assert_eq!(distant.bounds.dimensions(), [100.0; 3]);
    // At this offset, the two distinct f32 positions are 128 units apart.
    // Retaining distinct vertices alone does not preserve the intended size.
    assert!(distant.to_binary_stl(1.0).is_err());
    assert!(distant.to_binary_stl_axes([1.0; 3]).is_err());

    let origin = Mesh::from_triangles(large_cube).unwrap();
    let factors = [1.23456789, 0.987654321, std::f64::consts::PI];
    let measured = origin
        .measure_axes(LengthUnit::Millimeters, factors)
        .unwrap();
    let restored = stl::parse(&origin.to_binary_stl_axes(factors).unwrap()).unwrap();
    for (actual, expected) in restored
        .bounds
        .dimensions()
        .into_iter()
        .zip(measured.dimensions_mm)
    {
        assert!((actual - expected).abs() <= expected * 1e-5);
    }
}
