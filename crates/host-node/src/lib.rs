//! One Material editor window's host side for node: the engine the browser build runs, so OneOS's `craft-material-ai-drive.test.ts` can
//! drive a window through the SAME `command.batch` the browser build answers `craft:command` with (`materialcraft_engine::host_commands`),
//! open a package as the host does (`host_open`), and read back the file the editor saves.

use materialcraft_engine::{Session, host_commands, host_open};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

fn err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

#[wasm_bindgen]
pub struct Window {
    session: Session,
    name: String,
}

#[wasm_bindgen]
impl Window {
    /// A window the host opened `name` in (`craft:open`): the package, or the refusal by name.
    #[wasm_bindgen(constructor)]
    pub fn new(name: &str, b64: &str) -> Result<Window, JsError> {
        let mut session = materialcraft_engine::new_session();
        host_open::open_host_file(&mut session, name, &io_b64(b64)?).map_err(err)?;
        Ok(Window { session, name: name.to_string() })
    }

    /// The assistant's run (`craft:command`, `command.batch`): the answer as JSON text, or an error `<code>: <reason>`.
    pub fn command_batch(&mut self, params_json: &str) -> Result<String, JsError> {
        let params: Value = serde_json::from_str(params_json).map_err(err)?;
        host_commands::run_command_batch(&mut self.session, &params).map(|v| v.to_string()).map_err(|e| JsError::new(&e))
    }

    /// The person presses Save: the file the host is handed, as `{name, b64}` (the name is the opened file's own).
    pub fn save(&mut self) -> Result<String, JsError> {
        let bytes = host_open::save_host_file(&mut self.session).map_err(|e| JsError::new(&e))?;
        Ok(json!({ "name": self.name, "b64": b64(&bytes) }).to_string())
    }

    /// How many undo steps the open package holds.
    pub fn undo_depth(&self) -> usize {
        self.session.history().len()
    }

    /// The label of the last undo step.
    pub fn undo_label(&self) -> Option<String> {
        self.session.history().last().cloned()
    }

    /// Undo one step (the person's Undo).
    pub fn undo(&mut self) -> bool {
        self.session.undo().is_some()
    }

    /// Whether the package has unsaved changes.
    pub fn dirty(&self) -> bool {
        self.session.modified
    }

    /// The package as JSON (`session.inspect`), the oracle a test reads back.
    pub fn inspect(&self) -> String {
        materialcraft_engine::inspect::session(&self.session, None).to_string()
    }
}

const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 (with padding).
fn b64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

fn io_b64(s: &str) -> Result<Vec<u8>, JsError> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = TABLE.iter().position(|t| *t == c).ok_or_else(|| JsError::new("not base64"))? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}
