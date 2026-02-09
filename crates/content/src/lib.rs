use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub schema_version: u32,
    pub theme: String,
    pub performance_mode: String,
    pub keymap_profile: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            theme: "forge".to_string(),
            performance_mode: "auto".to_string(),
            keymap_profile: "default".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayHistoryRecord {
    pub game_id: String,
    pub last_played_at: Option<DateTime<Utc>>,
    pub play_count: u64,
    pub total_play_time_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayHistoryFile {
    pub schema_version: u32,
    pub games: BTreeMap<String, PlayHistoryRecord>,
}

pub type PlayHistoryMap = BTreeMap<String, PlayHistoryRecord>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreEntry {
    pub score: i64,
    pub recorded_at: DateTime<Utc>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighScores {
    pub schema_version: u32,
    pub game_id: String,
    pub higher_is_better: bool,
    pub entries: Vec<ScoreEntry>,
}

impl HighScores {
    #[must_use]
    pub fn empty(game_id: &str) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            game_id: game_id.to_string(),
            higher_is_better: true,
            entries: Vec::new(),
        }
    }

    pub fn push_score(&mut self, score: i64) {
        self.entries.push(ScoreEntry {
            score,
            recorded_at: Utc::now(),
            metadata: BTreeMap::new(),
        });

        if self.higher_is_better {
            self.entries.sort_by(|a, b| b.score.cmp(&a.score));
        } else {
            self.entries.sort_by(|a, b| a.score.cmp(&b.score));
        }

        self.entries.truncate(20);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstalledFile {
    pub schema_version: u32,
    pub installed: Vec<InstalledRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledRecord {
    pub id: String,
    pub source: String,
    pub current_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionsFile {
    pub schema_version: u32,
    pub grants: BTreeMap<String, Vec<PermissionGrant>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionGrant {
    pub capability: String,
    pub scope: String,
    pub decision: String,
    pub remembered: bool,
}

pub trait ContentStore {
    fn load_settings(&self) -> Result<Settings>;
    fn save_settings(&self, settings: &Settings) -> Result<()>;
    fn load_play_history(&self) -> Result<PlayHistoryMap>;
    fn save_play_history(&self, history: &PlayHistoryMap) -> Result<()>;
    fn load_installed(&self) -> Result<InstalledFile>;
    fn save_installed(&self, installed: &InstalledFile) -> Result<()>;
    fn load_high_scores(&self, game_id: &str) -> Result<HighScores>;
    fn save_high_scores(&self, game_id: &str, scores: &HighScores) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct JsonContentStore {
    root: PathBuf,
}

impl JsonContentStore {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn create_with_default_root() -> Result<Self> {
        let base = BaseDirs::new().context("failed to resolve base directories")?;
        let root = base.home_dir().join(".dark-forest");
        let store = Self::new(root);
        store.ensure_layout()?;
        Ok(store)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(self.high_scores_dir())?;
        fs::create_dir_all(self.quarantine_dir())?;
        self.ensure_seed_files()?;
        Ok(())
    }

    fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    fn play_history_path(&self) -> PathBuf {
        self.root.join("play_history.json")
    }

    fn high_scores_dir(&self) -> PathBuf {
        self.root.join("high_scores")
    }

    fn high_score_path(&self, game_id: &str) -> PathBuf {
        self.high_scores_dir().join(format!("{game_id}.json"))
    }

    fn quarantine_dir(&self) -> PathBuf {
        self.root.join("quarantine")
    }

    fn installed_path(&self) -> PathBuf {
        self.root.join("installed.json")
    }

    fn permissions_path(&self) -> PathBuf {
        self.root.join("permissions.json")
    }

    fn ensure_seed_files(&self) -> Result<()> {
        if !self.settings_path().exists() {
            self.atomic_write_json(&self.settings_path(), &Settings::default())?;
        }

        if !self.play_history_path().exists() {
            self.atomic_write_json(
                &self.play_history_path(),
                &PlayHistoryFile {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    games: BTreeMap::new(),
                },
            )?;
        }

        if !self.installed_path().exists() {
            self.atomic_write_json(
                &self.installed_path(),
                &InstalledFile {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    installed: Vec::new(),
                },
            )?;
        }

        if !self.permissions_path().exists() {
            self.atomic_write_json(
                &self.permissions_path(),
                &PermissionsFile {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    grants: BTreeMap::new(),
                },
            )?;
        }

        Ok(())
    }

    fn read_json_or_quarantine<T>(&self, path: &Path) -> Result<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        if !path.exists() {
            return Ok(None);
        }

        let data = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        match serde_json::from_str::<T>(&data) {
            Ok(parsed) => Ok(Some(parsed)),
            Err(err) => {
                self.quarantine(path)?;
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "invalid JSON moved to quarantine"
                );
                Ok(None)
            }
        }
    }

    fn quarantine(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }

        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let target_dir = self.quarantine_dir().join(timestamp);
        fs::create_dir_all(&target_dir)?;
        let file_name = path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("unknown.json"));
        let target = target_dir.join(file_name);
        fs::rename(path, target)?;
        Ok(())
    }

    fn atomic_write_json<T>(&self, path: &Path, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp_path = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(value)?;

        let mut file = File::create(&tmp_path)?;
        file.write_all(&json)?;
        file.sync_all()?;
        drop(file);

        fs::rename(&tmp_path, path)?;

        if let Some(parent) = path.parent()
            && let Ok(dir) = File::open(parent)
        {
            let _ = dir.sync_all();
        }

        Ok(())
    }
}

impl ContentStore for JsonContentStore {
    fn load_settings(&self) -> Result<Settings> {
        self.ensure_layout()?;
        let path = self.settings_path();
        let loaded = self.read_json_or_quarantine::<Settings>(&path)?;
        Ok(loaded.unwrap_or_default())
    }

    fn save_settings(&self, settings: &Settings) -> Result<()> {
        self.atomic_write_json(&self.settings_path(), settings)
    }

    fn load_play_history(&self) -> Result<PlayHistoryMap> {
        self.ensure_layout()?;
        let path = self.play_history_path();
        let loaded = self.read_json_or_quarantine::<PlayHistoryFile>(&path)?;
        Ok(loaded.unwrap_or_default().games)
    }

    fn save_play_history(&self, history: &PlayHistoryMap) -> Result<()> {
        let file = PlayHistoryFile {
            schema_version: CURRENT_SCHEMA_VERSION,
            games: history.clone(),
        };
        self.atomic_write_json(&self.play_history_path(), &file)
    }

    fn load_installed(&self) -> Result<InstalledFile> {
        self.ensure_layout()?;
        let path = self.installed_path();
        let loaded = self.read_json_or_quarantine::<InstalledFile>(&path)?;
        Ok(loaded.unwrap_or_default())
    }

    fn save_installed(&self, installed: &InstalledFile) -> Result<()> {
        self.atomic_write_json(&self.installed_path(), installed)
    }

    fn load_high_scores(&self, game_id: &str) -> Result<HighScores> {
        self.ensure_layout()?;
        let path = self.high_score_path(game_id);
        let loaded = self.read_json_or_quarantine::<HighScores>(&path)?;
        Ok(loaded.unwrap_or_else(|| HighScores::empty(game_id)))
    }

    fn save_high_scores(&self, game_id: &str, scores: &HighScores) -> Result<()> {
        self.atomic_write_json(&self.high_score_path(game_id), scores)
    }
}

#[cfg(test)]
mod tests {
    use super::{ContentStore, JsonContentStore, PlayHistoryRecord};
    use anyhow::{Result, anyhow};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn saves_and_loads_settings() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = JsonContentStore::new(PathBuf::from(temp.path()));
        let mut settings = store.load_settings()?;
        settings.performance_mode = "60".to_string();
        store.save_settings(&settings)?;

        let reloaded = store.load_settings()?;
        assert_eq!(reloaded.performance_mode, "60");
        Ok(())
    }

    #[test]
    fn saves_and_loads_play_history() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = JsonContentStore::new(PathBuf::from(temp.path()));

        let mut map = BTreeMap::new();
        map.insert(
            "snake".to_string(),
            PlayHistoryRecord {
                game_id: "snake".to_string(),
                play_count: 3,
                ..PlayHistoryRecord::default()
            },
        );

        store.save_play_history(&map)?;
        let loaded = store.load_play_history()?;
        let snake = loaded
            .get("snake")
            .ok_or_else(|| anyhow!("missing snake play history"))?;
        assert_eq!(snake.play_count, 3);
        Ok(())
    }

    #[test]
    fn quarantines_invalid_json_and_returns_default() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = PathBuf::from(temp.path());
        let store = JsonContentStore::new(root.clone());
        store.ensure_layout()?;

        std::fs::write(root.join("settings.json"), "not json")?;
        let settings = store.load_settings()?;
        assert_eq!(settings.theme, "forge");

        let quarantine_dir = root.join("quarantine");
        assert!(quarantine_dir.exists());
        Ok(())
    }

    #[test]
    fn saves_and_loads_installed_records() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = JsonContentStore::new(PathBuf::from(temp.path()));
        let mut installed = store.load_installed()?;
        installed.installed.push(super::InstalledRecord {
            id: "snake-plus".to_string(),
            source: "builtin://dark-forest".to_string(),
            current_version: "0.1.0".to_string(),
        });
        store.save_installed(&installed)?;

        let reloaded = store.load_installed()?;
        assert_eq!(reloaded.installed.len(), 1);
        assert_eq!(reloaded.installed[0].id, "snake-plus");
        Ok(())
    }
}
