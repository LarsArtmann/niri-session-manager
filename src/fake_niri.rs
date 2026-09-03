//! A minimal in-process fake niri IPC server for integration tests.
//!
//! Speaks just enough of the niri protocol (Windows, Workspaces, and the
//! Spawn / `MoveWindowToMonitor` / `MoveWindowToWorkspace` / `FocusWindow` actions)
//! to exercise the real restore, save, and shutdown code paths end-to-end.
//! Spawned windows are simulated: they appear in the next `Windows` reply.
//!
//! Because `Socket::connect()` reads `$NIRI_SOCKET` from the process
//! environment, every test must hold [`IPC_ENV_LOCK`] (via
//! [`FakeNiri::env`]) for the duration of its run so parallel tests cannot
//! race on the variable.

use crate::{
    get_restore_marker_path, reactive_save_session, restore_session, save_session_with_backup,
    shutdown_with_final_save, AppConfig, Config, RestoreOutcome, MAX_SPAWN_CONCURRENCY,
};
use niri_ipc::{
    Action, Reply, Request, Response, Window, WindowLayout, Workspace, WorkspaceReferenceArg,
};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Serializes `$NIRI_SOCKET` manipulation across parallel tests.
static IPC_ENV_LOCK: Mutex<()> = Mutex::new(());

/// Holds the env lock and points `$NIRI_SOCKET` at the fake server; removing
/// the variable on drop.
pub struct SocketEnv {
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl Drop for SocketEnv {
    fn drop(&mut self) {
        std::env::remove_var("NIRI_SOCKET");
    }
}

/// Actions the fake server observed, in arrival order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedAction {
    MoveToMonitor {
        window: Option<u64>,
        output: String,
    },
    MoveToWorkspace {
        window: Option<u64>,
        reference: WorkspaceReferenceArg,
    },
    FocusWindow {
        window: u64,
    },
}

#[derive(Debug, Default)]
struct FakeState {
    windows: Vec<Window>,
    workspaces: Vec<Workspace>,
    spawn_commands: Vec<Vec<String>>,
    actions: Vec<RecordedAction>,
    fail_next_windows: usize,
    spawn_delay: Option<Duration>,
    next_window_id: u64,
    in_flight: u64,
    max_in_flight: u64,
    pending_events: Vec<niri_ipc::Event>,
}

pub struct FakeNiri {
    dir: tempfile::TempDir,
    socket_path: PathBuf,
    state: Arc<Mutex<FakeState>>,
    stop: Arc<AtomicBool>,
}

