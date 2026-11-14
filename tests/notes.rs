use chrono::{NaiveDate, Utc};
use jig::{
    documents::{read_ods, save_ods},
    model::{note_text, State, TaskColor, MAX_NOTES, MAX_NOTE_BYTES},
    spreadsheet::Workbook,
    timeline,
};
use serde_json::{json, Value};

fn snapshot(state: &State) -> Value {
    serde_json::to_value(state).unwrap()
}

fn note_ids(state: &State) -> Vec<u64> {
    state.notes.iter().map(|note| note.id).collect()
}

#[test]
fn old_files_have_no_notes_or_task_details_and_serialize_without_new_fields() {
    let old = json!({
        "version": 1,
        "preferences": { "decimals": 2, "focus": 3, "weather": null },
        "history": [],
        "tasks": [{ "id": 1, "text": "Cast studies", "complete": false }],
        "deadlines": [], "entries": [], "featured": null
    });
    let state = State::from_json(&old.to_string()).unwrap();
    assert!(state.notes.is_empty());
    assert!(state.tasks[0].details.is_none());
    assert_eq!(snapshot(&state), old);
}

#[test]
fn plain_note_crud_preserves_formatting_and_rejects_invalid_text_atomically() {
    let text = "\n  Studio ideas\r\n\tline two\r\nhttps://example.com/a?b=2\n";
    assert_eq!(note_text(text).unwrap(), text);
    let mut state = State::default();
    let note = state.add_note(text).unwrap();
    assert_eq!(state.notes[0].text, text);
    assert!(!state.notes[0].pinned);
    assert!(state.notes[0].task_origin.is_none());
    let edited = "\tA different idea\n\tWith indentation\n";
    state.edit_note(note, edited).unwrap();
    assert_eq!(state.notes[0].text, edited);
    for text in [
        "",
        " \n\t ",
        "Escape\u{1b}[31m",
        "Null\0",
        "Back\u{8}",
        "Delete\u{7f}",
        "Bare\rcarriage",
    ] {
        let before = snapshot(&state);
        assert!(state.add_note(text).is_err(), "{text:?}");
        assert!(state.edit_note(note, text).is_err(), "{text:?}");
        assert_eq!(snapshot(&state), before);
    }
    let max = "é".repeat(MAX_NOTE_BYTES / 2);
    state.edit_note(note, &max).unwrap();
    let before = snapshot(&state);
    assert!(state.edit_note(note, &format!("{max}a")).is_err());
    assert_eq!(snapshot(&state), before, "The limit counts UTF-8 bytes");
    assert!(state.edit_note(999, "Valid text").is_err());
    state.remove_note(note).unwrap();
    assert!(state.notes.is_empty());
    assert!(state.remove_note(note).is_err());
    state.validate().unwrap();
}

#[test]
fn pinning_and_drag_reordering_preserve_both_groups_and_other_work() {
    let mut state = State::default();
    let task = state.add_task("Featured work").unwrap();
    state
        .set_task_deadline(task, Some("2026-11-20T18:00:00Z"))
        .unwrap();
    state.feature_task(task).unwrap();
    let before_work = snapshot(&state);
    let a = state.add_note("A").unwrap();
    let b = state.add_note("B").unwrap();
    let c = state.add_note("C").unwrap();
    let d = state.add_note("D").unwrap();
    state.set_note_pinned(c, true).unwrap();
    state.set_note_pinned(d, true).unwrap();
    assert_eq!(note_ids(&state), [c, d, a, b]);
    state.move_note(d, 1).unwrap();
    assert_eq!(note_ids(&state), [d, c, a, b]);
    state.move_note_relative(a, b, true).unwrap();
    assert_eq!(note_ids(&state), [d, c, b, a]);
    let ordered = snapshot(&state);
    assert!(state.move_note_relative(d, a, false).is_err());
    assert!(state.move_note(d, 0).is_err());
    assert!(state.move_note(d, 3).is_err());
    assert!(state.move_note_relative(d, 999, true).is_err());
    assert_eq!(snapshot(&state), ordered);
    state.move_note_relative(d, d, true).unwrap();
    assert_eq!(snapshot(&state), ordered);
    state.set_note_pinned(d, false).unwrap();
    assert_eq!(note_ids(&state), [c, d, b, a]);
    state.set_note_pinned(b, true).unwrap();
    assert_eq!(note_ids(&state), [c, b, d, a]);
    let mut after_work = snapshot(&state);
    after_work.as_object_mut().unwrap().remove("notes");
    assert_eq!(
        after_work, before_work,
        "Pinning must not change tasks or the countdown"
    );
    state.validate().unwrap();
}

