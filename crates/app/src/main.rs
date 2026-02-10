use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use content::{ContentStore, JsonContentStore};
use crossterm::event::{self, Event as CrosstermEvent, KeyCode};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ExecutableCommand, terminal};
use diagnostics::{DiagnosticsSnapshot, PerfDiagnostics, detect_terminal_capabilities};
use games::builtin_catalog;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use registry::{
    BuiltinRegistry, IndexRegistryProvider, RegistryProvider, parse_manifest, unpack_tarball_to_dir,
};
use runtime::{
    PerfMode, RunnerSignal, RuntimeEvent, RuntimeRunner, load_replay_from_path, run_replay,
};
use shell::{GameStatsSummary, InstalledSummary, RenderContext, Route, ShellCommand, ShellState};

#[derive(Debug)]
enum AppEvent {
    Terminal(CrosstermEvent),
    Tick,
    OperationCompleted(OperationReport),
    HotReloadDetected,
    MarketplaceCatalogLoaded(MarketplaceCatalogSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LaunchMode {
    Interactive,
    Replay { path: PathBuf },
    Operation(ContentOperation),
    Registry(RegistryCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RegistryCommand {
    List,
    Add { locator: String },
    Remove { locator: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ContentOperation {
    InstallLocal {
        artifact_dir: PathBuf,
        source: Option<String>,
    },
    InstallIndex {
        locator: String,
        game_id: String,
        version: Option<String>,
    },
    Update {
        game_id: String,
    },
    Rollback {
        game_id: String,
    },
    Verify {
        game_id: String,
    },
    Remove {
        game_id: String,
    },
}

impl ContentOperation {
    fn label(&self) -> String {
        match self {
            Self::InstallLocal { artifact_dir, .. } => {
                format!("install-local:{}", artifact_dir.display())
            }
            Self::InstallIndex {
                locator,
                game_id,
                version,
            } => {
                let suffix = version
                    .as_ref()
                    .map(|value| format!("@{value}"))
                    .unwrap_or_else(|| "@latest".to_string());
                format!("install-index:{locator}:{game_id}{suffix}")
            }
            Self::Update { game_id } => format!("update:{game_id}"),
            Self::Rollback { game_id } => format!("rollback:{game_id}"),
            Self::Verify { game_id } => format!("verify:{game_id}"),
            Self::Remove { game_id } => format!("remove:{game_id}"),
        }
    }
}

#[derive(Debug, Clone)]
struct OperationReport {
    operation: String,
    success: bool,
    message: String,
    installed_changed: bool,
}

impl OperationReport {
    fn success(operation: &ContentOperation, message: String, installed_changed: bool) -> Self {
        Self {
            operation: operation.label(),
            success: true,
            message,
            installed_changed,
        }
    }

    fn failure(operation: &ContentOperation, message: String) -> Self {
        Self {
            operation: operation.label(),
            success: false,
            message,
            installed_changed: false,
        }
    }
}

#[derive(Debug, Clone)]
struct RegistryCommandReport {
    command: String,
    success: bool,
    message: String,
    registry_locators: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct MarketplaceCatalogSnapshot {
    games: Vec<shell::GameItem>,
    install_locators: BTreeMap<String, String>,
    warnings: Vec<String>,
}

struct AppModel {
    shell: ShellState,
    runner: RuntimeRunner,
    store: JsonContentStore,
    settings: content::Settings,
    play_history: content::PlayHistoryMap,
    installed: content::InstalledFile,
    best_scores: BTreeMap<String, i64>,
    current_game_id: Option<String>,
    current_game_seed: Option<u64>,
    current_game_started_at: Option<Instant>,
    seed_counter: u64,
    last_render_at: Instant,
    builtin_games: Vec<shell::GameItem>,
    marketplace_games: Vec<shell::GameItem>,
    marketplace_install_locators: BTreeMap<String, String>,
    marketplace_warnings: Vec<String>,
}

fn should_render_frame(last_render_at: Instant, now: Instant, target: Duration) -> bool {
    now.duration_since(last_render_at) >= target
}

fn should_forward_key_to_runner(
    commands: &[ShellCommand],
    route: &Route,
    overlay: Option<shell::Overlay>,
    runner_running: bool,
) -> bool {
    overlay.is_none()
        && runner_running
        && matches!(route, Route::Runner)
        && commands.iter().all(|cmd| matches!(cmd, ShellCommand::None))
}

fn parse_launch_mode(args: impl IntoIterator<Item = String>) -> Result<LaunchMode> {
    let args = args.into_iter().collect::<Vec<_>>();

    if args.is_empty() {
        return Ok(LaunchMode::Interactive);
    }

    match args[0].as_str() {
        "--help" | "-h" => {
            println!("Usage:");
            println!("  dark-forest                                  # interactive shell");
            println!("  dark-forest --replay <path>                  # run replay fixture");
            println!("  dark-forest --install-local <dir> [--source <source>]");
            println!("  dark-forest --install-index <locator> <id> [--version <ver>]");
            println!("  dark-forest --update <id>");
            println!("  dark-forest --rollback <id>");
            println!("  dark-forest --verify <id>");
            println!("  dark-forest --remove <id>");
            println!("  dark-forest --registry-list");
            println!("  dark-forest --registry-add <locator>");
            println!("  dark-forest --registry-remove <locator>");
            std::process::exit(0);
        }
        "--replay" => {
            if args.len() != 2 {
                return Err(anyhow!("--replay requires exactly one path argument"));
            }
            Ok(LaunchMode::Replay {
                path: PathBuf::from(&args[1]),
            })
        }
        "--install-local" => {
            if args.len() < 2 {
                return Err(anyhow!("--install-local requires an artifact directory"));
            }
            let mut source = None;
            let mut idx = 2;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--source" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--source requires a value"));
                        }
                        source = Some(args[idx + 1].clone());
                        idx += 2;
                    }
                    other => {
                        return Err(anyhow!("unknown argument for --install-local: {other}"));
                    }
                }
            }

            Ok(LaunchMode::Operation(ContentOperation::InstallLocal {
                artifact_dir: PathBuf::from(&args[1]),
                source,
            }))
        }
        "--install-index" => {
            if args.len() < 3 {
                return Err(anyhow!(
                    "--install-index requires <locator> <id> [--version <ver>]"
                ));
            }

            let locator = args[1].clone();
            let game_id = args[2].clone();
            let mut version = None;
            let mut idx = 3;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--version" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--version requires a value"));
                        }
                        version = Some(args[idx + 1].clone());
                        idx += 2;
                    }
                    other => {
                        return Err(anyhow!("unknown argument for --install-index: {other}"));
                    }
                }
            }

            Ok(LaunchMode::Operation(ContentOperation::InstallIndex {
                locator,
                game_id,
                version,
            }))
        }
        "--update" => {
            if args.len() != 2 {
                return Err(anyhow!("--update requires exactly one game id"));
            }
            Ok(LaunchMode::Operation(ContentOperation::Update {
                game_id: args[1].clone(),
            }))
        }
        "--rollback" => {
            if args.len() != 2 {
                return Err(anyhow!("--rollback requires exactly one game id"));
            }
            Ok(LaunchMode::Operation(ContentOperation::Rollback {
                game_id: args[1].clone(),
            }))
        }
        "--verify" => {
            if args.len() != 2 {
                return Err(anyhow!("--verify requires exactly one game id"));
            }
            Ok(LaunchMode::Operation(ContentOperation::Verify {
                game_id: args[1].clone(),
            }))
        }
        "--remove" => {
            if args.len() != 2 {
                return Err(anyhow!("--remove requires exactly one game id"));
            }
            Ok(LaunchMode::Operation(ContentOperation::Remove {
                game_id: args[1].clone(),
            }))
        }
        "--registry-list" => {
            if args.len() != 1 {
                return Err(anyhow!("--registry-list takes no additional arguments"));
            }
            Ok(LaunchMode::Registry(RegistryCommand::List))
        }
        "--registry-add" => {
            if args.len() != 2 {
                return Err(anyhow!("--registry-add requires exactly one locator"));
            }
            Ok(LaunchMode::Registry(RegistryCommand::Add {
                locator: args[1].clone(),
            }))
        }
        "--registry-remove" => {
            if args.len() != 2 {
                return Err(anyhow!("--registry-remove requires exactly one locator"));
            }
            Ok(LaunchMode::Registry(RegistryCommand::Remove {
                locator: args[1].clone(),
            }))
        }
        other => Err(anyhow!("unknown argument: {other}")),
    }
}

