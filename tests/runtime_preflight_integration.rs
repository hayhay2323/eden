use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn compiled_bin(env_var: &str, fallback_name: &str) -> PathBuf {
    if let Some(path) = std::env::var_os(env_var) {
        return PathBuf::from(path);
    }

    let exe = std::env::current_exe().expect("current_exe");
    let debug_dir = exe
        .parent()
        .and_then(|parent| parent.parent())
        .expect("integration test binary should live in target/{profile}/deps");
    let candidate = debug_dir.join(if cfg!(windows) {
        format!("{fallback_name}.exe")
    } else {
        fallback_name.to_string()
    });
    assert!(
        candidate.exists(),
        "expected compiled binary at {}",
        candidate.display()
    );
    candidate
}

fn eden_bin() -> PathBuf {
    compiled_bin("CARGO_BIN_EXE_eden", "eden")
}

fn eden_api_bin() -> PathBuf {
    compiled_bin("CARGO_BIN_EXE_eden-api", "eden-api")
}

fn run_eden_with_missing_longport(args: &[&str]) -> std::process::Output {
    run_binary_in_empty_workdir(eden_bin(), args, &[])
}

fn run_binary_in_empty_workdir(
    binary: impl AsRef<Path>,
    args: &[&str],
    envs: &[(&str, &str)],
) -> std::process::Output {
    let workdir = std::env::temp_dir().join(format!(
        "eden-runtime-preflight-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workdir).expect("create temp workdir");
    let mut command = Command::new(binary.as_ref());
    let output = command
        .args(args)
        .env_clear()
        .envs(envs.iter().copied())
        .current_dir(&workdir)
        .output()
        .expect("spawn binary");
    let _ = std::fs::remove_dir_all(&workdir);
    output
}

#[test]
fn hk_live_missing_longport_env_exits_before_runtime_connect() {
    let output = run_eden_with_missing_longport(&[]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Live runtime failed to load Longport config from env"));
    assert!(stderr.contains("LONGPORT_APP_KEY"));
}

#[test]
fn us_live_missing_longport_env_exits_before_runtime_connect() {
    let output = run_eden_with_missing_longport(&["us"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("US runtime failed:"));
    assert!(stderr.contains("LONGPORT_APP_KEY"));
}

#[test]
fn eden_api_serve_missing_master_key_fails_before_bind() {
    let output = run_binary_in_empty_workdir(eden_api_bin(), &["serve"], &[]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("EDEN_API_MASTER_KEY is not set"));
}

#[test]
fn eden_api_serve_invalid_bind_fails_in_cli_preflight() {
    let output = run_binary_in_empty_workdir(
        eden_api_bin(),
        &["serve"],
        &[("EDEN_API_BIND", "not-a-socket")],
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid bind address `not-a-socket`"));
}
