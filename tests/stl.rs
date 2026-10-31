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