#[test]
fn note_task_conversion_moves_the_same_record_and_preserves_exact_full_text() {
    let mut state = State::default();
    let studio = state.add_task_list("Studio").unwrap();
    let full = "\n\n  Photograph sculptures  \r\n\tUse soft light\nhttps://example.com/reference\n";
    let id = state.add_note(full).unwrap();
    state.set_note_pinned(id, true).unwrap();
    assert_eq!(state.note_to_task(id, studio).unwrap(), id);
    assert!(state.notes.is_empty());
    assert_eq!(state.tasks.len(), 1);
    assert_eq!(state.tasks[0].id, id);
    assert_eq!(state.tasks[0].text, "Photograph sculptures");
    assert_eq!(state.tasks[0].details.as_deref(), Some(full));
    assert_eq!(state.tasks[0].list_id, studio);
    assert!(!state.tasks[0].complete);
    assert!(state.tasks[0].deadline.is_none());
    assert!(state.tasks[0].color.is_none());
    assert_eq!(state.task_to_note(id).unwrap(), id);
    assert!(state.tasks.is_empty());
    assert_eq!(state.notes[0].text, full);
    assert!(
        !state.notes[0].pinned,
        "Task star and note pin are independent"
    );
    assert_eq!(state.notes[0].id, id);
    state.validate().unwrap();
}

#[test]
fn editing_a_converted_task_keeps_its_source_body_and_new_name_on_return() {
    let mut state = State::default();
    let source = "\n  Original idea\n\tAn indented body\n";
    let id = state.add_note(source).unwrap();
    state.note_to_task(id, state.default_list_id()).unwrap();
    state.edit_task(id, "Revised plan").unwrap();
    assert_eq!(state.tasks[0].details.as_deref(), Some(source));
    state.task_to_note(id).unwrap();
    let expected = format!("Revised plan\n{source}");
    assert_eq!(state.notes[0].text, expected);
    state.note_to_task(id, state.default_list_id()).unwrap();
    state.task_to_note(id).unwrap();
    assert_eq!(
        state.notes[0].text, expected,
        "Repeated moves must not keep prefixing the title"
    );

    let invalid_title = state.add_note("A\ttabbed name\nBody").unwrap();
    let before = snapshot(&state);
    assert!(state
        .note_to_task(invalid_title, state.default_list_id())
        .is_err());
    assert_eq!(
        snapshot(&state),
        before,
        "An invalid task title must leave the note intact"
    );
}

