use crate::config::{config_home, write_atomic, ConfigError};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "io.github.ethanbird.RuyiSeek.desktop";
const MANAGED_MARKER: &str = "X-RuyiSeek-Managed=true";

pub(crate) fn default_path() -> Result<PathBuf, ConfigError> {
    Ok(config_home()?.join("autostart").join(FILE_NAME))
}

pub(crate) fn set_enabled(path: &Path, executable: &Path, enabled: bool) -> io::Result<()> {
    if enabled {
        write_atomic(path, desktop_entry(executable).as_bytes())
    } else {
        remove_managed_file(path)
    }
}

fn remove_managed_file(path: &Path) -> io::Result<()> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !source.lines().any(|line| line == MANAGED_MARKER) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "拒绝删除不是由如意寻管理的自动启动文件",
        ));
    }
    fs::remove_file(path)
}

fn desktop_entry(executable: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=如意寻\nComment=本地文件搜索与全局启动器\nExec={} --background\nIcon=system-search\nTerminal=false\nCategories=Utility;FileTools;\nOnlyShowIn=Deepin;\n{}\n",
        escape_exec_argument(&executable.to_string_lossy()),
        MANAGED_MARKER
    )
}

fn escape_exec_argument(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' | '`' | '$' | '\\' => {
                output.push('\\');
                output.push(character);
            }
            '%' => output.push_str("%%"),
            _ => output.push(character),
        }
    }
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    fn test_directory() -> PathBuf {
        let number = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ruyiseek-autostart-test-{}-{number}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create test directory");
        path
    }

    #[test]
    fn enable_writes_managed_entry_and_disable_removes_it() {
        let directory = test_directory();
        let path = directory.join(FILE_NAME);
        set_enabled(&path, Path::new("/opt/Ruyi Seek/ruyiseek-ui"), true)
            .expect("enable autostart");
        let source = fs::read_to_string(&path).expect("read desktop entry");
        assert!(source.contains("Exec=\"/opt/Ruyi Seek/ruyiseek-ui\" --background"));
        assert!(source.contains(MANAGED_MARKER));

        set_enabled(&path, Path::new("ignored"), false).expect("disable autostart");
        assert!(!path.exists());
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn disable_preserves_foreign_desktop_entry() {
        let directory = test_directory();
        let path = directory.join(FILE_NAME);
        fs::write(&path, "[Desktop Entry]\nExec=foreign\n").expect("write foreign entry");

        let error = set_enabled(&path, Path::new("ignored"), false)
            .expect_err("foreign entry must be preserved");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(path.exists());
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn exec_argument_escapes_field_codes_and_shell_metacharacters() {
        assert_eq!(
            escape_exec_argument("/tmp/a% b$`\\\""),
            "\"/tmp/a%% b\\$\\`\\\\\\\"\""
        );
    }
}
