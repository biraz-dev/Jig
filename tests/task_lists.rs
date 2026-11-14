use chrono::{NaiveDate, Utc};
use jig::{
    documents::{read_ods, save_ods},
    model::{State, TaskColor, TaskList, DEFAULT_TASK_LIST_ID, MAX_TASK_LISTS},
    spreadsheet::Workbook,
};
use serde_json::{json, Value};

fn snapshot(state: &State) -> Value {
    serde_json::to_value(state).unwrap()
}

#[test]
fn legacy_files_keep_one_list_without_rewriting_their_json() {
    let old = json!({
        "version": 1,
        "preferences": { "decimals": 2, "focus": 3, "weather": null },
        "history": [],
        "tasks": [{ "id": 1, "text": "Cast studies", "complete": false }],
        "deadlines": [{ "id": 2, "title": "Opening", "at": "2026-11-20T18:00:00Z" }],
        "entries": [],
        "featured": 2
    });
    let state = State::from_json(&old.to_string()).unwrap();
    assert_eq!(state.default_list_id(), DEFAULT_TASK_LIST_ID);
    assert_eq!(state.task_lists.len(), 1);
    assert_eq!(state.task_lists[0].name, "Tasks");
    assert_eq!(state.tasks[0].list_id, DEFAULT_TASK_LIST_ID);
    assert_eq!(snapshot(&state), old);
    let day = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
    let projected = state.tasks_on_date(day, &Utc);
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].list_id, DEFAULT_TASK_LIST_ID);
    assert_eq!(projected[0].task_id, None);
    assert_eq!(
        snapshot(&state),
        old,
        "Projection must not create task records"
    );
}

#[test]
fn list_names_and_five_list_cap_are_validated_before_mutation() {
    let mut state = State::default();
    let studio = state.add_task_list("  Studio  ").unwrap();
    assert_eq!(state.task_list(studio).unwrap().name, "Studio");
    state.rename_task_list(studio, "Sculpture").unwrap();
    assert_eq!(state.task_list(studio).unwrap().name, "Sculpture");
    for name in ["", " \t ", "New\nlist", "tasks"] {
        let before = snapshot(&state);
        assert!(state.add_task_list(name).is_err(), "{name:?}");
        assert_eq!(snapshot(&state), before);
        assert!(state.rename_task_list(studio, name).is_err(), "{name:?}");
        assert_eq!(snapshot(&state), before);
    }
    assert!(state.add_task_list("SCULPTURE").is_err());
    state.rename_task_list(studio, "SCULPTURE").unwrap();
    assert_eq!(state.task_list(studio).unwrap().name, "SCULPTURE");
    let long = "é".repeat(65);
    assert!(state.add_task_list(&long).is_err());
    for name in ["Packing", "Invoices", "Home"] {
        state.add_task_list(name).unwrap();
    }
    assert_eq!(state.task_lists.len(), MAX_TASK_LISTS);
    let capped = snapshot(&state);
    assert!(state.add_task_list("Sixth").is_err());
    assert_eq!(snapshot(&state), capped);
    state.validate().unwrap();
}

#[test]
fn moving_and_removing_lists_preserves_tasks_dates_colors_and_countdown() {
    let mut state = State::default();
    let first = state.default_list_id();
    let studio = state.add_task_list("Studio").unwrap();
    let home = state.add_task_list("Home").unwrap();
    let task = state
        .add_task_to_list(studio, "Photograph sculptures")
        .unwrap();
    state
        .set_task_schedule(
            task,
            Some("2026-11-15T09:00:00-05:00"),
            Some("2026-11-20T18:00:00-05:00"),
        )
        .unwrap();
    state.set_task_color(task, Some(TaskColor::Rose)).unwrap();
    state.feature_task(task).unwrap();
    let other = state.add_task_to_list(home, "Repair shelf").unwrap();
    let complete = state.add_task_to_list(studio, "Order clay").unwrap();
    state.toggle_task(complete).unwrap();
    let before = snapshot(&state);
    for operation in [
        state.set_task_list(task, 999),
        state.set_task_list(999, home),
        state.remove_task_list(studio, studio),
        state.remove_task_list(studio, 999),
        state.remove_task_list(999, first),
    ] {
        assert!(operation.is_err());
    }
    assert_eq!(
        snapshot(&state),
        before,
        "Rejected list operations must be atomic"
    );
    state.set_task_list(task, first).unwrap();
    assert_eq!(state.tasks[0].list_id, first);
    state.set_task_list(task, studio).unwrap();
    state.remove_task_list(studio, home).unwrap();
    let mut expected = before;
    expected["task_lists"]
        .as_array_mut()
        .unwrap()
        .retain(|list| list["id"] != studio);
    for item in expected["tasks"].as_array_mut().unwrap() {
        if item["list_id"] == studio {
            item["list_id"] = json!(home);
        }
    }
    assert_eq!(snapshot(&state), expected);
    assert_eq!(
        state.tasks.iter().map(|task| task.id).collect::<Vec<_>>(),
        [task, other, complete]
    );
    assert!(state.tasks[2].complete);
    assert_eq!(state.featured, state.tasks[0].deadline);
    state.validate().unwrap();

    // Removing the original default list is valid. Old CLI callers subsequently
    // add to the first surviving list, and legacy deadlines follow that list.
    state.remove_task_list(first, home).unwrap();
    assert_eq!(state.default_list_id(), home);
    let added = state.add_task("Pack tools").unwrap();
    assert_eq!(
        state
            .tasks
            .iter()
            .find(|task| task.id == added)
            .unwrap()
            .list_id,
        home
    );
    let before_last = snapshot(&state);
    assert!(state.remove_task_list(home, home).is_err());
    assert_eq!(snapshot(&state), before_last);
}