#[test]
fn archived_schedules_disappear_from_gantt_and_calendar_and_keep_their_ids_reserved() {
    let mut state = State::default();
    let other = state.add_task("Other opening").unwrap();
    state
        .set_task_deadline(other, Some("2026-11-20T19:00:00Z"))
        .unwrap();
    let other_deadline = state.task_deadline(other).unwrap().id;
    let studio = state.add_task_list("Studio").unwrap();
    let task = state.add_task_to_list(studio, "Casting").unwrap();
    state
        .set_task_schedule(
            task,
            Some("2026-11-15T09:13:27Z"),
            Some("2026-11-20T18:04:56Z"),
        )
        .unwrap();
    state.set_task_color(task, Some(TaskColor::Rose)).unwrap();
    state.feature_task(task).unwrap();
    let original = state
        .tasks
        .iter()
        .find(|item| item.id == task)
        .unwrap()
        .clone();
    let deadline = state.task_deadline(task).unwrap().clone();
    state.task_to_note(task).unwrap();
    assert!(state.featured.is_none());
    assert_eq!(state.tasks.len(), 1);
    assert_eq!(state.deadlines.len(), 1);
    assert!(timeline::rows(&state, &Utc)
        .iter()
        .all(|row| row.task_id != Some(task)));
    let day = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
    assert!(state
        .tasks_on_date(day, &Utc)
        .iter()
        .all(|row| row.task_id != Some(task)));
    let origin = state.notes[0].task_origin.as_ref().unwrap();
    assert_eq!(origin.deadline.as_ref().unwrap().id, deadline.id);
    assert_eq!(origin.start, original.start);
    assert_eq!(origin.color, original.color);
    assert_eq!(origin.list_id, studio);
    assert!(origin.was_featured);
    let allocated = state.add_task("Later task").unwrap();
    assert!(
        allocated > deadline.id,
        "An inactive archived deadline ID must not be reused"
    );
    state.feature_task(other).unwrap();
    state
        .edit_note(task, "Updated casting\nAdditional instructions")
        .unwrap();
    state.note_to_task(task, studio).unwrap();
    let restored = state.tasks.iter().find(|item| item.id == task).unwrap();
    assert_eq!(restored.color, original.color);
    assert_eq!(restored.start, original.start);
    assert_eq!(restored.complete, original.complete);
    assert_eq!(restored.list_id, original.list_id);
    assert_eq!(state.task_deadline(task).unwrap().id, deadline.id);
    assert_eq!(state.task_deadline(task).unwrap().at, deadline.at);
    assert_eq!(state.task_deadline(task).unwrap().title, "Updated casting");
    assert_eq!(
        state.featured,
        Some(other_deadline),
        "Restoring an old star must not replace the current one"
    );
    assert_eq!(state.tasks.iter().filter(|item| item.id == task).count(), 1);
    assert!(state.notes.is_empty());
    state.validate().unwrap();
}

#[test]
fn completed_tasks_and_featured_countdowns_restore_without_crossing_semantics() {
    let mut state = State::default();
    let task = state.add_task("Featured casting").unwrap();
    state
        .set_task_deadline(task, Some("2026-11-20T18:00:00Z"))
        .unwrap();
    state.feature_task(task).unwrap();
    let featured = state.featured;
    state.task_to_note(task).unwrap();
    state.set_note_pinned(task, true).unwrap();
    assert!(state.featured.is_none());
    state.note_to_task(task, state.default_list_id()).unwrap();
    assert_eq!(
        state.featured, featured,
        "An archived star may return when no other task is featured"
    );
    state.toggle_task(task).unwrap();
    assert!(state.featured.is_none());
    state.task_to_note(task).unwrap();
    state.note_to_task(task, state.default_list_id()).unwrap();
    assert!(state.tasks[0].complete);
    assert!(state.featured.is_none());
    state.validate().unwrap();
}

#[test]
fn removed_task_lists_do_not_destroy_archived_tasks_or_allow_invalid_restore_targets() {
    let mut state = State::default();
    let target = state.default_list_id();
    let studio = state.add_task_list("Studio").unwrap();
    let task = state.add_task_to_list(studio, "Archived work").unwrap();
    state
        .set_task_deadline(task, Some("2026-11-20T18:00:00Z"))
        .unwrap();
    state.task_to_note(task).unwrap();
    state.remove_task_list(studio, target).unwrap();
    state.validate().unwrap();
    let before = snapshot(&state);
    assert!(state.note_to_task(task, studio).is_err());
    assert_eq!(snapshot(&state), before);
    let mut reloaded = State::from_json(&serde_json::to_string(&state).unwrap()).unwrap();
    reloaded.note_to_task(task, target).unwrap();
    assert_eq!(reloaded.tasks[0].list_id, target);
    assert!(reloaded.task_deadline(task).is_some());
    reloaded.validate().unwrap();
}

#[test]
fn the_eighty_one_note_cap_and_detail_errors_never_consume_a_task() {
    let mut state = State::default();
    let task = state.add_task("Still active").unwrap();
    state
        .set_task_deadline(task, Some("2026-11-20T18:00:00Z"))
        .unwrap();
    state.feature_task(task).unwrap();
    for index in 0..MAX_NOTES {
        state.add_note(&format!("Note {index}")).unwrap();
    }
    let before = snapshot(&state);
    assert!(state.add_note("Eighty second").is_err());
    assert!(state.task_to_note(task).is_err());
    assert_eq!(snapshot(&state), before);
    state.remove_note(state.notes[0].id).unwrap();
    state.task_to_note(task).unwrap();
    assert_eq!(state.notes.len(), MAX_NOTES);
    assert!(state.tasks.is_empty());

    let mut state = State::default();
    let task = state.add_task("Name").unwrap();
    let full_body = "x".repeat(MAX_NOTE_BYTES);
    state.set_task_details(task, Some(&full_body)).unwrap();
    let before = snapshot(&state);
    assert!(
        state.task_to_note(task).is_err(),
        "Prepending a changed title must still respect the byte limit"
    );
    assert!(state
        .set_task_details(task, Some(&format!("{full_body}x")))
        .is_err());
    assert!(state
        .set_task_details(task, Some("Unsafe\u{1b}text"))
        .is_err());
    assert_eq!(snapshot(&state), before);
    state.set_task_details(task, None).unwrap();
    state.task_to_note(task).unwrap();
    assert_eq!(state.notes[0].text, "Name");
}