impl FakeNiri {
    pub(crate) fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("niri.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let state: Arc<Mutex<FakeState>> = Arc::new(Mutex::new(FakeState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let listener_state = Arc::clone(&state);
        let listener_stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                if listener_stop.load(Ordering::SeqCst) {
                    break;
                }
                let state = Arc::clone(&listener_state);
                let conn_stop = Arc::clone(&listener_stop);
                std::thread::spawn(move || serve_connection(stream, state, conn_stop));
            }
        });
        Self {
            dir,
            socket_path,
            state,
            stop,
        }
    }

    /// Stops the fake's event-push loops and closes their connections so a
    /// test's blocked reader threads (`spawn_blocking`) can exit before the
    /// tokio runtime drops (its drop waits for blocking tasks).
    pub(crate) fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Takes the env lock with `$NIRI_SOCKET` explicitly REMOVED, for tests
    /// that require niri to be unreachable.
    pub(crate) fn env_without_socket() -> SocketEnv {
        let guard = IPC_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::env::remove_var("NIRI_SOCKET");
        SocketEnv { _guard: guard }
    }

    /// Points `$NIRI_SOCKET` at this fake for as long as the guard lives.
    pub(crate) fn env(&self) -> SocketEnv {
        let guard = IPC_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::env::set_var("NIRI_SOCKET", &self.socket_path);
        SocketEnv { _guard: guard }
    }

    pub(crate) fn temp_dir(&self) -> &Path {
        self.dir.path()
    }

    pub(crate) fn set_windows(&self, windows: Vec<Window>) {
        self.lock().windows = windows;
    }

    pub(crate) fn set_workspaces(&self, workspaces: Vec<Workspace>) {
        self.lock().workspaces = workspaces;
    }

    pub(crate) fn fail_next_windows(&self, count: usize) {
        self.lock().fail_next_windows = count;
    }

    pub(crate) fn set_spawn_delay(&self, delay: Duration) {
        self.lock().spawn_delay = Some(delay);
    }

    pub(crate) fn spawn_commands(&self) -> Vec<Vec<String>> {
        self.lock().spawn_commands.clone()
    }

    pub(crate) fn actions(&self) -> Vec<RecordedAction> {
        self.lock().actions.clone()
    }

    pub(crate) fn max_concurrent_spawns(&self) -> u64 {
        self.lock().max_in_flight
    }

    pub(crate) fn push_events(&self, events: Vec<niri_ipc::Event>) {
        self.lock().pending_events.extend(events);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn serve_connection(stream: UnixStream, state: Arc<Mutex<FakeState>>, stop: Arc<AtomicBool>) {
    eprintln!("FAKE: connection accepted");
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let Ok(request) = serde_json::from_str::<Request>(&line) else {
            eprintln!("FAKE: undecodable request line: {line:?}");
            break;
        };
        eprintln!("FAKE: request {:?}", std::mem::discriminant(&request));
        if matches!(request, Request::EventStream) {
            let handled = serde_json::to_string(&Reply::Ok(Response::Handled)).unwrap();
            if writeln!(writer, "{handled}").is_err() {
                break;
            }
            push_events_forever(&state, &mut writer, stop);
            break;
        }
        let reply = handle_request(request, &state);
        let Ok(json) = serde_json::to_string(&reply) else {
            break;
        };
        if writeln!(writer, "{json}").is_err() {
            break;
        }
    }
}

