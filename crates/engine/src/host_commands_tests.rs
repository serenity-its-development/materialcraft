//! The host bridge's command surface and the file it opens and saves (Material).

use serde_json::json;

use crate::host_commands::{ASSISTANT_COMMANDS, LEFT_OUT, allowed, run_command_batch};
use crate::host_open::{Refusal, open_host_file, save_host_file};
use crate::{Session, command_specs};

fn package() -> Vec<u8> {
    crate::file_cmds::save_bytes(&crate::sample_session()).unwrap()
}

fn opened() -> Session {
    let mut s = crate::new_session();
    open_host_file(&mut s, "bricks.sbs", &package()).unwrap();
    s
}

#[test]
fn every_registry_command_is_allowed_or_left_out_with_a_reason() {
    let mut accounted = std::collections::BTreeSet::new();
    for id in allowed() {
        assert!(accounted.insert(id), "{id} is listed twice");
    }
    for (reason, ids) in LEFT_OUT {
        assert!(!reason.trim().is_empty(), "a group left out states its reason");
        for id in *ids {
            assert!(accounted.insert(id), "{id} is both allowed and left out, or left out twice");
        }
    }
    for spec in command_specs() {
        assert!(accounted.contains(spec.id), "{} is a registry command that is neither allowed nor left out with a reason", spec.id);
    }
    for id in &accounted {
        assert!(crate::find(id).is_some(), "{id} is not a registry command");
    }
    for id in ASSISTANT_COMMANDS {
        let params = crate::find(id).map(|c| c.params).unwrap_or_default();
        assert!(!params.contains("{path") && !params.contains("{dir"), "{id} reaches a file: {params}");
        assert!(!matches!(*id, "edit.undo" | "edit.redo" | "edit.finish"), "{id} is the person's history");
    }
}

#[test]
fn a_package_opens_from_bytes_and_saves_back_as_itself() {
    let mut s = opened();
    assert_eq!(s.path.as_deref(), Some("bricks.sbs"));
    assert!(!s.modified, "a freshly opened file is not unsaved work");
    let saved = save_host_file(&mut s).unwrap();
    let mut again = crate::new_session();
    open_host_file(&mut again, "bricks.sbs", &saved).unwrap();
    assert_eq!(again.pkg.graphs.len(), s.pkg.graphs.len());
}

#[test]
fn anything_material_would_not_write_back_unchanged_is_refused_by_name() {
    let mut s = crate::new_session();
    let png = open_host_file(&mut s, "wood.png", b"\x89PNG\r\n\x1a\n").unwrap_err();
    assert!(matches!(png, Refusal::NotEditable(_)), "{png}");
    assert!(png.to_string().starts_with("format-not-editable:"), "{png}");
    let junk = open_host_file(&mut s, "broken.sbs", b"not a package").unwrap_err();
    assert!(junk.to_string().starts_with("file-unreadable:"), "{junk}");
}

#[test]
fn a_run_is_judged_whole_first_and_refuses_files_and_unknown_ids() {
    let mut s = opened();
    let before = s.history().len();
    let e = run_command_batch(&mut s, &json!({ "steps": [{ "name": ASSISTANT_COMMANDS[0] }, { "name": "file.save" }] })).unwrap_err();
    assert!(e.starts_with("command-not-allowed:"), "{e}");
    let e = run_command_batch(&mut s, &json!({ "steps": [{ "name": ASSISTANT_COMMANDS[0], "args": { "path": "/etc/passwd" } }] })).unwrap_err();
    assert!(e.starts_with("argument-not-allowed:"), "{e}");
    let e = run_command_batch(&mut s, &json!({ "steps": [] })).unwrap_err();
    assert!(e.starts_with("run-empty:"), "{e}");
    assert_eq!(s.history().len(), before, "nothing ran");
    assert!(!s.modified);
}

#[test]
fn a_run_is_one_undo_step_named_by_its_label_and_the_saved_file_carries_it() {
    let mut s = opened();
    let depth = s.history().len();
    let nodes = s.graph().unwrap().nodes.len();
    let def = "uniform";
    let r = run_command_batch(
        &mut s,
        &json!({ "label": "Add a node and note it", "steps": [
            { "name": "graph.node.add", "args": { "def": def, "at": [40, 40] } },
            { "name": "graph.comment.add", "args": { "text": "From the assistant" } },
        ] }),
    )
    .unwrap();
    assert_eq!(r["undoStep"], true, "{r}");
    assert_eq!(s.history().len(), depth + 1, "the steps are one");
    assert_eq!(s.history().last().map(String::as_str), Some("Add a node and note it"));
    assert_eq!(s.graph().unwrap().nodes.len(), nodes + 1);
    assert!(s.modified);
    let saved = save_host_file(&mut s).unwrap();
    let mut again = crate::new_session();
    open_host_file(&mut again, "bricks.sbs", &saved).unwrap();
    assert_eq!(again.graph().unwrap().nodes.len(), nodes + 1, "the saved package holds the run");
    assert!(crate::inspect::session(&again, None).to_string().contains("From the assistant"), "the comment is saved");
    s.undo();
    assert_eq!(s.graph().unwrap().nodes.len(), nodes, "Undo takes the whole run back");
}

#[test]
fn a_step_that_fails_takes_the_whole_run_back() {
    let mut s = opened();
    let nodes = s.graph().unwrap().nodes.len();
    let def = "uniform";
    let e = run_command_batch(
        &mut s,
        &json!({ "steps": [
            { "name": "graph.node.add", "args": { "def": def } },
            { "name": "graph.node.param", "args": { "id": 999_999, "param": "nope", "value": 1 } },
        ] }),
    )
    .unwrap_err();
    assert!(e.starts_with("step-failed: step 2"), "{e}");
    assert_eq!(s.graph().unwrap().nodes.len(), nodes);
    assert!(!s.modified);
}

#[test]
fn a_node_label_the_package_cannot_carry_is_refused_by_name() {
    let mut s = opened();
    assert!(!ASSISTANT_COMMANDS.contains(&"graph.node.rename"));
    let e = run_command_batch(&mut s, &json!({ "steps": [{ "name": "graph.node.add", "args": { "def": "uniform", "label": "Lost" } }] })).unwrap_err();
    assert!(e.starts_with("argument-not-allowed:"), "{e}");
    assert!(!s.modified);
}
