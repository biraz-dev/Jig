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