fn run_replay_cli(path: &Path) -> Result<()> {
    let scenario = load_replay_from_path(path)?;
    let outcome = run_replay(&scenario, games::instantiate)?;

    println!("replay.path={}", path.display());
    println!("replay.game_id={}", scenario.game_id);
    println!("replay.seed={}", scenario.seed);
    println!("replay.events={}", scenario.events.len());
    println!("replay.score={}", outcome.score);
    println!("replay.finished={}", outcome.finished);
    println!("replay.frame_hash={}", outcome.frame_hash);

    Ok(())
}

fn run_operation_cli(operation: ContentOperation) -> Result<()> {
    let store = JsonContentStore::create_with_default_root()?;
    let report = execute_content_operation(store.root().to_path_buf(), operation.clone())
        .unwrap_or_else(|err| OperationReport::failure(&operation, err.to_string()));

    println!("operation={}", report.operation);
    println!("success={}", report.success);
    println!("message={}", report.message);

    if report.success {
        Ok(())
    } else {
        Err(anyhow!(report.message))
    }
}

fn execute_registry_command(
    root: PathBuf,
    command: RegistryCommand,
) -> Result<RegistryCommandReport> {
    let store = JsonContentStore::new(root);
    store.ensure_layout()?;
    let mut settings = store.load_settings()?;

    let mut locators = settings
        .registries
        .iter()
        .filter(|item| item.scheme.eq_ignore_ascii_case("index"))
        .map(|item| item.locator.clone())
        .collect::<Vec<_>>();

    match command {
        RegistryCommand::List => Ok(RegistryCommandReport {
            command: "registry-list".to_string(),
            success: true,
            message: format!("{} registries configured", locators.len()),
            registry_locators: locators,
        }),
        RegistryCommand::Add { locator } => {
            if locator.trim().is_empty() {
                return Ok(RegistryCommandReport {
                    command: "registry-add".to_string(),
                    success: false,
                    message: "registry locator must not be empty".to_string(),
                    registry_locators: locators,
                });
            }

            if locators.iter().any(|entry| entry == &locator) {
                return Ok(RegistryCommandReport {
                    command: "registry-add".to_string(),
                    success: true,
                    message: format!("registry already configured: {locator}"),
                    registry_locators: locators,
                });
            }

            settings.registries.push(content::RegistryConfig {
                scheme: "index".to_string(),
                locator: locator.clone(),
            });
            store.save_settings(&settings)?;

            locators.push(locator.clone());
            Ok(RegistryCommandReport {
                command: "registry-add".to_string(),
                success: true,
                message: format!("registry added: {locator}"),
                registry_locators: locators,
            })
        }
        RegistryCommand::Remove { locator } => {
            let before_len = settings.registries.len();
            settings.registries.retain(|item| {
                !(item.scheme.eq_ignore_ascii_case("index") && item.locator == locator)
            });

            if settings.registries.len() == before_len {
                return Ok(RegistryCommandReport {
                    command: "registry-remove".to_string(),
                    success: false,
                    message: format!("registry not configured: {locator}"),
                    registry_locators: locators,
                });
            }

            store.save_settings(&settings)?;
            locators = settings
                .registries
                .iter()
                .filter(|item| item.scheme.eq_ignore_ascii_case("index"))
                .map(|item| item.locator.clone())
                .collect::<Vec<_>>();

            Ok(RegistryCommandReport {
                command: "registry-remove".to_string(),
                success: true,
                message: format!("registry removed: {locator}"),
                registry_locators: locators,
            })
        }
    }
}

fn run_registry_cli(command: RegistryCommand) -> Result<()> {
    let store = JsonContentStore::create_with_default_root()?;
    let report = execute_registry_command(store.root().to_path_buf(), command)?;

    println!("command={}", report.command);
    println!("success={}", report.success);
    println!("message={}", report.message);
    println!("registry_count={}", report.registry_locators.len());
    for (idx, locator) in report.registry_locators.iter().enumerate() {
        println!("registry[{idx}]={locator}");
    }

    if report.success {
        Ok(())
    } else {
        Err(anyhow!(report.message))
    }
}

fn latest_played_game_id(history: &content::PlayHistoryMap) -> Option<String> {
    history
        .iter()
        .filter_map(|(id, entry)| entry.last_played_at.as_ref().map(|ts| (id, ts)))
        .max_by_key(|(_id, ts)| *ts)
        .map(|(id, _ts)| id.clone())
}

fn ensure_builtin_installed(
    mut installed: content::InstalledFile,
    games: &[shell::GameItem],
) -> content::InstalledFile {
    for game in games {
        let exists = installed.installed.iter().any(|item| item.id == game.id);
        if !exists {
            installed.installed.push(content::InstalledRecord {
                id: game.id.clone(),
                source: "builtin://dark-forest".to_string(),
                current_version: "0.1.0".to_string(),
                installed_versions: vec!["0.1.0".to_string()],
                version_checksums: BTreeMap::new(),
            });
        }
    }
    installed
}

fn read_installed_game_item(root: &Path, record: &content::InstalledRecord) -> shell::GameItem {
    let manifest_path = root
        .join("games")
        .join(&record.id)
        .join(&record.current_version)
        .join("game.json");

    if let Ok(raw) = fs::read_to_string(&manifest_path)
        && let Ok(manifest) = parse_manifest(&raw)
    {
        return shell::GameItem {
            id: manifest.id,
            name: manifest.name,
            description: format!("Installed from {}", record.source),
            tags: vec!["installed".to_string()],
        };
    }

    shell::GameItem {
        id: record.id.clone(),
        name: record.id.clone(),
        description: format!("Installed from {}", record.source),
        tags: vec!["installed".to_string()],
    }
}

