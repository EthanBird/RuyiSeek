//! Bounded filesystem scanner used to build the daemon's in-memory search snapshot.
//!
//! The scan does not follow symlinks and never requires elevated privileges. Along with
//! searchable items it reports every directory that was successfully traversed, allowing
//! the daemon to attach file-change watches without walking the tree a second time.

mod mounts;

pub use mounts::discover_default_roots;

use ruyiseek_core::{ItemKind, SearchItem};
use std::collections::VecDeque;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ScanOptions {
    pub roots: Vec<PathBuf>,
    pub include_hidden: bool,
    pub max_entries: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            include_hidden: false,
            max_entries: 250_000,
        }
    }
}

#[derive(Debug, Default)]
pub struct ScanReport {
    pub items: Vec<SearchItem>,
    pub scanned_directories: Vec<PathBuf>,
    pub skipped_paths: usize,
    pub truncated: bool,
}

/// Recursively scan configured roots without crossing symlinks.
#[must_use]
pub fn scan(options: &ScanOptions) -> ScanReport {
    let mut report = ScanReport::default();
    let mut pending: VecDeque<(usize, PathBuf)> =
        options.roots.iter().cloned().enumerate().collect();

    while let Some((origin_root, directory)) = pending.pop_front() {
        let Ok(entries) = fs::read_dir(&directory) else {
            report.skipped_paths += 1;
            continue;
        };
        report.scanned_directories.push(directory);

        for entry in entries {
            if report.items.len() >= options.max_entries {
                report.truncated = true;
                return report;
            }

            let Ok(entry) = entry else {
                report.skipped_paths += 1;
                continue;
            };
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let hidden = name.starts_with('.');
            if hidden && !options.include_hidden {
                continue;
            }

            let Ok(file_type) = entry.file_type() else {
                report.skipped_paths += 1;
                continue;
            };

            let kind = if file_type.is_dir() {
                ItemKind::Directory
            } else if file_type.is_file() {
                ItemKind::File
            } else {
                // Symlinks and special files are deliberately excluded in this bootstrap.
                continue;
            };

            report.items.push(SearchItem {
                id: stable_path_id(&path),
                name,
                path: path.clone(),
                kind,
                hidden,
            });

            if kind == ItemKind::Directory {
                let is_another_root = options
                    .roots
                    .iter()
                    .enumerate()
                    .any(|(index, root)| index != origin_root && root == &path);
                if !is_another_root {
                    pending.push_back((origin_root, path));
                }
            }
        }
    }

    report
}

fn stable_path_id(path: &Path) -> u64 {
    let mut hasher = StableHasher::default();
    path.hash(&mut hasher);
    hasher.finish()
}

/// FNV-1a is sufficient for bootstrap identity and deterministic across processes.
struct StableHasher(u64);

impl Default for StableHasher {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for StableHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock must be after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ruyiseek-index-test-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn scans_regular_files_and_directories() {
        let root = test_root();
        fs::create_dir_all(root.join("folder")).expect("create fixture directory");
        fs::write(root.join("folder/report.txt"), b"fixture").expect("create fixture file");
        fs::write(root.join(".hidden"), b"fixture").expect("create hidden fixture");

        let report = scan(&ScanOptions {
            roots: vec![root.clone()],
            include_hidden: false,
            max_entries: 100,
        });
        let names: Vec<_> = report.items.iter().map(|item| item.name.as_str()).collect();

        assert!(names.contains(&"folder"));
        assert!(names.contains(&"report.txt"));
        assert!(!names.contains(&".hidden"));
        assert_eq!(
            report.scanned_directories,
            vec![root.clone(), root.join("folder")]
        );
        assert_eq!(report.skipped_paths, 0);
        assert!(!report.truncated);

        fs::remove_dir_all(&root).expect("remove fixture directory");
    }

    #[test]
    fn respects_entry_limit() {
        let root = test_root();
        fs::create_dir_all(&root).expect("create fixture directory");
        fs::write(root.join("one"), b"1").expect("create first fixture");
        fs::write(root.join("two"), b"2").expect("create second fixture");

        let report = scan(&ScanOptions {
            roots: vec![root.clone()],
            include_hidden: true,
            max_entries: 1,
        });

        assert_eq!(report.items.len(), 1);
        assert!(report.truncated);
        fs::remove_dir_all(&root).expect("remove fixture directory");
    }

    #[test]
    fn overlapping_roots_do_not_duplicate_nested_volume_contents() {
        let root = test_root();
        let nested = root.join("mounted-volume");
        fs::create_dir_all(&nested).expect("create nested root");
        fs::write(nested.join("external.txt"), b"fixture").expect("create nested fixture");

        let report = scan(&ScanOptions {
            roots: vec![root.clone(), nested],
            include_hidden: true,
            max_entries: 100,
        });
        assert_eq!(
            report
                .items
                .iter()
                .filter(|item| item.name == "external.txt")
                .count(),
            1
        );

        fs::remove_dir_all(&root).expect("remove fixture directory");
    }
}
