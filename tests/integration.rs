use hoist::config::{Config, Global, Profile};
use hoist::runtime::run_command;
use std::collections::BTreeMap;

fn no_tuning_config() -> Config {
    let mut profiles = BTreeMap::new();
    profiles.insert(
        "default".to_string(),
        Profile {
            cpu: None,
            gpu: None,
            process: None,
            hooks: None,
        },
    );
    Config {
        global: Global {
            poll_interval_ms: Some(20),
            require_all: false,
            log_level: None,
            default_profile: "default".to_string(),
            helper_timeout_secs: None,
        },
        profile: profiles,
    }
}

#[test]
fn exit_zero_is_forwarded() {
    let cfg = no_tuning_config();
    let code = run_command(&cfg, "default", &["/bin/true".to_string()]).unwrap();
    assert_eq!(code, 0);
}

#[test]
fn exit_nonzero_is_forwarded() {
    let cfg = no_tuning_config();
    let code = run_command(
        &cfg,
        "default",
        &["/bin/sh".to_string(), "-c".to_string(), "exit 42".to_string()],
    )
    .unwrap();
    assert_eq!(code, 42);
}

#[test]
fn signal_exit_code_is_forwarded() {
    let cfg = no_tuning_config();
    // SIGTERM = 15, exit code should be 128 + 15 = 143
    let code = run_command(
        &cfg,
        "default",
        &[
            "/bin/sh".to_string(),
            "-c".to_string(),
            "kill -TERM $$".to_string(),
        ],
    )
    .unwrap();
    assert_eq!(code, 128 + 15);
}

#[test]
fn missing_command_argv_fails() {
    let cfg = no_tuning_config();
    let err = run_command(&cfg, "default", &[]).unwrap_err();
    assert!(err.to_string().contains("missing command"));
}

#[test]
fn nonexistent_binary_fails() {
    let cfg = no_tuning_config();
    let err = run_command(
        &cfg,
        "default",
        &["/nonexistent/binary-xyzzy".to_string()],
    )
    .unwrap_err();
    assert!(err.to_string().contains("failed to spawn"));
}

#[test]
fn unknown_profile_fails() {
    let cfg = no_tuning_config();
    let err = run_command(&cfg, "nonexistent", &["/bin/true".to_string()]).unwrap_err();
    assert!(err.to_string().contains("unknown profile"));
}

#[test]
fn process_env_is_inherited() {
    let cfg = no_tuning_config();
    // Verify the child can see env vars set in the parent.
    // SAFETY: test runs single-threaded at this point.
    unsafe { std::env::set_var("HOIST_TEST_VAR", "hello") };
    let code = run_command(
        &cfg,
        "default",
        &[
            "/bin/sh".to_string(),
            "-c".to_string(),
            r#"[ "$HOIST_TEST_VAR" = "hello" ]"#.to_string(),
        ],
    )
    .unwrap();
    assert_eq!(code, 0);
}