fn load_marketplace_catalog(
    settings: &content::Settings,
    cache_root: PathBuf,
) -> MarketplaceCatalogSnapshot {
    let mut snapshot = MarketplaceCatalogSnapshot::default();

    for registry in &settings.registries {
        if !registry.scheme.eq_ignore_ascii_case("index") {
            snapshot.warnings.push(format!(
                "registry {} has unsupported scheme '{}'",
                registry.locator, registry.scheme
            ));
            continue;
        }

        if registry.locator.trim().is_empty() {
            snapshot
                .warnings
                .push("registry locator must not be empty".to_string());
            continue;
        }

        let provider = IndexRegistryProvider::new(registry.locator.clone(), cache_root.clone());
        match provider.list() {
            Ok(listings) => {
                for listing in listings {
                    if let Some(previous) = snapshot.install_locators.get(&listing.id) {
                        snapshot.warnings.push(format!(
                            "duplicate marketplace id '{}' from {} ignored (already provided by {})",
                            listing.id, registry.locator, previous
                        ));
                        continue;
                    }

                    snapshot
                        .install_locators
                        .insert(listing.id.clone(), registry.locator.clone());
                    snapshot.games.push(shell::GameItem {
                        id: listing.id,
                        name: listing.name,
                        description: listing.description,
                        tags: listing.tags,
                    });
                }
            }
            Err(err) => {
                snapshot.warnings.push(format!(
                    "registry {} load failed: {}",
                    registry.locator, err
                ));
            }
        }
    }

    snapshot
}

fn select_unpacked_artifact_root(unpack_dir: &Path) -> Result<PathBuf> {
    if unpack_dir.join("game.json").exists() {
        return Ok(unpack_dir.to_path_buf());
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(unpack_dir)? {
        let path = entry?.path();
        if path.is_dir() && path.join("game.json").exists() {
            candidates.push(path);
        }
    }

    if candidates.len() == 1 {
        return Ok(candidates.remove(0));
    }

    Err(anyhow!(
        "unable to locate game.json in unpacked artifact {}",
        unpack_dir.display()
    ))
}

fn parse_manifest_from_artifact_dir(dir: &Path) -> Result<registry::Manifest> {
    let manifest_path = dir.join("game.json");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    parse_manifest(&raw)
}

fn install_from_index(
    store: &JsonContentStore,
    locator: &str,
    game_id: &str,
    version: Option<String>,
) -> Result<content::InstallOutcome> {
    let provider = IndexRegistryProvider::new(locator.to_string(), store.cache_dir_path());
    let artifact = provider.resolve(game_id, version.as_deref())?;
    let archive_path = provider.fetch_artifact_to_cache(&artifact)?;

    let unpack_dir = store.root().join("tmp").join(format!(
        "index-install-{}-{}-{}",
        game_id,
        artifact.version,
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));

    if unpack_dir.exists() {
        let _ = fs::remove_dir_all(&unpack_dir);
    }
    fs::create_dir_all(&unpack_dir)?;
    unpack_tarball_to_dir(&archive_path, &unpack_dir)?;
    let artifact_root = select_unpacked_artifact_root(&unpack_dir)?;

    let result = store.install_from_directory(&content::InstallRequest {
        game_id: game_id.to_string(),
        version: artifact.version,
        source: format!("index://{locator}"),
        artifact_dir: artifact_root,
        expected_sha256: artifact.checksum_sha256,
    });

    let _ = fs::remove_dir_all(&unpack_dir);
    result.map_err(|err| anyhow!(err.to_string()))
}

fn execute_content_operation(
    root: PathBuf,
    operation: ContentOperation,
) -> Result<OperationReport> {
    let store = JsonContentStore::new(root);
    store.ensure_layout()?;

    match operation.clone() {
        ContentOperation::InstallLocal {
            artifact_dir,
            source,
        } => {
            let manifest = parse_manifest_from_artifact_dir(&artifact_dir)?;
            let outcome = store
                .install_from_directory(&content::InstallRequest {
                    game_id: manifest.id.clone(),
                    version: manifest.version.clone(),
                    source: source.unwrap_or_else(|| "local://manual".to_string()),
                    artifact_dir,
                    expected_sha256: None,
                })
                .map_err(|err| anyhow!(err.to_string()))?;

            Ok(OperationReport::success(
                &operation,
                format!("installed {}@{}", outcome.game_id, outcome.version),
                true,
            ))
        }
        ContentOperation::InstallIndex {
            locator,
            game_id,
            version,
        } => {
            let outcome = install_from_index(&store, &locator, &game_id, version)?;
            Ok(OperationReport::success(
                &operation,
                format!(
                    "installed {}@{} from index",
                    outcome.game_id, outcome.version
                ),
                true,
            ))
        }
        ContentOperation::Update { game_id } => {
            let installed = store.load_installed()?;
            let record = installed
                .installed
                .iter()
                .find(|item| item.id == game_id)
                .ok_or_else(|| anyhow!("game is not installed: {game_id}"))?;

            let Some(locator) = record.source.strip_prefix("index://") else {
                return Err(anyhow!(
                    "update is currently supported only for index:// sources"
                ));
            };

            let provider = IndexRegistryProvider::new(locator.to_string(), store.cache_dir_path());
            let latest = provider.resolve(&game_id, None)?;
            if latest.version == record.current_version {
                return Ok(OperationReport::success(
                    &operation,
                    format!("{game_id} is already up to date ({})", latest.version),
                    false,
                ));
            }

            let outcome = install_from_index(&store, locator, &game_id, Some(latest.version))?;
            Ok(OperationReport::success(
                &operation,
                format!("updated {} to {}", outcome.game_id, outcome.version),
                true,
            ))
        }
        ContentOperation::Rollback { game_id } => {
            let outcome = store
                .rollback_game(&game_id)
                .map_err(|err| anyhow!(err.to_string()))?;
            Ok(OperationReport::success(
                &operation,
                format!(
                    "rolled back {} from {} to {}",
                    outcome.game_id, outcome.from_version, outcome.to_version
                ),
                true,
            ))
        }
        ContentOperation::Verify { game_id } => {
            let outcome = store
                .verify_game(&game_id)
                .map_err(|err| anyhow!(err.to_string()))?;

            if outcome.verified {
                Ok(OperationReport::success(
                    &operation,
                    format!("verified {}@{}", outcome.game_id, outcome.version),
                    false,
                ))
            } else {
                Ok(OperationReport::failure(
                    &operation,
                    format!(
                        "verification failed for {}@{} (expected {:?}, actual {})",
                        outcome.game_id,
                        outcome.version,
                        outcome.expected_sha256,
                        outcome.actual_sha256,
                    ),
                ))
            }
        }
        ContentOperation::Remove { game_id } => {
            let outcome = store
                .remove_game(&game_id)
                .map_err(|err| anyhow!(err.to_string()))?;
            Ok(OperationReport::success(
                &operation,
                format!("removed {}@{}", outcome.game_id, outcome.removed_version),
                true,
            ))
        }
    }
}

fn compute_hotload_signature(root: &Path) -> Result<u64> {
    let mut files = Vec::new();
    let installed_path = root.join("installed.json");
    if installed_path.exists() {
        files.push(installed_path);
    }

    collect_game_manifests(&root.join("games"), &mut files)?;
    files.sort();

    let mut hasher = DefaultHasher::new();
    for path in files {
        path.hash(&mut hasher);
        let metadata = fs::metadata(&path)
            .with_context(|| format!("failed to read metadata for {}", path.display()))?;
        metadata.len().hash(&mut hasher);
        let modified = metadata.modified().ok();
        modified.hash(&mut hasher);
    }

    Ok(hasher.finish())
}

fn collect_game_manifests(dir: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_game_manifests(&path, output)?;
        } else if path.file_name().is_some_and(|name| name == "game.json") {
            output.push(path);
        }
    }

    Ok(())
}