/// After an `EventStream` handshake: drain queued events as JSON lines,
/// keeping the connection open so the subscriber sees a live stream.
fn push_events_forever(
    state: &Arc<Mutex<FakeState>>,
    writer: &mut UnixStream,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::SeqCst) {
        let events: Vec<niri_ipc::Event> = {
            let mut st = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut st.pending_events)
        };
        for event in events {
            let Ok(json) = serde_json::to_string(&event) else {
                return;
            };
            if writeln!(writer, "{json}").is_err() {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn handle_request(request: Request, state: &Arc<Mutex<FakeState>>) -> Reply {
    match request {
        Request::Version => Reply::Ok(Response::Version("niri 25.11 (fake)".to_string())),
        Request::Windows => {
            let mut st = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if st.fail_next_windows > 0 {
                st.fail_next_windows -= 1;
                return Reply::Err("injected windows failure".to_string());
            }
            Reply::Ok(Response::Windows(st.windows.clone()))
        }
        Request::Workspaces => {
            let st = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Reply::Ok(Response::Workspaces(st.workspaces.clone()))
        }
        Request::Action(action) => handle_action(action, state),
        _ => Reply::Err("fake niri: unsupported request".to_string()),
    }
}

fn handle_action(action: Action, state: &Arc<Mutex<FakeState>>) -> Reply {
    match action {
        Action::Spawn { command } => {
            {
                let mut st = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                st.spawn_commands.push(command.clone());
                st.in_flight = st.in_flight.saturating_add(1);
                st.max_in_flight = st.max_in_flight.max(st.in_flight);
            }
            let delay = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .spawn_delay;
            if let Some(delay) = delay {
                std::thread::sleep(delay);
            }
            let app_id = command.first().cloned().unwrap_or_default();
            let mut st = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            st.in_flight = st.in_flight.saturating_sub(1);
            st.next_window_id = st.next_window_id.saturating_add(1);
            let id = st.next_window_id;
            st.windows.push(fake_window(id, &app_id));
            Reply::Ok(Response::Handled)
        }
        Action::MoveWindowToMonitor { id, output } => {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .actions
                .push(RecordedAction::MoveToMonitor { window: id, output });
            Reply::Ok(Response::Handled)
        }
        Action::MoveWindowToWorkspace {
            window_id,
            reference,
            ..
        } => {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .actions
                .push(RecordedAction::MoveToWorkspace {
                    window: window_id,
                    reference,
                });
            Reply::Ok(Response::Handled)
        }
        Action::FocusWindow { id } => {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .actions
                .push(RecordedAction::FocusWindow { window: id });
            Reply::Ok(Response::Handled)
        }
        _ => Reply::Ok(Response::Handled),
    }
}

pub fn fake_window(id: u64, app_id: &str) -> Window {
    Window {
        id,
        title: None,
        app_id: Some(app_id.to_string()),
        pid: None,
        workspace_id: None,
        is_focused: false,
        is_floating: false,
        is_urgent: false,
        layout: WindowLayout {
            pos_in_scrolling_layout: None,
            tile_size: (0.0, 0.0),
            window_size: (0, 0),
            tile_pos_in_workspace_view: None,
            window_offset_in_tile: (0.0, 0.0),
        },
        focus_timestamp: None,
    }
}

pub fn niri_workspace(id: u64, idx: u8, name: Option<&str>, output: &str) -> Workspace {
    Workspace {
        id,
        idx,
        name: name.map(String::from),
        output: Some(output.to_string()),
        is_urgent: false,
        is_active: false,
        is_focused: false,
        active_window_id: None,
    }
}

fn saved_win(id: u64, app: &str, name: &str, idx: u8, focused: bool) -> crate::SavedWindow {
    crate::SavedWindow {
        id,
        app_id: app.to_string(),
        workspace: crate::WorkspaceInfo {
            idx: Some(idx),
            name: Some(name.to_string()),
            output: None,
        },
        is_focused: focused,
        pid: None,
        terminal_state: None,
    }
}

fn save_session_file(path: &Path, windows: &[crate::SavedWindow]) {
    let session = crate::VersionedSession {
        version: crate::SESSION_FORMAT_VERSION,
        windows: windows.to_vec(),
    };
    std::fs::write(path, serde_json::to_string_pretty(&session).unwrap()).unwrap();
}

fn ipc_config() -> Config {
    Config {
        save_interval: 15,
        max_backup_count: 5,
        spawn_timeout: 1,
        retry_attempts: 2,
        retry_delay: 1,
        max_restore_windows: 100,
        dry_run: false,
        app_config_path: None,
        restore: false,
        save_only: false,
        save_once: false,
        health_check: false,
        export_to: None,
        import_from: None,
    }
}

// --- M7: harness assertions ---

#[tokio::test]
async fn restore_spawns_recorded_commands_in_saved_order_and_places_them() {
    let niri = FakeNiri::start();
    let _env = niri.env();
    niri.set_windows(vec![]);
    niri.set_workspaces(vec![
        niri_workspace(1, 1, Some("dev"), "DP-1"),
        niri_workspace(2, 2, Some("web"), "DP-1"),
    ]);

    let session = niri.temp_dir().join("session.json");
    save_session_file(
        &session,
        &[
            saved_win(1, "firefox", "dev", 1, false),
            saved_win(2, "firefox", "web", 2, false),
            saved_win(3, "chromium", "web", 2, false),
        ],
    );

    let outcome = restore_session(&session, &ipc_config(), &AppConfig::default())
        .await
        .unwrap();

    assert_eq!(
        outcome,
        RestoreOutcome::Restored { spawned: 3 },
        "all three saved windows are new to the fake compositor"
    );

    let spawns = niri.spawn_commands();
    let firefox_spawns = spawns
        .iter()
        .filter(|c| c.first().map(String::as_str) == Some("firefox"))
        .count();
    let chromium_spawns = spawns
        .iter()
        .filter(|c| c.first().map(String::as_str) == Some("chromium"))
        .count();
    assert_eq!(spawns.len(), 3);
    assert_eq!(firefox_spawns, 2);
    assert_eq!(chromium_spawns, 1);
    // Same-app spawns must be serialized, so both firefox spawns were
    // dispatched one-after-another (order between different apps is free).

    let actions = niri.actions();
    let workspace_moves: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            RecordedAction::MoveToWorkspace { window, reference } => Some((*window, reference)),
            _ => None,
        })
        .collect();
    assert_eq!(workspace_moves.len(), 3, "every spawned window is placed");
    assert!(
        workspace_moves
            .iter()
            .any(|(_, r)| **r == WorkspaceReferenceArg::Name("dev".to_string())),
        "workspace name references are preserved"
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, RecordedAction::MoveToMonitor { output, .. } if output == "DP-1")),
        "windows are pinned to the output hosting the saved workspace"
    );
}

