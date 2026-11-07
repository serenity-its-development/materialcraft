//! The file the OneOS host opens in Material, and the bytes a save hands back.
//!
//! Material edits ONE format: a Substance package (`.sbs`), opened from the host's bytes and saved back as `.sbs` under the file's own
//! name (a new version of it). A package that holds content Material cannot write back (`read_sbs` reports it as a warning) would lose
//! that content on save, so it is refused by name and not opened for editing; so is any other file (a `.mcmat` material, a picture, a
//! mesh): those are imports or exports, never a document a save could write back unchanged.

use crate::Session;

/// The extension Material edits.
pub const EDITS: &[&str] = &["sbs"];

/// Why a file is not opened for editing, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Not a format Material writes back unchanged.
    NotEditable(String),
    /// A package with content Material would drop on save.
    NotRoundTrip { name: String, warnings: Vec<String> },
    /// The bytes are not a package Material reads.
    Unreadable { name: String, error: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotEditable(name) => write!(f, "format-not-editable: Material edits Substance packages (.sbs); {name} is not opened for editing"),
            Self::NotRoundTrip { name, warnings } => write!(
                f,
                "format-not-round-trip: {name} holds content Material cannot write back ({}), so a save would lose it; it is not opened for editing",
                warnings.join("; ")
            ),
            Self::Unreadable { name, error } => write!(f, "file-unreadable: {name} could not be read as a Substance package: {error}"),
        }
    }
}

fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// Open what the host sent as the session's package. The session is left as it was on any refusal.
pub fn open_host_file(session: &mut Session, name: &str, bytes: &[u8]) -> Result<(), Refusal> {
    if !EDITS.contains(&extension(name).as_str()) || crate::formats::material::is_material(bytes) {
        return Err(Refusal::NotEditable(name.to_string()));
    }
    let read = crate::formats::sbs::read_sbs(bytes, name, &|_| None).map_err(|e| Refusal::Unreadable { name: name.to_string(), error: e.to_string() })?;
    if !read.warnings.is_empty() {
        return Err(Refusal::NotRoundTrip { name: name.to_string(), warnings: read.warnings });
    }
    crate::file_cmds::replace_package(session, read.pkg, Some(name.to_string()));
    Ok(())
}

/// The bytes of the session's package as the host stores them (`.sbs`); the session is clean after.
pub fn save_host_file(session: &mut Session) -> Result<Vec<u8>, String> {
    let bytes = crate::file_cmds::save_bytes(session).map_err(|e| e.to_string())?;
    session.modified = false;
    Ok(bytes)
}