impl AppModel {
    fn new() -> Result<Self> {
        let listings = builtin_catalog();
        let registry = BuiltinRegistry::new(listings.clone());
        let builtin_games = registry
            .list()?
            .into_iter()
            .map(|listing| shell::GameItem {
                id: listing.id,
                name: listing.name,
                description: listing.description,
                tags: listing.tags,
            })
            .collect::<Vec<_>>();

        let store = JsonContentStore::create_with_default_root()?;
        store.ensure_layout()?;
        let settings = store.load_settings()?;
        let play_history = store.load_play_history()?;
        let installed = ensure_builtin_installed(store.load_installed()?, &builtin_games);
        let _ = store.save_installed(&installed);
        let mut best_scores = BTreeMap::new();

        for game in &builtin_games {
            if let Ok(scores) = store.load_high_scores(&game.id)
                && let Some(best) = scores.entries.first()
            {
                best_scores.insert(game.id.clone(), best.score);
            }
        }

        let (width, height) = terminal::size().unwrap_or((120, 40));
        let mut runner = RuntimeRunner::new(width.saturating_sub(4), height.saturating_sub(8));

        let perf_mode = match settings.performance_mode.as_str() {
            "60" => PerfMode::Fps60,
            "30" => PerfMode::Fps30,
            _ => PerfMode::Auto,
        };
        runner.set_perf_mode(perf_mode);

        let mut shell = ShellState::new(builtin_games.clone());
        shell.continue_game_id = latest_played_game_id(&play_history);
        shell.performance_mode = settings.performance_mode.clone();

        let mut model = Self {
            shell,
            runner,
            store,
            settings,
            play_history,
            installed,
            best_scores,
            current_game_id: None,
            current_game_seed: None,
            current_game_started_at: None,
            seed_counter: Utc::now().timestamp() as u64,
            last_render_at: Instant::now(),
            builtin_games,
            marketplace_games: Vec::new(),
            marketplace_install_locators: BTreeMap::new(),
            marketplace_warnings: Vec::new(),
        };

        model.refresh_game_catalog();
        Ok(model)
    }

    fn next_seed(&mut self) -> u64 {
        self.seed_counter = self.seed_counter.saturating_add(1);
        self.seed_counter
    }

    fn refresh_game_catalog(&mut self) {
        let mut merged = self.builtin_games.clone();

        for game in &self.marketplace_games {
            if merged.iter().any(|item| item.id == game.id) {
                continue;
            }
            merged.push(game.clone());
        }

        for record in &self.installed.installed {
            if merged.iter().any(|game| game.id == record.id) {
                continue;
            }
            merged.push(read_installed_game_item(self.store.root(), record));
        }

        self.shell.games = merged;
        self.shell.set_installed_game_ids(
            self.installed
                .installed
                .iter()
                .map(|item| item.id.clone())
                .collect(),
        );
    }

    fn apply_marketplace_snapshot(&mut self, snapshot: MarketplaceCatalogSnapshot) {
        self.marketplace_games = snapshot.games;
        self.marketplace_install_locators = snapshot.install_locators;
        self.refresh_game_catalog();

        for warning in &snapshot.warnings {
            if !self
                .marketplace_warnings
                .iter()
                .any(|existing| existing == warning)
            {
                self.shell
                    .push_notification(format!("Marketplace warning: {warning}"));
            }
        }
        self.marketplace_warnings = snapshot.warnings;
    }

    fn refresh_installed_state(&mut self) -> bool {
        let previous_active_version = self.current_game_id.as_ref().and_then(|id| {
            self.installed
                .installed
                .iter()
                .find(|item| &item.id == id)
                .map(|item| item.current_version.clone())
        });

        if let Ok(installed) = self.store.load_installed() {
            self.installed = installed;
            self.refresh_game_catalog();
        }

        let next_active_version = self.current_game_id.as_ref().and_then(|id| {
            self.installed
                .installed
                .iter()
                .find(|item| &item.id == id)
                .map(|item| item.current_version.clone())
        });

        previous_active_version.is_some() && previous_active_version != next_active_version
    }

    fn cycle_performance_mode(&mut self) {
        self.settings.performance_mode = match self.settings.performance_mode.as_str() {
            "auto" => "60".to_string(),
            "60" => "30".to_string(),
            _ => "auto".to_string(),
        };

        let mode = match self.settings.performance_mode.as_str() {
            "60" => PerfMode::Fps60,
            "30" => PerfMode::Fps30,
            _ => PerfMode::Auto,
        };

        self.runner.set_perf_mode(mode);
        self.shell.performance_mode = self.settings.performance_mode.clone();
    }

    fn start_game(&mut self, game_id: &str) {
        let seed = self.next_seed();
        match games::instantiate(game_id, seed) {
            Ok(game) => {
                if let Err(err) = self.runner.start(game, seed) {
                    self.shell.set_error(format!("failed to start game: {err}"));
                    return;
                }
                self.shell.route = Route::Runner;
                self.shell.overlay = None;
                self.current_game_id = Some(game_id.to_string());
                self.current_game_seed = Some(seed);
                self.current_game_started_at = Some(Instant::now());
                self.last_render_at = Instant::now()
                    .checked_sub(self.runner.target_frame_duration())
                    .unwrap_or_else(Instant::now);
                self.shell.push_notification(format!("Started {game_id}"));
            }
            Err(err) => self.shell.set_error(format!(
                "unable to start game ({game_id}): {err}. Third-party execution is not enabled yet"
            )),
        }
    }

    fn stop_game(&mut self) {
        self.update_play_stats();
        self.runner.stop();
        self.shell.overlay = None;
        self.current_game_id = None;
        self.current_game_seed = None;
        self.current_game_started_at = None;
    }

