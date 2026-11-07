//! The assistant's command surface for the OneOS host bridge (`craft:command {method: "command.batch"}`).
//!
//! The host relays an allow-listed run: `{label, steps: [{name, args?}]}`. The whole run is judged BEFORE anything runs (an id
//! outside the list, or an argument that names a file or an address, refuses the run), every step runs through the same registry
//! the menus, the control channel and the MCP server use (so a step the package's state forbids is refused exactly as it would be for
//! the person), and the run is ONE undo step named by `label`: a step that fails takes back every step before it, so a run is all or
//! nothing. There is no file, dialog or undo command in the list: files move only through the host.
//!
//! This list is the one declared in the host's `platform-craft` registry as `material` commands (pinned equal there).

use serde_json::{Value, json};

use crate::Session;

/// The plain name the refusals use.
const APP: &str = "Material";

/// Registry commands the assistant may run: EVERY command of the registry except those in [`LEFT_OUT`], each with its reason. A step the
/// document's state forbids is refused by the registry exactly as it would be for the person, and the run is taken back whole.
pub const ASSISTANT_COMMANDS: &[&str] = &[
    "file.new",
    "graph.templates",
    "file.newAtomicSample",
    "graph.input.bind",
    "graph.input.unbind",
    "edit.copy",
    "edit.cut",
    "edit.paste",
    "edit.duplicate",
    "edit.pasteWithoutLinks",
    "edit.duplicateWithoutLinks",
    "edit.delete",
    "edit.deleteRelink",
    "edit.selectAll",
    "edit.selectNone",
    "graph.function.edit",
    "graph.pattern.edit",
    "graph.open",
    "graph.rename",
    "graph.delete",
    "graph.duplicate",
    "graph.size",
    "graph.seed",
    "graph.format",
    "graph.cook",
    "graph.attributes",
    "graph.pin.add",
    "graph.pin.delete",
    "graph.exposed.add",
    "graph.exposed.set",
    "graph.exposed.delete",
    "graph.preset.save",
    "graph.preset.apply",
    "graph.preset.delete",
    "graph.select",
    "graph.selectUpstream",
    "graph.selectDownstream",
    "graph.createFromSelection",
    "graph.align",
    "graph.annotation.select",
    "graph.frame.add",
    "graph.frame.edit",
    "graph.frame.delete",
    "graph.frame.move",
    "graph.function.set",
    "graph.comment.add",
    "graph.comment.edit",
    "graph.comment.delete",
    "graph.node.add",
    "graph.node.move",
    "graph.node.rename",
    "graph.node.disable",
    "graph.node.param",
    "graph.node.reset",
    "graph.node.base",
    "graph.node.bind",
    "graph.node.function",
    "graph.node.edit",
    "graph.pattern.set",
    "graph.connect",
    "graph.disconnect",
    "graph.node.value_input",
    "view.2d",
    "view.3d",
    "view.3d.reset",
    "view.previewSize",
    "library.list",
    "session.inspect",
    "view.3d.outputs",
    "view.3d.assign",
    "view.3d.clear",
    "graph.wire.insertDot",
    "graph.wire.delete",
    "graph.wire.swap",
];

/// The registry commands the assistant may NOT run, each group with its reason (pinned equal to the host's `platform-craft` registry).
pub const LEFT_OUT: &[(&str, &[&str])] = &[
    (
        "files: opens, saves, exports or imports a file, reads a mesh by path, sends to another app's library folder, or replaces the whole package (files move only through the host; the host saves the opened file itself)",
        &["file.newPackage", "file.open", "file.save", "file.saveAs", "file.export", "file.importBitmap", "file.exportMaterial", "file.sendToTextureCraft", "view.3d.loadMesh"],
    ),
    (
        "history: Undo and Redo are the person's, and a run is already ONE undo step (edit.finish would end it)",
        &["edit.undo", "edit.redo", "edit.finish"],
    ),
];

/// Every command id a run may name, in the order the registry declares them.
pub fn allowed() -> Vec<&'static str> {
    ASSISTANT_COMMANDS.to_vec()
}

const MAX_STEPS: usize = 50;
const MAX_LABEL: usize = 120;
/// Argument names that would point a command at a file or a network address: refused anywhere in a step's arguments.
const FORBIDDEN_ARGS: &[&str] = &["path", "paths", "dir", "folder", "file", "url", "uri", "href", "server", "data"];

/// The first forbidden argument name anywhere in `v`.
fn forbidden_arg(v: &Value) -> Option<String> {
    match v {
        Value::Object(o) => o.iter().find_map(|(k, x)| if FORBIDDEN_ARGS.contains(&k.as_str()) { Some(k.clone()) } else { forbidden_arg(x) }),
        Value::Array(a) => a.iter().find_map(forbidden_arg),
        _ => None,
    }
}

/// The run's steps, judged whole before anything runs: `(name, args)` each, or the refusal by name.
fn judge(params: &Value) -> Result<(String, Vec<(&'static str, Value)>), String> {
    let label = params.get("label").and_then(Value::as_str).map(str::trim).unwrap_or("");
    if label.chars().count() > MAX_LABEL {
        return Err(format!("label-too-long: a run's label is at most {MAX_LABEL} characters"));
    }
    let steps = params.get("steps").and_then(Value::as_array).ok_or("run-empty: a run states its steps")?;
    if steps.is_empty() {
        return Err("run-empty: a run states at least one command".into());
    }
    if steps.len() > MAX_STEPS {
        return Err(format!("run-too-long: a run is at most {MAX_STEPS} steps"));
    }
    let mut named = Vec::with_capacity(steps.len());
    for step in steps {
        let name = step.get("name").and_then(Value::as_str).unwrap_or("");
        let Some(id) = ASSISTANT_COMMANDS.iter().copied().find(|c| *c == name) else {
            return Err(format!("command-not-allowed: {APP} does not allow the command '{name}' -- nothing was run"));
        };
        let args = match step.get("args") {
            None | Some(Value::Null) => json!({}),
            Some(a @ Value::Object(_)) => a.clone(),
            Some(_) => return Err(format!("argument-not-allowed: step '{name}' states args that are not an object -- nothing was run")),
        };
        if let Some(k) = forbidden_arg(&args) {
            return Err(format!("argument-not-allowed: step '{name}' names '{k}', and a run does not reach files or addresses -- nothing was run"));
        }
        named.push((id, args));
    }
    let label = if label.is_empty() { "Assistant".to_string() } else { label.to_string() };
    Ok((label, named))
}


/// One `command.batch`: all of it, as ONE undo step named by its label, or none of it (the package as it was before the run).
pub fn run_command_batch(session: &mut Session, params: &Value) -> Result<Value, String> {
    let (label, named) = judge(params)?;
    let depth = session.history().len();
    session.finish_edit();
    let mut results: Vec<Value> = Vec::with_capacity(named.len());
    let mut failure: Option<String> = None;
    let outcome = session.group(&label, |s| {
        for (i, (name, args)) in named.iter().enumerate() {
            match crate::execute(s, name, args) {
                Ok(v) => results.push(json!({ "name": name, "result": v })),
                Err(e) => {
                    failure = Some(format!("step-failed: step {} ({name}): {e} -- nothing was changed", i + 1));
                    return Err(crate::EngineError::Other(e.to_string()));
                }
            }
        }
        Ok(())
    });
    session.finish_edit();
    match outcome {
        Ok(()) => Ok(json!({ "label": label, "steps": results, "undoStep": session.history().len() > depth })),
        Err(e) => Err(failure.unwrap_or_else(|| format!("step-failed: {e} -- nothing was changed"))),
    }
}