#[test]
fn scoped_completed_clear_keeps_hidden_lists_and_reuses_deadline_records() {
    let mut state = State::default();
    let first = state.default_list_id();
    let second = state.add_task_list("Home").unwrap();
    let open = state.add_task("Open studio task").unwrap();
    let selected = state.add_task("Finished studio task").unwrap();
    let hidden = state
        .add_task_to_list(second, "Finished home task")
        .unwrap();
    for task in [selected, hidden] {
        state
            .set_task_deadline(task, Some("2026-11-20T18:00:00Z"))
            .unwrap();
        state.toggle_task(task).unwrap();
    }
    let hidden_deadline = state.task_deadline(hidden).unwrap().id;
    assert_eq!(state.remove_completed_tasks_in_lists(&[first]), 1);
    assert_eq!(
        state.tasks.iter().map(|task| task.id).collect::<Vec<_>>(),
        [open, hidden]
    );
    assert_eq!(state.deadlines.len(), 1);
    assert_eq!(state.deadlines[0].id, hidden_deadline);
    assert_eq!(state.remove_completed_tasks_in_lists(&[]), 0);
    assert_eq!(state.remove_completed_tasks(), 1);
    assert_eq!(state.tasks.len(), 1);
    assert!(state.deadlines.is_empty());
    state.validate().unwrap();
}

#[test]
fn lists_roundtrip_in_json_and_ods_and_state_clones_restore_changes() {
    let mut state = State::default();
    let studio = state.add_task_list("Sculpture").unwrap();
    let task = state.add_task_to_list(studio, "Cast form").unwrap();
    state
        .set_task_deadline(task, Some("2026-11-20T18:00:00Z"))
        .unwrap();
    state.set_task_color(task, Some(TaskColor::Sage)).unwrap();
    state.workbook = Some(Workbook::default());
    let before = snapshot(&state);
    let restored = State::from_json(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(snapshot(&restored), before);
    let ods = save_ods(&state).unwrap();
    let imported = read_ods(&ods).unwrap().embedded_state.unwrap();
    assert_eq!(snapshot(&imported), before);
    let undo = state.clone();
    state
        .remove_task_list(studio, state.default_list_id())
        .unwrap();
    assert_ne!(snapshot(&state), before);
    state = undo;
    assert_eq!(snapshot(&state), before);
    state.validate().unwrap();
}

#[test]
fn imports_reject_invalid_list_ids_names_counts_and_membership() {
    let mut state = State::default();
    state.add_task("Existing work").unwrap();
    let valid = snapshot(&state);
    for lists in [
        json!([]),
        json!([{ "id": 0, "name": "Tasks" }]),
        json!([{ "id": 1, "name": "Tasks" }, { "id": 1, "name": "Home" }]),
        json!([{ "id": 1, "name": "Tasks" }, { "id": 2, "name": "TASKS" }]),
        json!([{ "id": 1, "name": " Tasks " }]),
        json!([{ "id": 1, "name": "" }]),
        json!([{ "id": 1, "name": "Line\nbreak" }]),
        json!((1..=6)
            .map(|id| TaskList {
                id,
                name: format!("List {id}")
            })
            .collect::<Vec<_>>()),
    ] {
        let mut invalid = valid.clone();
        invalid["task_lists"] = lists;
        assert!(State::from_json(&invalid.to_string()).is_err(), "{invalid}");
    }
    let mut invalid = valid.clone();
    invalid["tasks"][0]["list_id"] = json!(999);
    assert!(State::from_json(&invalid.to_string()).is_err());
    assert_eq!(snapshot(&state), valid);
}