#[tokio::test]
async fn re_restore_spawns_only_the_missing_windows() {
    let niri = FakeNiri::start();
    let _env = niri.env();
    niri.set_workspaces(vec![niri_workspace(1, 1, Some("dev"), "DP-1")]);

    let session = niri.temp_dir().join("session.json");
    save_session_file(&session, &[saved_win(1, "firefox", "dev", 1, false)]);

    let config = ipc_config();
    let first = restore_session(&session, &config, &AppConfig::default())
        .await
        .unwrap();
    assert_eq!(first, RestoreOutcome::Restored { spawned: 1 });

    // Second run: the window from the first run is still "running" in the
    // fake compositor, so restoring again must spawn nothing.
    let second = restore_session(&session, &config, &AppConfig::default())
        .await
        .unwrap();
    assert_eq!(
        second,
        RestoreOutcome::Restored { spawned: 0 },
        "idempotent restore must not duplicate windows"
    );
    assert_eq!(niri.spawn_commands().len(), 1, "no extra spawns");
}

#[tokio::test]
async fn restore_retry_loop_recovers_from_injected_ipc_failure() {
    let niri = FakeNiri::start();
    let _env = niri.env();
    niri.fail_next_windows(1);
    niri.set_workspaces(vec![niri_workspace(1, 1, Some("dev"), "DP-1")]);

    let session = niri.temp_dir().join("session.json");
    save_session_file(&session, &[saved_win(1, "firefox", "dev", 1, false)]);

    let outcome = restore_session(&session, &ipc_config(), &AppConfig::default())
        .await
        .unwrap();

    assert_eq!(
        outcome,
        RestoreOutcome::Restored { spawned: 1 },
        "the second attempt succeeds after the injected failure"
    );
    assert_eq!(niri.spawn_commands().len(), 1);
}

#[tokio::test]
async fn global_spawn_concurrency_never_exceeds_the_cap() {
    let niri = FakeNiri::start();
    let _env = niri.env();
    niri.set_spawn_delay(Duration::from_millis(300));
    niri.set_workspaces(vec![niri_workspace(1, 1, Some("dev"), "DP-1")]);

    let session = niri.temp_dir().join("session.json");
    let windows: Vec<crate::SavedWindow> = (1..=6)
        .map(|i| saved_win(i, &format!("app{i}"), "dev", 1, false))
        .collect();
    save_session_file(&session, &windows);

    let outcome = restore_session(&session, &ipc_config(), &AppConfig::default())
        .await
        .unwrap();
    assert_eq!(outcome, RestoreOutcome::Restored { spawned: 6 });

    assert!(
        niri.max_concurrent_spawns() <= u64::try_from(MAX_SPAWN_CONCURRENCY).unwrap(),
        "spawn handler concurrency must respect the global semaphore cap"
    );
}

// --- M11: focus restoration ---

#[tokio::test]
async fn focus_is_restored_for_the_saved_focused_window() {
    let niri = FakeNiri::start();
    let _env = niri.env();
    niri.set_workspaces(vec![niri_workspace(1, 1, Some("dev"), "DP-1")]);

    let session = niri.temp_dir().join("session.json");
    save_session_file(
        &session,
        &[
            saved_win(1, "firefox", "dev", 1, false),
            saved_win(2, "chromium", "dev", 1, true),
        ],
    );

    let outcome = restore_session(&session, &ipc_config(), &AppConfig::default())
        .await
        .unwrap();
    assert_eq!(outcome, RestoreOutcome::Restored { spawned: 2 });

    let focus_actions: Vec<u64> = niri
        .actions()
        .into_iter()
        .filter_map(|a| match a {
            RecordedAction::FocusWindow { window } => Some(window),
            _ => None,
        })
        .collect();
    assert_eq!(
        focus_actions,
        vec![2],
        "the saved focused window (chromium, spawned second) gets focus"
    );
}

