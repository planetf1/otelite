//! End-to-end test for `otelite usage --codex-subagents` (#184, option B):
//! seed a temp database with Codex multi-agent metrics (the shapes observed
//! in production data) and deep-check the emitted JSON.

use assert_cmd::Command;
use otelite_core::telemetry::metric::{Metric, MetricType};
use otelite_storage::sqlite::SqliteBackend;
use otelite_storage::{StorageBackend, StorageConfig};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64
}

async fn write_metric(
    storage: &SqliteBackend,
    name: &str,
    ts: i64,
    attributes: HashMap<String, String>,
) {
    let metric = Metric {
        name: name.to_string(),
        description: None,
        unit: None,
        metric_type: MetricType::Counter(1),
        timestamp: ts,
        attributes,
        resource: None,
    };
    storage.write_metric(&metric).await.unwrap();
}

async fn build_db(data_dir: &std::path::Path) {
    let storage_config = StorageConfig::default().with_data_dir(data_dir.to_path_buf());
    let mut storage = SqliteBackend::new(storage_config);
    storage.initialize().await.unwrap();

    let now = now_ns();
    let uuid_a = "01a00000-0000-7000-8000-00000000000a";
    let uuid_b = "01b00000-0000-7000-8000-00000000000b";
    let attrs = |session_source: &str| {
        HashMap::from([("session_source".to_string(), session_source.to_string())])
    };

    // Session A: three sub-agent starts; session B: one. Plain (non
    // sub-agent) thread starts and a truncated session_source are also
    // present and must be ignored.
    for off in [5_000_000_000i64, 40_000_000_000i64, 80_000_000_000i64] {
        write_metric(
            &storage,
            "codex.thread.started",
            now - off,
            attrs(&format!("subagent_thread_spawn_{uuid_a}_d1")),
        )
        .await;
    }
    write_metric(
        &storage,
        "codex.thread.started",
        now - 60_000_000_000,
        attrs(&format!("subagent_thread_spawn_{uuid_b}_d1")),
    )
    .await;
    write_metric(
        &storage,
        "codex.thread.started",
        now - 5_500_000_000,
        attrs("cli"),
    )
    .await;
    write_metric(
        &storage,
        "codex.thread.started",
        now - 5_600_000_000,
        attrs("subagent_thread_spawn_01a00000"),
    )
    .await;
    // Spawns (with roles) and a resume.
    for (off, role) in [
        (5_100_000_000i64, "reviewer"),
        (40_100_000_000i64, "reviewer"),
        (80_100_000_000i64, "worker"),
    ] {
        write_metric(
            &storage,
            "codex.multi_agent.spawn",
            now - off,
            HashMap::from([("role".to_string(), role.to_string())]),
        )
        .await;
    }
    write_metric(
        &storage,
        "codex.multi_agent.resume",
        now - 60_100_000_000,
        HashMap::new(),
    )
    .await;
}

#[tokio::test]
async fn usage_codex_subagents_json() {
    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    build_db(&data_dir).await;

    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    let output = Command::cargo_bin("otelite")
        .expect("otelite binary should build")
        .args([
            "--format",
            "json",
            "usage",
            "--codex-subagents",
            "--since",
            "24h",
        ])
        .env("OTELITE_DATA_DIR", &data_dir)
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("CLI stdout must be JSON");

    let resp = actual
        .get("codex_subagents")
        .expect("codex_subagents in output");

    let sessions = resp["sessions"].as_array().expect("sessions array");
    assert_eq!(
        sessions.len(),
        2,
        "only sub-agent starts count: {sessions:?}"
    );
    let a = &sessions[0];
    assert_eq!(a["thread_id"], uuid_session_a());
    assert_eq!(a["subagents"], 3);
    let b = &sessions[1];
    assert_eq!(b["thread_id"], uuid_session_b());
    assert_eq!(b["subagents"], 1);

    let roles = resp["roles"].as_array().expect("roles array");
    assert_eq!(roles.len(), 2);
    assert_eq!(roles[0]["role"], "reviewer");
    assert_eq!(roles[0]["spawns"], 2);
    assert_eq!(roles[1]["role"], "worker");
    assert_eq!(roles[1]["spawns"], 1);

    let daily = resp["daily"].as_array().expect("daily array");
    assert!(!daily.is_empty());
    // All events are within the last ~2h, so a single UTC day is expected
    // unless the test straddles midnight — sum across days instead.
    let total_subs: u64 = daily.iter().map(|d| d["subagents"].as_u64().unwrap()).sum();
    assert_eq!(total_subs, 4);
    let total_spawns: u64 = daily.iter().map(|d| d["spawns"].as_u64().unwrap()).sum();
    assert_eq!(total_spawns, 3);
    let total_resumes: u64 = daily.iter().map(|d| d["resumes"].as_u64().unwrap()).sum();
    assert_eq!(total_resumes, 1);
}

fn uuid_session_a() -> &'static str {
    "01a00000-0000-7000-8000-00000000000a"
}

fn uuid_session_b() -> &'static str {
    "01b00000-0000-7000-8000-00000000000b"
}

#[tokio::test]
async fn usage_codex_subagents_pretty_empty_and_nonempty() {
    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    build_db(&data_dir).await;

    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    // Non-empty: the panel renders with the session rows.
    let output = Command::cargo_bin("otelite")
        .expect("otelite binary should build")
        .args(["usage", "--codex-subagents", "--since", "24h"])
        .env("OTELITE_DATA_DIR", &data_dir)
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Codex Sub-agents"), "{stdout}");
    assert!(stdout.contains("01a00000"), "session row: {stdout}");
    assert!(stdout.contains("reviewer"), "role table: {stdout}");

    // Empty database: the panel says so explicitly.
    let empty = tempfile::tempdir().unwrap();
    let empty_dir = empty.path().join("data");
    {
        let storage_config = StorageConfig::default().with_data_dir(empty_dir.to_path_buf());
        let mut storage = SqliteBackend::new(storage_config);
        storage.initialize().await.unwrap();
    }
    let output = Command::cargo_bin("otelite")
        .expect("otelite binary should build")
        .args(["usage", "--codex-subagents", "--since", "24h"])
        .env("OTELITE_DATA_DIR", &empty_dir)
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("no sub-agent thread starts in range"),
        "{stdout}"
    );
}
