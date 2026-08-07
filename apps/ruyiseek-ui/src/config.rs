use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub(crate) const SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AppConfig {
    pub(crate) schema_version: u16,
    pub(crate) launch_at_login: bool,
    pub(crate) double_ctrl_enabled: bool,
    pub(crate) suppress_in_fullscreen: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            launch_at_login: false,
            double_ctrl_enabled: true,
            suppress_in_fullscreen: true,
        }
    }
}

impl AppConfig {
    pub(crate) fn load(path: &Path) -> Result<Self, ConfigError> {
        let source = fs::read_to_string(path)?;
        let config: Self = toml::from_str(&source)?;
        if config.schema_version != SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedVersion(config.schema_version));
        }
        Ok(config)
    }

    pub(crate) fn load_resilient(path: &Path) -> (Self, Option<String>) {
        match Self::load(path) {
            Ok(config) => (config, None),
            Err(ConfigError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                (Self::default(), None)
            }
            Err(error) => {
                let backup = previous_path(path);
                match Self::load(&backup) {
                    Ok(config) => (
                        config,
                        Some(format!("配置文件损坏，已使用上一次配置：{error}")),
                    ),
                    Err(_) => (
                        Self::default(),
                        Some(format!("配置文件无法读取，已恢复默认值：{error}")),
                    ),
                }
            }
        }
    }

    pub(crate) fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedVersion(self.schema_version));
        }
        let source = toml::to_string_pretty(self)?;
        if path.exists() {
            let backup = previous_path(path);
            fs::copy(path, &backup)?;
            File::open(&backup)?.sync_all()?;
        }
        write_atomic(path, source.as_bytes())?;
        Ok(())
    }
}

pub(crate) fn default_path() -> Result<PathBuf, ConfigError> {
    Ok(config_home()?.join("ruyiseek/config.toml"))
}

pub(crate) fn config_home() -> Result<PathBuf, ConfigError> {
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(|home| PathBuf::from(home).join(".config"))
        .ok_or(ConfigError::MissingConfigHome)
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    fs::create_dir_all(parent)?;

    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy();
    let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }

    let mut guard = TemporaryFile::new(temporary);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(guard.path())?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(guard.path(), path)?;
    guard.keep = true;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn previous_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".previous");
    PathBuf::from(name)
}

struct TemporaryFile {
    path: PathBuf,
    keep: bool,
}

impl TemporaryFile {
    fn new(path: PathBuf) -> Self {
        Self { path, keep: false }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[derive(Debug)]
pub(crate) enum ConfigError {
    Io(io::Error),
    Decode(toml::de::Error),
    Encode(toml::ser::Error),
    UnsupportedVersion(u16),
    MissingConfigHome,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Decode(error) => write!(formatter, "TOML 解析失败：{error}"),
            Self::Encode(error) => write!(formatter, "TOML 序列化失败：{error}"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "不支持配置版本 {version}")
            }
            Self::MissingConfigHome => formatter.write_str("未设置 HOME 或 XDG_CONFIG_HOME"),
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Decode(error) => Some(error),
            Self::Encode(error) => Some(error),
            Self::UnsupportedVersion(_) | Self::MissingConfigHome => None,
        }
    }
}

impl From<io::Error> for ConfigError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(error: toml::de::Error) -> Self {
        Self::Decode(error)
    }
}

impl From<toml::ser::Error> for ConfigError {
    fn from(error: toml::ser::Error) -> Self {
        Self::Encode(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    fn test_directory() -> PathBuf {
        let number = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ruyiseek-config-test-{}-{number}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create test directory");
        path
    }

    #[test]
    fn configuration_round_trip_preserves_preferences() {
        let directory = test_directory();
        let path = directory.join("config.toml");
        let expected = AppConfig {
            launch_at_login: true,
            double_ctrl_enabled: false,
            suppress_in_fullscreen: false,
            ..AppConfig::default()
        };

        expected.save(&path).expect("save configuration");
        assert_eq!(
            AppConfig::load(&path).expect("load configuration"),
            expected
        );
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn invalid_current_file_falls_back_to_previous_version() {
        let directory = test_directory();
        let path = directory.join("config.toml");
        let expected = AppConfig {
            launch_at_login: true,
            ..AppConfig::default()
        };
        expected.save(&path).expect("save initial configuration");
        expected.save(&path).expect("create previous configuration");
        fs::write(&path, "not valid toml = [").expect("corrupt current configuration");

        let (loaded, warning) = AppConfig::load_resilient(&path);
        assert_eq!(loaded, expected);
        assert!(warning.is_some());
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn unsupported_schema_does_not_silently_load() {
        let directory = test_directory();
        let path = directory.join("config.toml");
        fs::write(
            &path,
            "schema_version = 99\nlaunch_at_login = false\ndouble_ctrl_enabled = true\nsuppress_in_fullscreen = true\n",
        )
        .expect("write future configuration");

        assert!(matches!(
            AppConfig::load(&path),
            Err(ConfigError::UnsupportedVersion(99))
        ));
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