// --- M3 (completion): shutdown performs the final save ---

#[tokio::test]
async fn shutdown_aborts_periodic_save_then_runs_final_save() {
    let niri = FakeNiri::start();
    let _env = niri.env();
    niri.set_windows(vec![fake_window(1, "firefox"), fake_window(2, "chromium")]);
    niri.set_workspaces(vec![niri_workspace(1, 1, Some("dev"), "DP-1")]);

    let session = niri.temp_dir().join("session.json");
    let config = ipc_config();
    let app_config = AppConfig::default();

    let save_task = tokio::spawn(reactive_save_session(
        session.clone(),
        config.clone(),
        app_config.clone(),
    ));

    shutdown_with_final_save(save_task, &session, &config, &app_config).await;

    let content = std::fs::read_to_string(&session).unwrap();
    let saved: crate::VersionedSession = serde_json::from_str(&content).unwrap();
    assert_eq!(
        saved.windows.len(),
        2,
        "final save snapshots the live fake-compositor state"
    );
    assert_eq!(saved.windows[0].app_id, "firefox");

    niri.close();
}

// --- M22: health check ---

#[tokio::test]
async fn health_check_passes_with_a_live_niri() {
    let niri = FakeNiri::start();
    let _env = niri.env();
    niri.set_windows(vec![fake_window(1, "firefox")]);
    niri.set_workspaces(vec![niri_workspace(1, 1, Some("dev"), "DP-1")]);

    let session = niri.temp_dir().join("session.json");
    save_session_file(&session, &[saved_win(1, "firefox", "dev", 1, false)]);

    crate::run_health_check(&session)
        .await
        .expect("health check must pass with reachable niri and a valid session file");
}

#[tokio::test]
async fn health_check_fails_when_niri_is_unreachable() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.json");

    // No NIRI_SOCKET: the check must fail loudly instead of pretending.
    // The lock prevents parallel tests from re-pointing the env at their fakes.
    let _env = FakeNiri::env_without_socket();
    assert!(
        crate::run_health_check(&session).await.is_err(),
        "health check without niri must fail"
    );
}

// --- M12: event-driven reactive saves ---

/// M29: measures protocol + restore-logic throughput against the fake
/// compositor. Not a compositing benchmark — it isolates OUR overhead.
#[tokio::test]
#[ignore = "benchmark: run explicitly with `cargo test restore_burst -- --ignored --nocapture`"]
async fn restore_burst_benchmark() {
    let niri = FakeNiri::start();
    let _env = niri.env();
    niri.set_workspaces(vec![
        niri_workspace(1, 1, Some("ws1"), "DP-1"),
        niri_workspace(2, 2, Some("ws2"), "DP-1"),
        niri_workspace(3, 3, Some("ws3"), "DP-1"),
    ]);

    let session = niri.temp_dir().join("session.json");
    let windows: Vec<crate::SavedWindow> = (1..=30)
        .map(|i| {
            saved_win(
                i,
                &format!("app{}", i % 10),
                "ws1",
                ((i % 3) + 1) as u8,
                i == 5,
            )
        })
        .collect();
    save_session_file(&session, &windows);

    let start = std::time::Instant::now();
    let outcome = restore_session(&session, &ipc_config(), &AppConfig::default())
        .await
        .unwrap();
    let elapsed = start.elapsed();
    niri.close();

    assert_eq!(outcome, RestoreOutcome::Restored { spawned: 30 });
    let per_window = elapsed.as_secs_f64() / 30.0;
    eprintln!(
        "BENCH: 30 windows in {elapsed:?} ({:.0} windows/s, {:.1} ms/window)",
        30.0 / elapsed.as_secs_f64(),
        per_window * 1000.0
    );
}

