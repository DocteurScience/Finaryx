//! Where the database lives and how it is opened.

use std::path::PathBuf;

use finaryx_core::rusqlite::Connection;
use finaryx_core::{db, demo};
use gtk::glib;

use crate::config;

/// What the application works on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The user's database file.
    File(PathBuf),
    /// Demonstration data in memory: nothing is written to disk.
    Demo,
}

impl Source {
    pub fn is_demo(&self) -> bool {
        matches!(self, Source::Demo)
    }
}

/// `$XDG_DATA_HOME/finaryx/finaryx.db` (inside the Flatpak sandbox this is
/// the application's own data directory). Development builds use
/// `finaryx-devel/` so that they never touch real data.
pub fn default_path() -> PathBuf {
    let dir = if config::PROFILE == "Devel" {
        "finaryx-devel"
    } else {
        "finaryx"
    };
    glib::user_data_dir().join(dir).join("finaryx.db")
}

pub fn open(source: &Source) -> Result<Connection, String> {
    match source {
        Source::File(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            db::open(path).map_err(|e| format!("{}: {e}", path.display()))
        }
        Source::Demo => {
            let mut conn = db::open_in_memory().map_err(|e| e.to_string())?;
            demo::load(&mut conn).map_err(|e| e.to_string())?;
            Ok(conn)
        }
    }
}
