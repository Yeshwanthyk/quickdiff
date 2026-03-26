//! User configuration loading and persistence.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Resolved view preferences used by the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewPreferences {
    /// Active theme name.
    pub theme: String,
    /// Whether long lines should wrap inside the diff panes.
    pub wrap_lines: bool,
    /// Whether line numbers should be shown in the gutter.
    pub line_numbers: bool,
}

impl Default for ViewPreferences {
    fn default() -> Self {
        Self {
            theme: "default".to_string(),
            wrap_lines: false,
            line_numbers: true,
        }
    }
}

/// Config file shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuickdiffConfig {
    /// Theme name.
    #[serde(default)]
    pub theme: Option<String>,
    /// Preferred layout mode (reserved for future layout work).
    #[serde(default)]
    pub layout: Option<String>,
    /// Whether to wrap long lines.
    #[serde(default)]
    pub wrap_lines: Option<bool>,
    /// Whether to show line numbers.
    #[serde(default, alias = "show_line_numbers")]
    pub line_numbers: Option<bool>,
}

impl QuickdiffConfig {
    fn merge_into(&self, prefs: &mut ViewPreferences) {
        if let Some(theme) = &self.theme {
            prefs.theme = theme.clone();
        }
        if let Some(wrap_lines) = self.wrap_lines {
            prefs.wrap_lines = wrap_lines;
        }
        if let Some(line_numbers) = self.line_numbers {
            prefs.line_numbers = line_numbers;
        }
    }
}

/// CLI overrides that outrank config files.
#[derive(Debug, Clone, Default)]
pub struct ConfigOverrides {
    /// Theme override from CLI.
    pub theme: Option<String>,
}

/// Result of resolving config layers.
#[derive(Debug, Clone)]
pub struct LoadedPreferences {
    /// Final merged view preferences.
    pub prefs: ViewPreferences,
    /// Non-fatal warnings encountered while loading config.
    pub warnings: Vec<String>,
}

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Resolve preferences from defaults + global config + repo config + CLI overrides.
pub fn load_preferences(repo_root: &Path, overrides: &ConfigOverrides) -> LoadedPreferences {
    let mut prefs = ViewPreferences::default();
    let mut warnings = Vec::new();

    if let Some(global) = load_config_file(&global_config_path(), "global", &mut warnings) {
        global.merge_into(&mut prefs);
    }
    if let Some(repo) = load_config_file(&repo_config_path(repo_root), "repo", &mut warnings) {
        repo.merge_into(&mut prefs);
    }

    if let Some(theme) = &overrides.theme {
        prefs.theme = theme.clone();
    }

    LoadedPreferences { prefs, warnings }
}

/// Persist the current preferences to the global config file.
pub fn save_global_preferences(prefs: &ViewPreferences) -> std::io::Result<()> {
    let path = global_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let config = QuickdiffConfig {
        theme: Some(prefs.theme.clone()),
        layout: None,
        wrap_lines: Some(prefs.wrap_lines),
        line_numbers: Some(prefs.line_numbers),
    };

    let content = toml::to_string_pretty(&config)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    let temp_path = path.with_extension("toml.tmp");
    std::fs::write(&temp_path, content)?;
    std::fs::rename(&temp_path, &path)?;
    Ok(())
}

/// Global config path.
pub fn global_config_path() -> PathBuf {
    config_dir().join("config.toml")
}

/// Repo-local config path.
pub fn repo_config_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".quickdiff").join("config.toml")
}

fn config_dir() -> &'static Path {
    CONFIG_DIR.get_or_init(|| {
        directories::ProjectDirs::from("", "", "quickdiff")
            .map(|dirs| dirs.config_dir().to_path_buf())
            .unwrap_or_else(|| {
                std::env::var("HOME")
                    .map(|home| Path::new(&home).join(".config").join("quickdiff"))
                    .unwrap_or_else(|_| PathBuf::from(".quickdiff"))
            })
    })
}

fn load_config_file(
    path: &Path,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<QuickdiffConfig> {
    if !path.exists() {
        return None;
    }

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) => {
            warnings.push(format!(
                "Failed to read {} config {}: {}",
                label,
                path.display(),
                err
            ));
            return None;
        }
    };

    match toml::from_str(&content) {
        Ok(config) => Some(config),
        Err(err) => {
            warnings.push(format!(
                "Failed to parse {} config {}: {}",
                label,
                path.display(),
                err
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_precedence_is_defaults_then_global_then_repo_then_cli() {
        let mut prefs = ViewPreferences::default();
        QuickdiffConfig {
            theme: Some("dracula".to_string()),
            layout: None,
            wrap_lines: Some(true),
            line_numbers: None,
        }
        .merge_into(&mut prefs);
        QuickdiffConfig {
            theme: Some("nord".to_string()),
            layout: None,
            wrap_lines: None,
            line_numbers: Some(false),
        }
        .merge_into(&mut prefs);
        if let Some(theme) = Some("gruvbox".to_string()) {
            prefs.theme = theme;
        }

        assert_eq!(prefs.theme, "gruvbox");
        assert!(prefs.wrap_lines);
        assert!(!prefs.line_numbers);
    }

    #[test]
    fn invalid_config_is_ignored_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.toml");
        std::fs::write(&path, "wrap_lines = [not valid").unwrap();

        let mut warnings = Vec::new();
        let config = load_config_file(&path, "test", &mut warnings);
        assert!(config.is_none());
        assert_eq!(warnings.len(), 1);
    }
}