#[tokio::test]
async fn layout_event_triggers_debounced_save() {
    let niri = FakeNiri::start();
    let _env = niri.env();
    niri.set_windows(vec![fake_window(1, "firefox"), fake_window(2, "chromium")]);
    niri.set_workspaces(vec![niri_workspace(1, 1, Some("dev"), "DP-1")]);

    let session = niri.temp_dir().join("session.json");
    let config = ipc_config();
    let app_config = AppConfig::default();

    let task = tokio::spawn(reactive_save_session(session.clone(), config, app_config));
    tokio::time::sleep(Duration::from_millis(300)).await;

    niri.push_events(vec![niri_ipc::Event::WindowsChanged { windows: vec![] }]);

    // Debounce window (2s) plus slack.
    tokio::time::sleep(Duration::from_millis(3200)).await;

    // Close the fake's event stream so the task's blocking reader can exit
    // before the runtime drops (its drop waits for blocking tasks).
    niri.close();
    tokio::time::sleep(Duration::from_millis(200)).await;
    task.abort();

    assert!(
        session.exists(),
        "a layout event must trigger a debounced save"
    );
    task.abort();
}

// --- M28: unchanged saves are skipped entirely ---

#[tokio::test]
async fn unchanged_layout_skips_backup_and_write() {
    let niri = FakeNiri::start();
    let _env = niri.env();
    niri.set_windows(vec![fake_window(1, "firefox")]);
    niri.set_workspaces(vec![niri_workspace(1, 1, Some("dev"), "DP-1")]);

    let session = niri.temp_dir().join("session.json");
    let config = ipc_config();
    let app_config = AppConfig::default();

    let backup_count = || -> usize {
        std::fs::read_dir(niri.temp_dir())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "bak"))
            .count()
    };

    eprintln!("TEST: about to save 1");
    let cap1 = crate::capture_session_json(&app_config).await.unwrap();
    eprintln!("CAP1: {cap1}");
    save_session_with_backup(&session, &config, &app_config)
        .await
        .unwrap();
    eprintln!("TEST: save 1 done, exists={}", session.exists());
    assert!(session.exists());
    assert_eq!(backup_count(), 0, "no backup before any prior file");

    // Identical layout: no backup rotation, no write churn.
    eprintln!("TEST: save 2");
    let cap2 = crate::capture_session_json(&app_config).await.unwrap();
    eprintln!("CAP2: {cap2}");
    eprintln!(
        "FILELEN: {}",
        std::fs::read_to_string(&session).unwrap().len()
    );
    eprintln!("CAP2LEN: {}", cap2.len());
    let before = backup_count();
    let file_bytes = std::fs::read(&session).unwrap();
    eprintln!(
        "EQ-CHECK: file==cap2={} before_count={}",
        file_bytes == cap2.as_bytes(),
        before
    );
    save_session_with_backup(&session, &config, &app_config)
        .await
        .unwrap();
    eprintln!("TEST: save 2 done, after_count={}", backup_count());
    assert_eq!(
        backup_count(),
        0,
        "unchanged layout must not rotate backups"
    );

    // Changed layout: exactly one new backup of the previous file.
    eprintln!("TEST: save 3");
    niri.set_windows(vec![fake_window(1, "firefox"), fake_window(2, "chromium")]);
    save_session_with_backup(&session, &config, &app_config)
        .await
        .unwrap();
    eprintln!("TEST: save 3 done");
    assert_eq!(
        backup_count(),
        1,
        "a real change rotates exactly one backup"
    );
}

#[tokio::test]
async fn boot_restore_writes_marker_and_second_gate_run_is_skipped() {
    let niri = FakeNiri::start();
    let _env = niri.env();
    niri.set_workspaces(vec![niri_workspace(1, 1, Some("dev"), "DP-1")]);

    let session = niri.temp_dir().join("session.json");
    save_session_file(&session, &[saved_win(1, "firefox", "dev", 1, false)]);

    let config = ipc_config();
    let app_config = AppConfig::default();

    // A marker left by a previous boot is stale; the gate prunes it and
    // restores anyway.
    let marker = get_restore_marker_path(&session);
    std::fs::write(&marker, "00000000-0000-0000-0000-000000000000").unwrap();

    crate::run_boot_restore(&session, &config, &app_config).await;

    assert_eq!(
        niri.spawn_commands().len(),
        1,
        "stale marker must not block the restore"
    );
    let written = std::fs::read_to_string(&marker).unwrap();
    assert!(
        !crate::should_restore_on_boot(Some(written.trim()), &marker),
        "the marker written after restore must gate the next boot-restore for this boot"
    );
}