    fn update_play_stats(&mut self) {
        let Some(game_id) = self.current_game_id.clone() else {
            return;
        };

        let elapsed = self
            .current_game_started_at
            .map_or(Duration::from_secs(0), |start| start.elapsed());

        let entry = self.play_history.entry(game_id.clone()).or_insert_with(|| {
            content::PlayHistoryRecord {
                game_id: game_id.clone(),
                ..content::PlayHistoryRecord::default()
            }
        });

        entry.play_count = entry.play_count.saturating_add(1);
        entry.total_play_time_seconds = entry
            .total_play_time_seconds
            .saturating_add(elapsed.as_secs());
        entry.last_played_at = Some(Utc::now());

        let mut scores = match self.store.load_high_scores(&game_id) {
            Ok(existing) => existing,
            Err(_) => content::HighScores::empty(&game_id),
        };
        scores.push_score(self.runner.score());
        if let Some(best) = scores.entries.first() {
            self.best_scores.insert(game_id.clone(), best.score);
        } else {
            self.best_scores.remove(&game_id);
        }
        self.shell.continue_game_id = latest_played_game_id(&self.play_history);

        let _ = self.store.save_high_scores(&game_id, &scores);
        let _ = self.store.save_play_history(&self.play_history);
    }

    fn handle_shell_command(&mut self, command: ShellCommand) -> bool {
        match command {
            ShellCommand::Quit => return true,
            ShellCommand::OpenRoute(route) => self.shell.route = route,
            ShellCommand::StartGame(id) => self.start_game(&id),
            ShellCommand::StopGame => self.stop_game(),
            ShellCommand::RestartGame => {
                let seed = self.next_seed();
                let _ = self.runner.restart(seed);
            }
            ShellCommand::ToggleFullscreen => self.runner.toggle_fullscreen(),
            ShellCommand::TogglePause => self.runner.toggle_pause(),
            ShellCommand::SetOverlay(overlay) => self.shell.overlay = overlay,
            ShellCommand::CyclePerformance => self.cycle_performance_mode(),
            ShellCommand::InstallSelected(_)
            | ShellCommand::UpdateInstalled(_)
            | ShellCommand::RollbackInstalled(_)
            | ShellCommand::VerifyInstalled(_)
            | ShellCommand::RemoveInstalled(_)
            | ShellCommand::None => {}
        }

        false
    }

    fn apply_runner_signal(&mut self, signal: RunnerSignal) {
        match signal {
            RunnerSignal::Started => self.shell.push_notification("Runner started"),
            RunnerSignal::Stopped => self.shell.push_notification("Runner stopped"),
            RunnerSignal::Paused => self.shell.push_notification("Paused"),
            RunnerSignal::Resumed => self.shell.push_notification("Resumed"),
            RunnerSignal::PerfModeChanged(mode) => {
                self.shell.push_notification(format!("Perf mode: {mode:?}"))
            }
            RunnerSignal::Crashed(err) => {
                let crashed_id = self.current_game_id.clone();
                self.current_game_id = None;
                self.current_game_seed = None;
                self.current_game_started_at = None;
                self.shell.route = crashed_id
                    .map(|id| Route::GameDetail { id })
                    .unwrap_or(Route::Library);
                self.shell.set_error(format!("Game crashed: {err}"));
            }
        }
    }

    fn apply_operation_report(&mut self, report: OperationReport) {
        if !self.shell.progress.is_empty() {
            let _ = self.shell.progress.remove(0);
        }

        if report.success {
            self.shell.push_notification(report.message);
            if report.installed_changed {
                let active_changed = self.refresh_installed_state();
                if active_changed {
                    self.shell
                        .push_notification("Active game content changed. Restart to apply.");
                }
            }
        } else {
            self.shell
                .set_error(format!("{} failed: {}", report.operation, report.message));
        }
    }

    fn render_context(&self) -> RenderContext {
        let mode_label = match self.runner.perf_mode() {
            PerfMode::Auto => format!(
                "mode:auto fps:{} avg:{:.2}ms",
                self.runner.auto_target_fps(),
                self.runner.average_render_ms()
            ),
            PerfMode::Fps60 => "mode:60".to_string(),
            PerfMode::Fps30 => "mode:30".to_string(),
        };
        let mut game_stats = BTreeMap::new();
        for game in &self.shell.games {
            let history = self.play_history.get(&game.id);
            game_stats.insert(
                game.id.clone(),
                GameStatsSummary {
                    play_count: history.map_or(0, |item| item.play_count),
                    best_score: self.best_scores.get(&game.id).copied(),
                    last_played_at: history.and_then(|item| {
                        item.last_played_at
                            .as_ref()
                            .map(chrono::DateTime::<Utc>::to_rfc3339)
                    }),
                },
            );
        }
        let installed = self
            .installed
            .installed
            .iter()
            .map(|item| {
                (
                    item.id.clone(),
                    InstalledSummary {
                        current_version: item.current_version.clone(),
                        source: item.source.clone(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        RenderContext {
            current_game: self.current_game_id.clone(),
            runner_frame: if self.runner.is_running() {
                Some(self.runner.current_frame().clone())
            } else {
                None
            },
            runner_paused: self.runner.is_paused(),
            runner_fullscreen: self.runner.is_fullscreen(),
            perf_summary: mode_label,
            game_stats,
            installed,
        }
    }

    fn diagnostics_snapshot(&self) -> DiagnosticsSnapshot {
        DiagnosticsSnapshot {
            terminal: detect_terminal_capabilities(),
            perf: PerfDiagnostics {
                target_fps: self.runner.auto_target_fps(),
                render_ms_avg: self.runner.average_render_ms(),
                mode: format!("{:?}", self.runner.perf_mode()),
            },
        }
    }

    fn maybe_render_runner_frame(&mut self, now: Instant) {
        if !self.runner.is_running() {
            return;
        }

        let target = self.runner.target_frame_duration();
        if should_render_frame(self.last_render_at, now, target) {
            let _ = self.runner.render();
            self.last_render_at = now;
        }
    }
}

fn enqueue_operation(
    shell: &mut ShellState,
    tx: &tokio::sync::mpsc::Sender<ContentOperation>,
    operation: ContentOperation,
) {
    let label = operation.label();
    match tx.try_send(operation) {
        Ok(()) => {
            shell.progress.push(format!("Running: {label}"));
            if shell.progress.len() > 5 {
                let _ = shell.progress.remove(0);
            }
        }
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            shell.set_error("operation queue is full; try again");
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
            shell.set_error("operation worker unavailable");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    match parse_launch_mode(std::env::args().skip(1))? {
        LaunchMode::Interactive => run().await,
        LaunchMode::Replay { path } => run_replay_cli(&path),
        LaunchMode::Operation(operation) => run_operation_cli(operation),
        LaunchMode::Registry(command) => run_registry_cli(command),
    }
}

async fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    stdout.execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run_loop(&mut terminal).await;

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_loop(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    let mut model = AppModel::new()?;

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<AppEvent>(256);
    let (operation_tx, mut operation_rx) = tokio::sync::mpsc::channel::<ContentOperation>(32);
    let running = Arc::new(AtomicBool::new(true));

    let input_running = Arc::clone(&running);
    let input_tx = event_tx.clone();
    let input_task = tokio::task::spawn_blocking(move || {
        while input_running.load(Ordering::SeqCst) {
            if event::poll(Duration::from_millis(20)).unwrap_or(false)
                && let Ok(evt) = event::read()
            {
                match input_tx.try_send(AppEvent::Terminal(evt)) {
                    Ok(()) => {}
                    Err(tokio::sync::mpsc::error::TrySendError::Full(AppEvent::Terminal(
                        CrosstermEvent::Key(key),
                    ))) if matches!(
                        key.code,
                        KeyCode::Up
                            | KeyCode::Down
                            | KeyCode::Left
                            | KeyCode::Right
                            | KeyCode::Char('j')
                            | KeyCode::Char('k')
                    ) => {}
                    Err(tokio::sync::mpsc::error::TrySendError::Full(event)) => {
                        let _ = input_tx.blocking_send(event);
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
                }
            }
        }
    });

    let tick_running = Arc::clone(&running);
    let tick_tx = event_tx.clone();
    let tick_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(16));
        while tick_running.load(Ordering::SeqCst) {
            interval.tick().await;
            let _ = tick_tx.try_send(AppEvent::Tick);
        }
    });

    let operation_running = Arc::clone(&running);
    let operation_event_tx = event_tx.clone();
    let operation_root = model.store.root().to_path_buf();
    let operation_task = tokio::spawn(async move {
        while operation_running.load(Ordering::SeqCst) {
            let Some(operation) = operation_rx.recv().await else {
                break;
            };

            let operation_for_error = operation.clone();
            let root = operation_root.clone();
            let result =
                tokio::task::spawn_blocking(move || execute_content_operation(root, operation))
                    .await;

            let report = match result {
                Ok(Ok(report)) => report,
                Ok(Err(err)) => OperationReport::failure(&operation_for_error, err.to_string()),
                Err(join_err) => OperationReport::failure(
                    &operation_for_error,
                    format!("operation task failed: {join_err}"),
                ),
            };

            let _ = operation_event_tx
                .send(AppEvent::OperationCompleted(report))
                .await;
        }
    });

    let watch_running = Arc::clone(&running);
    let watch_tx = event_tx.clone();
    let watch_root = model.store.root().to_path_buf();
    let watch_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        let mut last_signature = compute_hotload_signature(&watch_root).ok();

        while watch_running.load(Ordering::SeqCst) {
            interval.tick().await;
            let next_signature = compute_hotload_signature(&watch_root).ok();
            if next_signature.is_some() && next_signature != last_signature {
                last_signature = next_signature;
                let _ = watch_tx.try_send(AppEvent::HotReloadDetected);
            }
        }
    });

    let marketplace_running = Arc::clone(&running);
    let marketplace_tx = event_tx.clone();
    let marketplace_settings = model.settings.clone();
    let marketplace_cache_root = model.store.cache_dir_path();
    let marketplace_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        while marketplace_running.load(Ordering::SeqCst) {
            interval.tick().await;
            let settings = marketplace_settings.clone();
            let cache_root = marketplace_cache_root.clone();
            let snapshot = match tokio::task::spawn_blocking(move || {
                load_marketplace_catalog(&settings, cache_root)
            })
            .await
            {
                Ok(snapshot) => snapshot,
                Err(join_err) => MarketplaceCatalogSnapshot {
                    games: Vec::new(),
                    install_locators: BTreeMap::new(),
                    warnings: vec![format!("marketplace refresh task failed: {join_err}")],
                },
            };

            let _ = marketplace_tx
                .send(AppEvent::MarketplaceCatalogLoaded(snapshot))
                .await;
        }
    });

