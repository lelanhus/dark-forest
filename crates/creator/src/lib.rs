use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use flate2::{Compression, GzBuilder};
use registry::{parse_manifest, unpack_tarball_to_dir};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::{Builder, EntryType as TarEntryType, Header};

pub const CREATOR_METADATA_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRequest {
    pub game_dir: PathBuf,
    pub out: Option<PathBuf>,
    pub metadata_out: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackOutcome {
    pub artifact_path: PathBuf,
    pub metadata_path: PathBuf,
    pub metadata: PackageMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyRequest {
    pub artifact_path: PathBuf,
    pub metadata_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyOutcome {
    pub artifact_path: PathBuf,
    pub metadata_path: PathBuf,
    pub game_id: String,
    pub version: String,
    pub artifact_sha256: String,
    pub artifact_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageMetadata {
    pub schema_version: u32,
    pub game_id: String,
    pub version: String,
    pub entry_type: String,
    pub host_api: String,
    pub artifact_file: String,
    pub artifact_sha256: String,
    pub artifact_size_bytes: u64,
    pub generated_at: DateTime<Utc>,
}

pub fn pack_game(request: &PackRequest) -> Result<PackOutcome> {
    if !request.game_dir.exists() || !request.game_dir.is_dir() {
        return Err(anyhow!(
            "game directory is invalid: {}",
            request.game_dir.display()
        ));
    }

    let manifest = load_manifest(&request.game_dir)?;
    let artifact_path = request
        .out
        .clone()
        .unwrap_or_else(|| default_artifact_path(&manifest.id, &manifest.version));
    let metadata_path = request
        .metadata_out
        .clone()
        .map(Ok)
        .unwrap_or_else(|| default_metadata_path(&artifact_path))?;

    let tarball_bytes = build_deterministic_tarball(&request.game_dir)?;
    let artifact_sha256 = to_hex_lower(&Sha256::digest(&tarball_bytes));
    let artifact_size_bytes = u64::try_from(tarball_bytes.len())
        .map_err(|_| anyhow!("artifact is too large to represent size as u64"))?;

    atomic_write_bytes(&artifact_path, &tarball_bytes)?;

    let artifact_file = artifact_path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| {
            anyhow!(
                "artifact path has no file name: {}",
                artifact_path.display()
            )
        })?;
    let metadata = PackageMetadata {
        schema_version: CREATOR_METADATA_SCHEMA_VERSION,
        game_id: manifest.id,
        version: manifest.version,
        entry_type: entry_type_label(manifest.entry_type)?,
        host_api: manifest.host_api,
        artifact_file,
        artifact_sha256,
        artifact_size_bytes,
        generated_at: Utc::now(),
    };

    atomic_write_json(&metadata_path, &metadata)?;

    Ok(PackOutcome {
        artifact_path,
        metadata_path,
        metadata,
    })
}

pub fn verify_artifact(request: &VerifyRequest) -> Result<VerifyOutcome> {
    let metadata_path = request
        .metadata_path
        .clone()
        .map(Ok)
        .unwrap_or_else(|| default_metadata_path(&request.artifact_path))?;

    let metadata_raw = fs::read(&metadata_path)
        .with_context(|| format!("failed to read metadata {}", metadata_path.display()))?;
    let metadata: PackageMetadata = serde_json::from_slice(&metadata_raw)
        .with_context(|| format!("failed to parse metadata {}", metadata_path.display()))?;

    if metadata.schema_version != CREATOR_METADATA_SCHEMA_VERSION {
        return Err(anyhow!(
            "unsupported metadata schema version {} (expected {})",
            metadata.schema_version,
            CREATOR_METADATA_SCHEMA_VERSION
        ));
    }

    let artifact_file = request
        .artifact_path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| {
            anyhow!(
                "artifact path has no file name: {}",
                request.artifact_path.display()
            )
        })?;
    if artifact_file != metadata.artifact_file {
        return Err(anyhow!(
            "artifact file mismatch (metadata={}, actual={artifact_file})",
            metadata.artifact_file
        ));
    }

    let artifact_bytes = fs::read(&request.artifact_path).with_context(|| {
        format!(
            "failed to read artifact for verification {}",
            request.artifact_path.display()
        )
    })?;
    let actual_sha256 = to_hex_lower(&Sha256::digest(&artifact_bytes));
    let actual_size = u64::try_from(artifact_bytes.len())
        .map_err(|_| anyhow!("artifact is too large to represent size as u64"))?;

    if actual_sha256 != metadata.artifact_sha256 {
        return Err(anyhow!(
            "artifact checksum mismatch (expected {}, actual {})",
            metadata.artifact_sha256,
            actual_sha256
        ));
    }
    if actual_size != metadata.artifact_size_bytes {
        return Err(anyhow!(
            "artifact size mismatch (expected {}, actual {})",
            metadata.artifact_size_bytes,
            actual_size
        ));
    }

    let temp = tempfile::tempdir().context("failed to create temporary verification directory")?;
    unpack_tarball_to_dir(&request.artifact_path, temp.path())
        .with_context(|| format!("failed to unpack {}", request.artifact_path.display()))?;

    let manifest = load_manifest(temp.path())?;
    let manifest_entry_type = entry_type_label(manifest.entry_type)?;
    if manifest.id != metadata.game_id {
        return Err(anyhow!(
            "manifest id mismatch (metadata={}, manifest={})",
            metadata.game_id,
            manifest.id
        ));
    }
    if manifest.version != metadata.version {
        return Err(anyhow!(
            "manifest version mismatch (metadata={}, manifest={})",
            metadata.version,
            manifest.version
        ));
    }
    if manifest_entry_type != metadata.entry_type {
        return Err(anyhow!(
            "manifest entry_type mismatch (metadata={}, manifest={})",
            metadata.entry_type,
            manifest_entry_type
        ));
    }
    if manifest.host_api != metadata.host_api {
        return Err(anyhow!(
            "manifest host_api mismatch (metadata={}, manifest={})",
            metadata.host_api,
            manifest.host_api
        ));
    }

    Ok(VerifyOutcome {
        artifact_path: request.artifact_path.clone(),
        metadata_path,
        game_id: metadata.game_id,
        version: metadata.version,
        artifact_sha256: actual_sha256,
        artifact_size_bytes: actual_size,
    })
}

pub fn default_artifact_path(game_id: &str, version: &str) -> PathBuf {
    PathBuf::from("dist").join(format!("{game_id}-{version}.tar.gz"))
}

pub fn default_metadata_path(artifact_path: &Path) -> Result<PathBuf> {
    let file_name = artifact_path
        .file_name()
        .ok_or_else(|| {
            anyhow!(
                "artifact path has no file name: {}",
                artifact_path.display()
            )
        })?
        .to_string_lossy()
        .to_string();
    let stem = if let Some(stripped) = file_name.strip_suffix(".tar.gz") {
        stripped.to_string()
    } else if let Some((name, _)) = file_name.rsplit_once('.') {
        name.to_string()
    } else {
        file_name
    };
    let metadata_name = format!("{stem}.metadata.json");
    Ok(artifact_path.with_file_name(metadata_name))
}

fn load_manifest(game_dir: &Path) -> Result<registry::Manifest> {
    let manifest_path = game_dir.join("game.json");
    let manifest_raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read manifest {}", manifest_path.display()))?;
    parse_manifest(&manifest_raw)
        .with_context(|| format!("failed to parse manifest {}", manifest_path.display()))
}

fn build_deterministic_tarball(game_dir: &Path) -> Result<Vec<u8>> {
    let mut files = Vec::new();
    collect_regular_files(game_dir, game_dir, &mut files)?;
    files.sort_by_key(|path| normalize_relative_path(path));

    let mut encoder = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::default());
    {
        let mut tar = Builder::new(&mut encoder);
        tar.mode(tar::HeaderMode::Deterministic);

        for relative in files {
            let absolute = game_dir.join(&relative);
            let bytes = fs::read(&absolute)
                .with_context(|| format!("failed to read {}", absolute.display()))?;
            let archive_path = normalize_relative_path(&relative);
            let mut header = Header::new_gnu();
            header.set_entry_type(TarEntryType::Regular);
            header.set_size(
                u64::try_from(bytes.len())
                    .map_err(|_| anyhow!("file too large to archive: {}", absolute.display()))?,
            );
            header.set_mode(0o644);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            let _ = header.set_username("");
            let _ = header.set_groupname("");
            header.set_cksum();
            tar.append_data(&mut header, archive_path, bytes.as_slice())
                .with_context(|| {
                    format!("failed to append archive entry {}", absolute.display())
                })?;
        }

        tar.finish().context("failed to finalize tar archive")?;
    }

    encoder
        .finish()
        .context("failed to finalize deterministic gzip stream")
}

fn collect_regular_files(root: &Path, current: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_regular_files(root, &path, output)?;
            continue;
        }
        if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .map(Path::to_path_buf)
                .map_err(|err| anyhow!("failed to normalize path {}: {err}", path.display()))?;
            output.push(relative);
        }
    }
    Ok(())
}

