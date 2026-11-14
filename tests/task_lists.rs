use chrono::{NaiveDate, Utc};
use jig::{
    documents::{read_ods, save_ods},
    model::{State, TaskColor, TaskList, DEFAULT_TASK_LIST_ID, MAX_TASK_LISTS},
    spreadsheet::Workbook,
    timeline,
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
fn relative_task_moves_handle_both_directions_and_the_checklist_edges() {
    let cases = [
        (0, 4, true, [1, 2, 3, 4, 0]),
        (4, 0, false, [4, 0, 1, 2, 3]),
        (1, 3, false, [0, 2, 1, 3, 4]),
        (1, 3, true, [0, 2, 3, 1, 4]),
        (3, 1, false, [0, 3, 1, 2, 4]),
        (3, 1, true, [0, 1, 3, 2, 4]),
        (0, 1, false, [0, 1, 2, 3, 4]),
        (1, 0, true, [0, 1, 2, 3, 4]),
    ];
    for (source, target, after, expected) in cases {
        let mut state = State::default();
        let ids: Vec<_> = ["A", "B", "C", "D", "E"]
            .iter()
            .map(|text| state.add_task(text).unwrap())
            .collect();
        state
            .move_task_relative(ids[source], ids[target], after)
            .unwrap();
        assert_eq!(
            state.tasks.iter().map(|task| task.id).collect::<Vec<_>>(),
            expected.map(|index| ids[index]),
            "source={source}, target={target}, after={after}"
        );
        state.validate().unwrap();
    }
}

#[test]
fn relative_task_moves_validate_both_ids_and_leave_self_moves_unchanged() {
    let mut state = State::default();
    let task = state.add_task("Cast study").unwrap();
    state
        .set_task_deadline(task, Some("2026-11-20T18:00:00Z"))
        .unwrap();
    state.feature_task(task).unwrap();
    let before = snapshot(&state);
    for (source, target) in [(999, task), (task, 999), (999, 999), (0, task)] {
        for after in [false, true] {
            assert!(state.move_task_relative(source, target, after).is_err());
            assert_eq!(snapshot(&state), before);
        }
    }
    for after in [false, true] {
        state.move_task_relative(task, task, after).unwrap();
        assert_eq!(snapshot(&state), before);
    }
}

#[test]
fn relative_task_order_roundtrips_and_preserves_lists_metadata_and_timeline() {
    let mut state = State::default();
    let studio = state.default_list_id();
    let home = state.add_task_list("Home").unwrap();
    let hidden = state.add_task_list("Hidden").unwrap();
    let a = state.add_task_to_list(studio, "Cast main form").unwrap();
    let b = state.add_task_to_list(hidden, "Hidden first").unwrap();
    let c = state.add_task_to_list(studio, "Order clay").unwrap();
    let d = state.add_task_to_list(hidden, "Hidden second").unwrap();
    let e = state.add_task_to_list(home, "Repair shelf").unwrap();
    let f = state
        .add_task_to_list(studio, "Photograph studies")
        .unwrap();
    for id in [a, c] {
        state
            .set_task_schedule(
                id,
                Some("2026-11-15T09:00:00-05:00"),
                Some("2026-11-20T18:00:00-05:00"),
            )
            .unwrap();
    }
    state.set_task_color(a, Some(TaskColor::Rose)).unwrap();
    state.set_task_color(c, Some(TaskColor::Sage)).unwrap();
    state
        .set_task_details(a, Some("Cast main form\r\n\r\n  Keep this detail"))
        .unwrap();
    state.toggle_task(c).unwrap();
    state.feature_task(a).unwrap();
    state.add_note("Keep the test tile measurements").unwrap();
    let mut expected = snapshot(&state);
    let ordered: Vec<_> = [b, f, c, d, e, a]
        .iter()
        .map(|id| {
            expected["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|task| task["id"] == *id)
                .unwrap()
                .clone()
        })
        .collect();
    expected["tasks"] = Value::Array(ordered);

    state.move_task_relative(f, c, false).unwrap();
    state.move_task_relative(a, e, true).unwrap();
    assert_eq!(snapshot(&state), expected);
    state.validate().unwrap();

    let json = State::from_json(&serde_json::to_string(&state).unwrap()).unwrap();
    let ods = read_ods(&save_ods(&state).unwrap())
        .unwrap()
        .embedded_state
        .unwrap();
    for restored in [&state, &json, &ods] {
        assert_eq!(snapshot(restored), expected);
        let ids = |lists: &[u64]| {
            timeline::rows_for_lists(restored, &Utc, lists)
                .iter()
                .filter_map(|row| row.task_id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&[studio, home, hidden]), [b, f, c, d, e, a]);
        assert_eq!(ids(&[studio]), [f, c, a]);
        assert_eq!(ids(&[home]), [e]);
        assert_eq!(ids(&[hidden]), [b, d]);
        assert_eq!(ids(&[home, studio]), [f, c, e, a]);
    }
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
