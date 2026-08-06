//! Shared domain types used by the daemon, query engine, IPC and clients.

use std::path::PathBuf;

/// A searchable object known to `RuyiSeek`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchItem {
    pub id: u64,
    pub name: String,
    pub path: PathBuf,
    pub kind: ItemKind,
    pub hidden: bool,
}

/// High-level result type. More variants can be added without changing rank code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemKind {
    File,
    Directory,
    Application,
    Command,
}

impl ItemKind {
    #[must_use]
    pub const fn protocol_name(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
            Self::Application => "application",
            Self::Command => "command",
        }
    }

    #[must_use]
    pub fn from_protocol_name(value: &str) -> Option<Self> {
        match value {
            "file" => Some(Self::File),
            "directory" => Some(Self::Directory),
            "application" => Some(Self::Application),
            "command" => Some(Self::Command),
            _ => None,
        }
    }
}

/// A scored search result returned to callers.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub item: SearchItem,
    pub score: f32,
}
