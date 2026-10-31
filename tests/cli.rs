use std::{fs, process::Command};
#[test]
fn cli_shares_state_and_returns_predictable_errors() {
    let dir = std::env::temp_dir().join(format!("jig-cli-test-{}", std::process::id()));
    let path = dir.join("state.json");
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_jig"))
            .arg("--state")
            .arg(&path)
            .args(args)
            .output()
            .unwrap()
    };
    let calc = invoke(&["calc", "125 * 1.15"]);
    assert!(calc.status.success());
    assert_eq!(String::from_utf8_lossy(&calc.stdout).trim(), "143.75");
    let ans = invoke(&["calc", "ans * 2"]);
    assert_eq!(String::from_utf8_lossy(&ans.stdout).trim(), "287.50");
    assert!(invoke(&["tasks", "add", "Photograph sculptures"])
        .status
        .success());
    assert!(String::from_utf8_lossy(&invoke(&["tasks"]).stdout).contains("Photograph sculptures"));
    assert!(
        invoke(&["deadline", "add", "Opening", "2026-12-01T18:00:00Z"])
            .status
            .success()
    );
    assert!(
        String::from_utf8_lossy(&invoke(&["deadline", "countdown"]).stdout).contains("Opening")
    );
    let before = fs::read(&path).unwrap();
    assert!(!invoke(&["calc", "1/0"]).status.success());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!invoke(&["tasks", "delete", "999999"]).status.success());
    assert!(!invoke(&["nonsense"]).status.success());
    assert_eq!(
        String::from_utf8_lossy(&invoke(&["units", "1 1/2in", "mm"]).stdout).trim(),
        "38.10 mm"
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "units must remain read-only"
    );
    let converted = invoke(&["calc", "72 in mm"]);
    assert!(converted.status.success());
    assert_eq!(
        String::from_utf8_lossy(&converted.stdout).trim(),
        "1828.80 mm"
    );
    let restored = jig::model::State::from_json(&fs::read_to_string(&path).unwrap()).unwrap();
    let answer = restored.history.last().unwrap();
    assert_eq!(answer.expression, "72 in mm");
    assert_eq!(answer.unit.as_deref(), Some("mm"));
    assert!((answer.result - 1828.8).abs() < 1e-9);
    let history = invoke(&["history"]);
    assert!(history.status.success());
    assert!(String::from_utf8_lossy(&history.stdout).contains("72 in mm = 1828.80 mm"));
    let reused = invoke(&["calc", "ans / 25.4"]);
    assert!(reused.status.success());
    assert_eq!(String::from_utf8_lossy(&reused.stdout).trim(), "72.00");
    let fraction = invoke(&["calc", "1 1/2 in cm"]);
    assert!(fraction.status.success());
    assert_eq!(String::from_utf8_lossy(&fraction.stdout).trim(), "3.81 cm");
    let before_invalid = fs::read(&path).unwrap();
    let invalid = invoke(&["calc", "72 in nonsense"]);
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    assert!(!invalid.stderr.is_empty());
    assert_eq!(fs::read(&path).unwrap(), before_invalid);
    assert!(invoke(&["tasks", "done", "1"]).status.success());
    let cleared = invoke(&["tasks", "clear-completed"]);
    assert!(cleared.status.success());
    assert!(String::from_utf8_lossy(&cleared.stdout).contains("Removed 1 completed tasks"));
    assert!(invoke(&["tasks"]).stdout.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_dated_tasks_share_the_countdown_and_calendar() {
    let dir = std::env::temp_dir().join(format!("jig-cli-dated-test-{}", std::process::id()));
    let path = dir.join("state.json");
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_jig"))
            .arg("--state")
            .arg(&path)
            .args(args)
            .output()
            .unwrap()
    };
    let load = || jig::model::State::from_json(&fs::read_to_string(&path).unwrap()).unwrap();
    let added = invoke(&["tasks", "add", "Prepare installation"]);
    assert!(added.status.success());
    assert_eq!(String::from_utf8_lossy(&added.stdout).trim(), "1");
    let undated = invoke(&["tasks"]);
    assert_eq!(
        String::from_utf8_lossy(&undated.stdout).trim(),
        "1 [ ] Prepare installation"
    );

    let dated = invoke(&["tasks", "due", "1", "2030-01-15T12:00:00Z"]);
    assert!(dated.status.success());
    let state = load();
    let deadline_id = state.task_deadline(1).unwrap().id;
    assert_eq!(state.tasks.len(), 1);
    assert_eq!(state.deadlines.len(), 1);
    assert_eq!(state.featured, None);
    assert!(invoke(&["tasks", "star", "1"]).status.success());
    assert_eq!(load().featured, Some(deadline_id));
    let listed = invoke(&["tasks"]);
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(listed.contains("due 2030-01-15T12:00:00+00:00 *"));
    let countdown = invoke(&["deadline", "countdown"]);
    assert!(countdown.status.success());
    assert!(String::from_utf8_lossy(&countdown.stdout).contains("Prepare installation"));
    let calendar = ["calendar", "--year", "2030", "--month", "1"];
    assert!(String::from_utf8_lossy(&invoke(&calendar).stdout).contains('*'));

    let before_invalid = fs::read(&path).unwrap();
    assert!(!invoke(&["tasks", "due", "1", "not a date"])
        .status
        .success());
    assert_eq!(fs::read(&path).unwrap(), before_invalid);
    assert!(invoke(&["tasks", "done", "1"]).status.success());
    assert_eq!(load().featured, None);
    assert!(!invoke(&["deadline", "countdown"]).status.success());
    assert!(!String::from_utf8_lossy(&invoke(&calendar).stdout).contains('*'));
    let before_star = fs::read(&path).unwrap();
    assert!(!invoke(&["tasks", "star", "1"]).status.success());
    assert_eq!(fs::read(&path).unwrap(), before_star);

    assert!(invoke(&["tasks", "done", "1"]).status.success());
    assert_eq!(load().featured, None);
    assert!(invoke(&["tasks", "star", "1"]).status.success());
    assert!(invoke(&["tasks", "edit", "1", "Install new work"])
        .status
        .success());
    let countdown = invoke(&["deadline", "countdown"]);
    assert!(String::from_utf8_lossy(&countdown.stdout).contains("Install new work"));
    assert!(invoke(&["tasks", "due", "1"]).status.success());
    let state = load();
    assert_eq!(state.tasks.len(), 1);
    assert!(state.task_deadline(1).is_none());
    assert!(state.deadlines.is_empty());
    assert_eq!(state.featured, None);
    assert!(state.history.is_empty());
    let listed = invoke(&["tasks"]);
    assert_eq!(
        String::from_utf8_lossy(&listed.stdout).trim(),
        "1 [ ] Install new work"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_task_starts_and_timeline_reuse_portable_task_records() {
    let dir = std::env::temp_dir().join(format!("jig-cli-timeline-test-{}", std::process::id()));
    let path = dir.join("state.json");
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_jig"))
            .env("TZ", "UTC")
            .arg("--state")
            .arg(&path)
            .args(args)
            .output()
            .unwrap()
    };
    let load = || jig::model::State::from_json(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(invoke(&["tasks", "add", "Dry casting"]).status.success());
    assert!(invoke(&["tasks", "due", "1", "2030-01-16T17:00:00Z"])
        .status
        .success());
    assert!(
        invoke(&["tasks", "start", "1", "2030-01-14T03:00:00-05:00"])
            .status
            .success()
    );
    assert!(invoke(&["tasks", "star", "1"]).status.success());
    assert!(invoke(&["tasks", "add", "Fire kiln"]).status.success());
    assert!(invoke(&["tasks", "due", "3", "2030-01-17T12:00:00Z"])
        .status
        .success());
    assert!(invoke(&["tasks", "add", "Order supplies"]).status.success());
    let state = load();
    let deadline = state.task_deadline(1).unwrap().id;
    assert_eq!(state.tasks.len(), 3);
    assert_eq!(state.deadlines.len(), 2);
    assert_eq!(state.featured, Some(deadline));
    assert_eq!(
        state.tasks[0].start.unwrap().to_rfc3339(),
        "2030-01-14T08:00:00+00:00"
    );
    let listed = invoke(&["tasks"]);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("start 2030-01-14T08:00:00+00:00"));
    let saved = fs::read(&path).unwrap();
    let week = invoke(&["timeline", "2030-01-15"]);
    assert!(week.status.success());
    let week = String::from_utf8_lossy(&week.stdout);
    assert!(week.contains("2030-01-14 to 2030-01-20"));
    assert!(week.contains("Open: Dry casting · 2030-01-14 to 2030-01-16\n|===    |"));
    assert!(week.contains("Open: Fire kiln · due 2030-01-17\n|   *   |"));
    assert!(week.contains("Open: Order supplies · No date"));
    let month = invoke(&["timeline", "2030-01-15", "--month"]);
    assert!(month.status.success());
    assert!(String::from_utf8_lossy(&month.stdout).contains("2030-01-01 to 2030-01-31"));
    assert_eq!(
        fs::read(&path).unwrap(),
        saved,
        "Timeline views are read-only"
    );
    let invalid: &[&[&str]] = &[
        &["tasks", "start", "1", "not a date"],
        &["tasks", "start", "1", "2030-01-17T00:00:00Z"],
        &["tasks", "due", "1", "2030-01-13T12:00:00Z"],
        &[
            "deadline",
            "edit",
            "2",
            "Must not rename",
            "2030-01-13T12:00:00Z",
        ],
        &["tasks", "start", "5", "2030-01-14T00:00:00Z"],
        &["tasks", "start", "999999", "2030-01-14T00:00:00Z"],
        &["timeline", "2030-02-30"],
    ];
    for args in invalid {
        assert!(!invoke(args).status.success());
        assert_eq!(
            fs::read(&path).unwrap(),
            saved,
            "Invalid edits must not save"
        );
    }
    let restored = jig::model::State::from_json(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(restored.tasks[0].start, state.tasks[0].start);
    assert_eq!(restored.task_deadline(1).unwrap().id, deadline);
    assert_eq!(restored.featured, Some(deadline));
    assert!(invoke(&["tasks", "done", "1"]).status.success());
    assert!(
        String::from_utf8_lossy(&invoke(&["timeline", "2030-01-15"]).stdout)
            .contains("Done: Dry casting")
    );
    assert_eq!(load().tasks[0].start, state.tasks[0].start);
    assert_eq!(load().featured, None);
    assert!(invoke(&["tasks", "start", "1"]).status.success());
    assert_eq!(load().tasks[0].start, None);
    assert_eq!(load().task_deadline(1).unwrap().id, deadline);
    assert!(invoke(&["tasks", "start", "1", "2030-01-14T08:00:00Z"])
        .status
        .success());
    assert!(invoke(&["tasks", "due", "1"]).status.success());
    let state = load();
    assert_eq!(state.tasks[0].start, None);
    assert!(state.task_deadline(1).is_none());
    assert_eq!(state.tasks.len(), 3);
    assert_eq!(state.deadlines.len(), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_stl_reports_geometry_and_shrinkage_without_changing_state() {
    let dir = std::env::temp_dir().join(format!("jig-cli-stl-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("state.json");
    let binary_path = dir.join("sample-cube.stl");
    let ascii_path = dir.join("sample-cube-ascii.stl");
    let open_path = dir.join("sample-open.stl");
    let invalid_path = dir.join("sample-invalid.stl");
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_jig"))
            .arg("--state")
            .arg(&path)
            .args(args)
            .output()
            .unwrap()
    };
    let vertices: [[f32; 3]; 8] = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, 0.0, 10.0],
        [10.0, 0.0, 10.0],
        [10.0, 10.0, 10.0],
        [0.0, 10.0, 10.0],
    ];
    let faces = [
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
    ];
    let mut binary = vec![0; 80];
    binary.extend_from_slice(&(faces.len() as u32).to_le_bytes());
    let mut ascii = String::from("solid sample_cube\n");
    for face in faces {
        binary.extend_from_slice(&[0; 12]);
        ascii.push_str("facet normal 0 0 0\nouter loop\n");
        for index in face {
            let vertex = vertices[index];
            for value in vertex {
                binary.extend_from_slice(&value.to_le_bytes());
            }
            ascii.push_str(&format!(
                "vertex {} {} {}\n",
                vertex[0], vertex[1], vertex[2]
            ));
        }
        binary.extend_from_slice(&[0; 2]);
        ascii.push_str("endloop\nendfacet\n");
    }
    ascii.push_str("endsolid sample_cube\n");
    fs::write(&binary_path, &binary).unwrap();
    fs::write(&ascii_path, &ascii).unwrap();
    let mut open = binary[..binary.len() - 50].to_vec();
    open[80..84].copy_from_slice(&11_u32.to_le_bytes());
    fs::write(&open_path, open).unwrap();
    fs::write(&invalid_path, b"not an STL").unwrap();
    let binary_name = binary_path.to_str().unwrap();
    let ascii_name = ascii_path.to_str().unwrap();

    assert!(invoke(&["calc", "1 / 3"]).status.success());
    let before = fs::read(&path).unwrap();
    for name in [binary_name, ascii_name] {
        let output = invoke(&["stl", name]);
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("STL: 12 triangles"));
        assert!(text.contains("Dimensions: 10.00 x 10.00 x 10.00 mm"));
        assert!(text.contains("Surface area: 600.00 mm^2"));
        assert!(text.contains("Volume: 1000.00 mm^3 (1.00 cm^3)"));
        assert!(text.contains("0 boundary edges"));
    }
    let output = invoke(&["stl", binary_name, "--scale", "200"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Dimensions: 20.00 x 20.00 x 20.00 mm"));
    assert!(text.contains("Surface area: 2400.00 mm^2"));
    assert!(text.contains("Volume: 8000.00 mm^3 (8.00 cm^3)"));
    let inspect_axes = |extra: &[&str]| {
        let mut args = vec![
            "stl",
            binary_name,
            "--scale-x",
            "200",
            "--scale-y",
            "300",
            "--scale-z",
            "400",
        ];
        args.extend_from_slice(extra);
        invoke(&args)
    };
    let output = inspect_axes(&[]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Dimensions: 20.00 x 30.00 x 40.00 mm"));
    assert!(text.contains("Surface area: 5200.00 mm^2"));
    assert!(text.contains("Volume: 24000.00 mm^3 (24.00 cm^3)"));
    assert!(text.contains("Axis scale: X 200.00%, Y 300.00%, Z 400.00%"));

    let output = inspect_axes(&["--scale", "200"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Dimensions: 40.00 x 60.00 x 80.00 mm"));
    assert!(text.contains("Surface area: 20800.00 mm^2"));
    assert!(text.contains("Volume: 192000.00 mm^3 (192.00 cm^3)"));

    let output = inspect_axes(&["--units", "cm"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Dimensions: 200.00 x 300.00 x 400.00 mm"));
    assert!(text.contains("Surface area: 520000.00 mm^2"));
    assert!(text.contains("Volume: 24000000.00 mm^3 (24000.00 cm^3)"));

    let output = inspect_axes(&["--shrinkage", "10"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Dimensions: 18.00 x 27.00 x 36.00 mm"));
    assert!(text.contains("Surface area: 4212.00 mm^2"));
    assert!(text.contains("Volume: 17496.00 mm^3 (17.50 cm^3)"));

    let output = inspect_axes(&["--shrinkage", "20", "--compensate"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Dimensions: 25.00 x 37.50 x 50.00 mm"));
    assert!(text.contains("Surface area: 8125.00 mm^2"));
    assert!(text.contains("Volume: 46875.00 mm^3 (46.88 cm^3)"));

    let output = invoke(&["stl", binary_name, "--units", "cm"]);
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Dimensions: 100.00 x 100.00 x 100.00 mm")
    );
    let output = invoke(&["stl", binary_name, "--units", "in"]);
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Dimensions: 254.00 x 254.00 x 254.00 mm")
    );
    let output = invoke(&["stl", binary_name, "--shrinkage", "10"]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Dimensions: 9.00 x 9.00 x 9.00 mm"));
    assert!(text.contains("Surface area: 486.00 mm^2"));
    assert!(text.contains("Volume: 729.00 mm^3 (0.73 cm^3)"));
    let output = invoke(&["stl", binary_name, "--shrinkage", "20", "--compensate"]);
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Dimensions: 12.50 x 12.50 x 12.50 mm")
    );
    let output = invoke(&["stl", open_path.to_str().unwrap()]);
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Surface area: 550.00 mm^2"));
    assert!(text.contains("Volume: unavailable"));
    assert!(!text.contains("Volume: 0.00"));

    for args in [
        vec!["stl", binary_name, "--units", "yd"],
        vec!["stl", binary_name, "--scale", "0"],
        vec!["stl", binary_name, "--scale", "NaN"],
        vec!["stl", binary_name, "--scale", "inf"],
        vec!["stl", binary_name, "--shrinkage", "100"],
        vec!["stl", binary_name, "--shrinkage=-1"],
        vec!["stl", binary_name, "--compensate"],
        vec!["stl", invalid_path.to_str().unwrap()],
    ] {
        let output = invoke(&args);
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(!output.stderr.is_empty(), "{args:?}");
    }
    for axis in ["--scale-x", "--scale-y", "--scale-z"] {
        for value in ["0", "-1", "NaN", "inf"] {
            let argument = format!("{axis}={value}");
            let output = invoke(&["stl", binary_name, &argument]);
            assert!(!output.status.success(), "{argument}");
            assert!(output.stdout.is_empty(), "{argument}");
            assert!(!output.stderr.is_empty(), "{argument}");
        }
    }
    let output = invoke(&["stl", binary_name, "--scale", "1e308", "--scale-x", "1e308"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    let oversized_path = dir.join("sample-too-large.stl");
    fs::File::create(&oversized_path)
        .unwrap()
        .set_len(jig::stl::MAX_FILE_BYTES as u64 + 1)
        .unwrap();
    assert!(!invoke(&["stl", oversized_path.to_str().unwrap()])
        .status
        .success());
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "STL inspection is read-only"
    );
    assert!(invoke(&["precision", "3"]).status.success());
    let before = fs::read(&path).unwrap();
    let output = invoke(&["stl", binary_name]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Surface area: 600.000 mm^2"));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&binary_path).unwrap(), binary);
    assert_eq!(fs::read(&ascii_path).unwrap(), ascii.as_bytes());
    fs::remove_dir_all(dir).unwrap();
}