fn normalize_relative_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn entry_type_label(value: impl Serialize) -> Result<String> {
    let serialized = serde_json::to_value(value).context("failed to serialize entry_type")?;
    serialized
        .as_str()
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("entry_type did not serialize as string"))
}

fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp = path.with_extension("tmp");
    let mut file = File::create(&tmp)
        .with_context(|| format!("failed to create temporary file {}", tmp.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("failed to write temporary file {}", tmp.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync temporary file {}", tmp.display()))?;
    drop(file);

    fs::rename(&tmp, path).with_context(|| {
        format!(
            "failed to atomically move {} to {}",
            tmp.display(),
            path.display()
        )
    })?;

    if let Some(parent) = path.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }

    Ok(())
}

fn atomic_write_json<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value)?;
    atomic_write_bytes(path, &bytes)
}

fn to_hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{LazyLock, Mutex};

    use anyhow::{Context, Result};
    use serde_json::Value;

    use super::{
        PackRequest, VerifyRequest, default_artifact_path, default_metadata_path, pack_game,
        verify_artifact,
    };

    static TEST_CWD_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    struct CurrentDirGuard {
        previous: PathBuf,
    }

    impl CurrentDirGuard {
        fn enter(target: &Path) -> Result<Self> {
            let previous = std::env::current_dir().context("failed to read current dir")?;
            std::env::set_current_dir(target)
                .with_context(|| format!("failed to set cwd to {}", target.display()))?;
            Ok(Self { previous })
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }

    fn create_sample_game(root: &Path, game_id: &str, version: &str) -> Result<PathBuf> {
        let game_dir = root.join("sample-game");
        fs::create_dir_all(&game_dir)?;

        let manifest = serde_json::json!({
            "id": game_id,
            "name": "Sample Game",
            "version": version,
            "author": "Dark Forest",
            "entry_type": "wasm",
            "entry": "main.wasm",
            "host_api": "^0.1",
            "permissions": ["terminal.raw_input"]
        });
        fs::write(
            game_dir.join("game.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        fs::write(game_dir.join("main.wasm"), b"wasm bytes")?;
        fs::write(game_dir.join("assets.txt"), b"asset payload")?;
        Ok(game_dir)
    }

    #[test]
    fn pack_creates_tarball_and_metadata_with_expected_fields() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_path = temp.path().join("out").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("out")
            .join("sample-game-1.2.3.metadata.json");

        let outcome = pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;

        assert!(artifact_path.exists());
        assert!(metadata_path.exists());
        assert_eq!(outcome.artifact_path, artifact_path);
        assert_eq!(outcome.metadata_path, metadata_path);

        let metadata_json: Value = serde_json::from_slice(&fs::read(&outcome.metadata_path)?)?;
        for field in [
            "schema_version",
            "game_id",
            "version",
            "entry_type",
            "host_api",
            "artifact_file",
            "artifact_sha256",
            "artifact_size_bytes",
            "generated_at",
        ] {
            assert!(
                metadata_json.get(field).is_some(),
                "expected metadata field {field}"
            );
        }
        Ok(())
    }

    #[test]
    fn pack_is_byte_deterministic_for_same_input() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_a = temp.path().join("out").join("first.tar.gz");
        let metadata_a = temp.path().join("out").join("first.metadata.json");
        let artifact_b = temp.path().join("out").join("second.tar.gz");
        let metadata_b = temp.path().join("out").join("second.metadata.json");

        pack_game(&PackRequest {
            game_dir: game_dir.clone(),
            out: Some(artifact_a.clone()),
            metadata_out: Some(metadata_a),
        })?;
        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_b.clone()),
            metadata_out: Some(metadata_b),
        })?;

        assert_eq!(fs::read(&artifact_a)?, fs::read(&artifact_b)?);
        Ok(())
    }

    #[test]
    fn verify_artifact_succeeds_for_matching_artifact_and_metadata() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_path = temp.path().join("out").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("out")
            .join("sample-game-1.2.3.metadata.json");

        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;

        let outcome = verify_artifact(&VerifyRequest {
            artifact_path,
            metadata_path: Some(metadata_path),
        })?;

        assert_eq!(outcome.game_id, "sample-game");
        assert_eq!(outcome.version, "1.2.3");
        Ok(())
    }

    #[test]
    fn verify_artifact_fails_on_checksum_mismatch() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_path = temp.path().join("out").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("out")
            .join("sample-game-1.2.3.metadata.json");

        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;

        fs::write(&artifact_path, b"tampered artifact payload")?;

        let result = verify_artifact(&VerifyRequest {
            artifact_path,
            metadata_path: Some(metadata_path),
        });
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn verify_artifact_fails_on_manifest_id_or_version_mismatch() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let artifact_path = temp.path().join("out").join("sample-game-1.2.3.tar.gz");
        let metadata_path = temp
            .path()
            .join("out")
            .join("sample-game-1.2.3.metadata.json");

        pack_game(&PackRequest {
            game_dir,
            out: Some(artifact_path.clone()),
            metadata_out: Some(metadata_path.clone()),
        })?;

        let mut metadata_json: Value = serde_json::from_slice(&fs::read(&metadata_path)?)?;
        metadata_json["version"] = Value::String("9.9.9".to_string());
        fs::write(&metadata_path, serde_json::to_vec_pretty(&metadata_json)?)?;

        let result = verify_artifact(&VerifyRequest {
            artifact_path,
            metadata_path: Some(metadata_path),
        });
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn pack_defaults_to_dist_when_out_not_provided() -> Result<()> {
        let _cwd_guard = TEST_CWD_LOCK.lock().expect("cwd lock must succeed");
        let temp = tempfile::tempdir()?;
        let game_dir = create_sample_game(temp.path(), "sample-game", "1.2.3")?;
        let _dir_guard = CurrentDirGuard::enter(temp.path())?;

        let expected_artifact = default_artifact_path("sample-game", "1.2.3");
        let expected_metadata = default_metadata_path(&expected_artifact)?;

        let outcome = pack_game(&PackRequest {
            game_dir,
            out: None,
            metadata_out: None,
        })?;

        assert_eq!(outcome.artifact_path, expected_artifact);
        assert_eq!(outcome.metadata_path, expected_metadata);
        assert!(outcome.artifact_path.exists());
        assert!(outcome.metadata_path.exists());
        Ok(())
    }
}
