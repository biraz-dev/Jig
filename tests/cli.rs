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
    assert!(invoke(&["tasks", "done", "1"]).status.success());
    let cleared = invoke(&["tasks", "clear-completed"]);
    assert!(cleared.status.success());
    assert!(String::from_utf8_lossy(&cleared.stdout).contains("Removed 1 completed tasks"));
    assert!(invoke(&["tasks"]).stdout.is_empty());
    fs::remove_dir_all(dir).unwrap();
}