    let mut should_quit = false;
    while !should_quit {
        if let Some(event) = event_rx.recv().await {
            match event {
                AppEvent::Tick => {
                    let _ = model.runner.dispatch(RuntimeEvent::Tick { dt_ms: 16 });
                    model.maybe_render_runner_frame(Instant::now());

                    for signal in model.runner.take_signals() {
                        model.apply_runner_signal(signal);
                    }

                    if model.runner.game_finished() {
                        let previous_game_id = model.current_game_id.clone();
                        model.stop_game();
                        model.shell.route = Route::GameDetail {
                            id: previous_game_id.unwrap_or_else(|| "unknown".to_string()),
                        };
                    }
                }
                AppEvent::OperationCompleted(report) => {
                    model.apply_operation_report(report);
                }
                AppEvent::HotReloadDetected => {
                    if model.refresh_installed_state() {
                        model.shell.push_notification(
                            "Installed content changed. Restart active game to apply.",
                        );
                    }
                }
                AppEvent::MarketplaceCatalogLoaded(snapshot) => {
                    model.apply_marketplace_snapshot(snapshot);
                }
                AppEvent::Terminal(event) => match event {
                    CrosstermEvent::Key(key) => {
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(event::KeyModifiers::CONTROL)
                        {
                            should_quit = true;
                            continue;
                        }

                        let commands = model.shell.handle_key(
                            key,
                            model.runner.is_running(),
                            model.runner.is_paused(),
                        );
                        let forwarded_to_runner = should_forward_key_to_runner(
                            &commands,
                            &model.shell.route,
                            model.shell.overlay,
                            model.runner.is_running(),
                        );

                        for command in commands {
                            match command {
                                ShellCommand::InstallSelected(id) => {
                                    if model.installed.installed.iter().any(|item| item.id == id) {
                                        continue;
                                    }

                                    if let Some(locator) =
                                        model.marketplace_install_locators.get(&id).cloned()
                                    {
                                        enqueue_operation(
                                            &mut model.shell,
                                            &operation_tx,
                                            ContentOperation::InstallIndex {
                                                locator,
                                                game_id: id,
                                                version: None,
                                            },
                                        );
                                    } else {
                                        model.shell.set_error(format!(
                                            "install source unavailable for game: {id}"
                                        ));
                                    }
                                }
                                ShellCommand::UpdateInstalled(id) => enqueue_operation(
                                    &mut model.shell,
                                    &operation_tx,
                                    ContentOperation::Update { game_id: id },
                                ),
                                ShellCommand::RollbackInstalled(id) => enqueue_operation(
                                    &mut model.shell,
                                    &operation_tx,
                                    ContentOperation::Rollback { game_id: id },
                                ),
                                ShellCommand::VerifyInstalled(id) => enqueue_operation(
                                    &mut model.shell,
                                    &operation_tx,
                                    ContentOperation::Verify { game_id: id },
                                ),
                                ShellCommand::RemoveInstalled(id) => enqueue_operation(
                                    &mut model.shell,
                                    &operation_tx,
                                    ContentOperation::Remove { game_id: id },
                                ),
                                other => {
                                    if model.handle_shell_command(other) {
                                        should_quit = true;
                                    }
                                }
                            }
                        }

                        if forwarded_to_runner {
                            let _ = model.runner.dispatch(RuntimeEvent::Input(key));
                        }
                    }
                    CrosstermEvent::Resize(w, h) => {
                        let runner_w = w.saturating_sub(4).max(20);
                        let runner_h = h.saturating_sub(8).max(10);
                        model.runner.resize(runner_w, runner_h);
                    }
                    CrosstermEvent::FocusLost => {
                        let _ = model.runner.dispatch(RuntimeEvent::FocusLost);
                        if model.runner.is_running() && !model.runner.is_paused() {
                            model.runner.toggle_pause();
                        }
                    }
                    CrosstermEvent::FocusGained => {
                        let _ = model.runner.dispatch(RuntimeEvent::FocusGained);
                    }
                    _ => {}
                },
            }

            terminal.draw(|frame| {
                shell::render(frame, &model.shell, &model.render_context());
            })?;
        }
    }