#[test]
fn notes_archives_and_full_source_text_roundtrip_through_json_ods_and_undo_clones() {
    let mut state = State::default();
    let first = state
        .add_note("Pinned\n\tRemember this URL: https://example.com")
        .unwrap();
    state.set_note_pinned(first, true).unwrap();
    let task = state.add_task("Scheduled task").unwrap();
    state
        .set_task_schedule(
            task,
            Some("2026-11-15T09:00:00Z"),
            Some("2026-11-20T18:00:00Z"),
        )
        .unwrap();
    state.feature_task(task).unwrap();
    state.task_to_note(task).unwrap();
    state.workbook = Some(Workbook::default());
    let before = snapshot(&state);
    let restored = State::from_json(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(snapshot(&restored), before);
    let package = save_ods(&state).unwrap();
    assert_eq!(
        snapshot(&read_ods(&package).unwrap().embedded_state.unwrap()),
        before
    );
    let undo = state.clone();
    state.edit_note(first, "Changed").unwrap();
    state.remove_note(task).unwrap();
    state = undo;
    assert_eq!(snapshot(&state), before);
    state.validate().unwrap();
}

#[test]
fn imports_reject_invalid_notes_and_colliding_active_or_archived_ids() {
    let mut state = State::default();
    let task = state.add_task("Archive").unwrap();
    state
        .set_task_deadline(task, Some("2026-11-20T18:00:00Z"))
        .unwrap();
    state.task_to_note(task).unwrap();
    let other = state.add_task("Active").unwrap();
    let ordinary = state.add_note("Other note").unwrap();
    let valid = snapshot(&state);
    let mut invalids = Vec::new();
    for text in [
        json!(""),
        json!(" \n\t"),
        json!("Danger\u{1b}"),
        json!("x".repeat(MAX_NOTE_BYTES + 1)),
    ] {
        let mut value = valid.clone();
        value["notes"][0]["text"] = text;
        invalids.push(value);
    }
    for id in [0, other, ordinary, task + 1] {
        let mut value = valid.clone();
        value["notes"][0]["id"] = json!(id);
        invalids.push(value);
    }
    for id in [0, task, other, ordinary] {
        let mut value = valid.clone();
        value["notes"][0]["task_origin"]["deadline"]["id"] = json!(id);
        invalids.push(value);
    }
    let mut bad_order = valid.clone();
    bad_order["notes"][1]["pinned"] = json!(true);
    invalids.push(bad_order);
    let mut too_many = valid.clone();
    too_many["notes"] = json!((1..=82)
        .map(|id| json!({"id": id + 100,"text":"Note","pinned":false}))
        .collect::<Vec<_>>());
    invalids.push(too_many);
    let mut bad_origin = valid.clone();
    bad_origin["notes"][0]["task_origin"]["list_id"] = json!(0);
    invalids.push(bad_origin);
    let mut bad_start = valid.clone();
    bad_start["notes"][0]["task_origin"]["start"] = json!("2026-12-01T00:00:00Z");
    invalids.push(bad_start);
    let mut bad_star = valid.clone();
    bad_star["notes"][0]["task_origin"]["was_featured"] = json!(true);
    bad_star["notes"][0]["task_origin"]["complete"] = json!(true);
    invalids.push(bad_star);
    for invalid in invalids {
        assert!(
            State::from_json(&invalid.to_string()).is_err(),
            "Invalid note metadata was accepted: {invalid}"
        );
    }
    assert_eq!(snapshot(&state), valid);
}