    running.store(false, Ordering::SeqCst);
    let _ = input_task.await;
    let _ = tick_task.await;
    let _ = watch_task.await;
    let _ = marketplace_task.await;
    let _ = operation_task.await;

    model.update_play_stats();
    model.store.save_settings(&model.settings)?;
    model.store.save_play_history(&model.play_history)?;
    model.store.save_installed(&model.installed)?;

    let snapshot = model.diagnostics_snapshot();
    tracing::info!(
        width = snapshot.terminal.width,
        height = snapshot.terminal.height,
        truecolor = snapshot.terminal.truecolor,
        target_fps = snapshot.perf.target_fps,
        avg_ms = snapshot.perf.render_ms_avg,
        mode = snapshot.perf.mode,
        "shutdown diagnostics"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use content::ContentStore;
    use shell::{Overlay, Route, ShellCommand};

    use super::{
        ContentOperation, LaunchMode, RegistryCommand, compute_hotload_signature,
        execute_content_operation, execute_registry_command, load_marketplace_catalog,
        parse_launch_mode, should_forward_key_to_runner, should_render_frame,
    };

    #[test]
    fn render_scheduler_waits_until_target_duration() {
        let last = Instant::now();
        let now = last + Duration::from_millis(10);
        assert!(!should_render_frame(last, now, Duration::from_millis(16)));
    }

    #[test]
    fn render_scheduler_allows_render_when_target_elapsed() {
        let last = Instant::now();
        let now = last + Duration::from_millis(16);
        assert!(should_render_frame(last, now, Duration::from_millis(16)));
    }

    #[test]
    fn parses_replay_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--replay".to_string(),
            "fixtures/replays/snake-seed-12345.json".to_string(),
        ])
        .expect("replay mode should parse");
        assert_eq!(
            mode,
            LaunchMode::Replay {
                path: PathBuf::from("fixtures/replays/snake-seed-12345.json")
            }
        );
    }

    #[test]
    fn parses_install_local_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--install-local".to_string(),
            "/tmp/game".to_string(),
            "--source".to_string(),
            "local://fixtures".to_string(),
        ])
        .expect("install-local mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Operation(ContentOperation::InstallLocal {
                artifact_dir: PathBuf::from("/tmp/game"),
                source: Some("local://fixtures".to_string()),
            })
        );
    }

    #[test]
    fn parses_install_index_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--install-index".to_string(),
            "file:///tmp/index.json".to_string(),
            "snake-plus".to_string(),
            "--version".to_string(),
            "0.2.0".to_string(),
        ])
        .expect("install-index mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Operation(ContentOperation::InstallIndex {
                locator: "file:///tmp/index.json".to_string(),
                game_id: "snake-plus".to_string(),
                version: Some("0.2.0".to_string()),
            })
        );
    }

    #[test]
    fn parses_remove_launch_mode() {
        let mode = parse_launch_mode(vec!["--remove".to_string(), "snake-plus".to_string()])
            .expect("remove mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Operation(ContentOperation::Remove {
                game_id: "snake-plus".to_string(),
            })
        );
    }

    #[test]
    fn parses_registry_list_launch_mode() {
        let mode = parse_launch_mode(vec!["--registry-list".to_string()])
            .expect("registry-list mode should parse");

        assert_eq!(mode, LaunchMode::Registry(RegistryCommand::List));
    }

    #[test]
    fn parses_registry_add_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--registry-add".to_string(),
            "file:///tmp/index.json".to_string(),
        ])
        .expect("registry-add mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Registry(RegistryCommand::Add {
                locator: "file:///tmp/index.json".to_string(),
            })
        );
    }

    #[test]
    fn parses_registry_remove_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--registry-remove".to_string(),
            "file:///tmp/index.json".to_string(),
        ])
        .expect("registry-remove mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Registry(RegistryCommand::Remove {
                locator: "file:///tmp/index.json".to_string(),
            })
        );
    }

    #[test]
    fn rejects_unknown_launch_argument() {
        let error = parse_launch_mode(vec!["--nope".to_string()]);
        assert!(error.is_err());
    }

    #[test]
    fn overlay_blocks_runner_input_forwarding() {
        let commands = vec![ShellCommand::None];
        assert!(!should_forward_key_to_runner(
            &commands,
            &Route::Runner,
            Some(Overlay::RunnerQuitConfirm),
            true
        ));
    }

    #[test]
    fn unhandled_runner_key_without_overlay_is_forwarded() {
        let commands = vec![ShellCommand::None];
        assert!(should_forward_key_to_runner(
            &commands,
            &Route::Runner,
            None,
            true
        ));
    }

    #[test]
    fn hotload_signature_changes_when_installed_changes() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = content::JsonContentStore::new(temp.path().to_path_buf());
        store.ensure_layout()?;

        let before = compute_hotload_signature(store.root())?;
        let mut installed = store.load_installed()?;
        installed.installed.push(content::InstalledRecord {
            id: "sample-game".to_string(),
            source: "local://fixture".to_string(),
            current_version: "0.1.0".to_string(),
            installed_versions: vec!["0.1.0".to_string()],
            version_checksums: std::collections::BTreeMap::new(),
        });
        store.save_installed(&installed)?;
        let after = compute_hotload_signature(store.root())?;

        assert_ne!(before, after);
        Ok(())
    }

    #[test]
    fn rollback_operation_effects_current_pointer() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();
        let artifact_v1 = root.join("artifact-v1");
        let artifact_v2 = root.join("artifact-v2");
        std::fs::create_dir_all(&artifact_v1)?;
        std::fs::create_dir_all(&artifact_v2)?;

        std::fs::write(
            artifact_v1.join("game.json"),
            serde_json::json!({
                "id": "snake-plus",
                "name": "Snake+",
                "version": "0.1.0",
                "author": "Dark Forest",
                "entry_type": "wasm",
                "entry": "main.wasm",
                "host_api": "^0.1",
                "permissions": ["terminal.raw_input"]
            })
            .to_string(),
        )?;
        std::fs::write(artifact_v1.join("main.wasm"), b"v1")?;

        std::fs::write(
            artifact_v2.join("game.json"),
            serde_json::json!({
                "id": "snake-plus",
                "name": "Snake+",
                "version": "0.2.0",
                "author": "Dark Forest",
                "entry_type": "wasm",
                "entry": "main.wasm",
                "host_api": "^0.1",
                "permissions": ["terminal.raw_input"]
            })
            .to_string(),
        )?;
        std::fs::write(artifact_v2.join("main.wasm"), b"v2")?;

        let install1 = execute_content_operation(
            root.clone(),
            ContentOperation::InstallLocal {
                artifact_dir: artifact_v1,
                source: Some("local://fixture".to_string()),
            },
        )?;
        assert!(install1.success);

        let install2 = execute_content_operation(
            root.clone(),
            ContentOperation::InstallLocal {
                artifact_dir: artifact_v2,
                source: Some("local://fixture".to_string()),
            },
        )?;
        assert!(install2.success);

        let rollback = execute_content_operation(
            root.clone(),
            ContentOperation::Rollback {
                game_id: "snake-plus".to_string(),
            },
        )?;
        assert!(rollback.success);

        let store = content::JsonContentStore::new(root);
        assert_eq!(
            store.read_current_pointer("snake-plus"),
            Some("0.1.0".to_string())
        );
        Ok(())
    }

    #[test]
    fn remove_operation_deletes_installed_record_and_payload() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();
        let artifact_v1 = root.join("artifact-v1");
        std::fs::create_dir_all(&artifact_v1)?;

        std::fs::write(
            artifact_v1.join("game.json"),
            serde_json::json!({
                "id": "snake-plus",
                "name": "Snake+",
                "version": "0.1.0",
                "author": "Dark Forest",
                "entry_type": "wasm",
                "entry": "main.wasm",
                "host_api": "^0.1",
                "permissions": ["terminal.raw_input"]
            })
            .to_string(),
        )?;
        std::fs::write(artifact_v1.join("main.wasm"), b"v1")?;

        let install = execute_content_operation(
            root.clone(),
            ContentOperation::InstallLocal {
                artifact_dir: artifact_v1,
                source: Some("local://fixture".to_string()),
            },
        )?;
        assert!(install.success);

        let remove = execute_content_operation(
            root.clone(),
            ContentOperation::Remove {
                game_id: "snake-plus".to_string(),
            },
        )?;
        assert!(remove.success);

        let store = content::JsonContentStore::new(root.clone());
        let installed = store.load_installed()?;
        assert!(
            !installed
                .installed
                .iter()
                .any(|item| item.id == "snake-plus")
        );
        assert!(!root.join("games/snake-plus").exists());
        Ok(())
    }

    #[test]
    fn registry_add_is_idempotent() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        let first = execute_registry_command(
            root.clone(),
            RegistryCommand::Add {
                locator: "file:///tmp/index.json".to_string(),
            },
        )?;
        assert!(first.success);

        let second = execute_registry_command(
            root.clone(),
            RegistryCommand::Add {
                locator: "file:///tmp/index.json".to_string(),
            },
        )?;
        assert!(second.success);
        assert_eq!(second.registry_locators, vec!["file:///tmp/index.json"]);

        let listed = execute_registry_command(root, RegistryCommand::List)?;
        assert!(listed.success);
        assert_eq!(listed.registry_locators, vec!["file:///tmp/index.json"]);
        Ok(())
    }

    #[test]
    fn registry_remove_missing_is_failure() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        let removed = execute_registry_command(
            root,
            RegistryCommand::Remove {
                locator: "file:///tmp/missing-index.json".to_string(),
            },
        )?;
        assert!(!removed.success);
        assert_eq!(
            removed.message,
            "registry not configured: file:///tmp/missing-index.json"
        );
        Ok(())
    }

    #[test]
    fn registry_list_preserves_order() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        execute_registry_command(
            root.clone(),
            RegistryCommand::Add {
                locator: "file:///tmp/first.json".to_string(),
            },
        )?;
        execute_registry_command(
            root.clone(),
            RegistryCommand::Add {
                locator: "file:///tmp/second.json".to_string(),
            },
        )?;

        let listed = execute_registry_command(root, RegistryCommand::List)?;
        assert!(listed.success);
        assert_eq!(
            listed.registry_locators,
            vec!["file:///tmp/first.json", "file:///tmp/second.json"]
        );
        Ok(())
    }

    #[test]
    fn marketplace_catalog_loads_and_reports_errors() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        let index_path = root.join("index.json");
        std::fs::write(
            &index_path,
            serde_json::json!({
                "schema_version": 1,
                "games": [{
                    "id": "remote-snake",
                    "name": "Remote Snake",
                    "description": "Remote listing",
                    "tags": ["arcade", "remote"],
                    "author": "Remote Team",
                    "versions": [{
                        "version": "1.0.0",
                        "artifact": "remote-snake-1.0.0.tar.gz"
                    }]
                }]
            })
            .to_string(),
        )?;

        let settings = content::Settings {
            registries: vec![
                content::RegistryConfig {
                    scheme: "index".to_string(),
                    locator: index_path.to_string_lossy().to_string(),
                },
                content::RegistryConfig {
                    scheme: "index".to_string(),
                    locator: root.join("missing.json").to_string_lossy().to_string(),
                },
            ],
            ..content::Settings::default()
        };

        let snapshot = load_marketplace_catalog(&settings, root.join("cache"));
        assert!(
            snapshot.games.iter().any(|game| game.id == "remote-snake"),
            "expected marketplace game to load"
        );
        assert!(
            snapshot
                .warnings
                .iter()
                .any(|item| item.contains("missing.json")),
            "expected warning for failed registry fetch"
        );
        Ok(())
    }

    #[test]
    fn marketplace_catalog_deduplicates_by_first_registry() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        let first_index = root.join("first.json");
        std::fs::write(
            &first_index,
            serde_json::json!({
                "schema_version": 1,
                "games": [{
                    "id": "dup-game",
                    "name": "First Name",
                    "description": "From first",
                    "tags": ["first"],
                    "author": "A",
                    "versions": [{
                        "version": "1.0.0",
                        "artifact": "dup-game-1.0.0.tar.gz"
                    }]
                }]
            })
            .to_string(),
        )?;

        let second_index = root.join("second.json");
        std::fs::write(
            &second_index,
            serde_json::json!({
                "schema_version": 1,
                "games": [{
                    "id": "dup-game",
                    "name": "Second Name",
                    "description": "From second",
                    "tags": ["second"],
                    "author": "B",
                    "versions": [{
                        "version": "2.0.0",
                        "artifact": "dup-game-2.0.0.tar.gz"
                    }]
                }]
            })
            .to_string(),
        )?;

        let settings = content::Settings {
            registries: vec![
                content::RegistryConfig {
                    scheme: "index".to_string(),
                    locator: first_index.to_string_lossy().to_string(),
                },
                content::RegistryConfig {
                    scheme: "index".to_string(),
                    locator: second_index.to_string_lossy().to_string(),
                },
            ],
            ..content::Settings::default()
        };

        let snapshot = load_marketplace_catalog(&settings, root.join("cache"));
        assert_eq!(snapshot.games.len(), 1);
        assert_eq!(snapshot.games[0].name, "First Name");
        assert_eq!(
            snapshot.install_locators.get("dup-game"),
            Some(&first_index.to_string_lossy().to_string())
        );
        assert!(
            snapshot
                .warnings
                .iter()
                .any(|item| item.contains("duplicate marketplace id")),
            "expected duplicate warning"
        );
        Ok(())
    }
}
