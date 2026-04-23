use crate::config::{Config, GPUCTL_PATH, Profile, validate_profile_name};
use crate::procutil::{
    discover_descendants, kill_process_group, process_group_has_members, renice_pid,
};
use crate::state::{RuntimeState, create_state_file};
use crate::system::{
    gpuctl_exists, read_gpu_level, run_argv, run_pkexec_gpuctl, run_shell, tuned_active,
    tuned_set_profile,
};
use anyhow::{Context, Result, bail};
use nix::sys::signal::{SigSet, SigmaskHow, Signal, pthread_sigmask};
use nix::sys::signalfd::{SfdFlags, SignalFd};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{Pid, User, getuid};
use std::collections::BTreeSet;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};

pub fn run_command(cfg: &Config, profile_name: &str, argv: &[String]) -> Result<i32> {
    validate_profile_name(profile_name)?;
    if argv.is_empty() {
        bail!("missing command");
    }
    let profile = cfg
        .profile
        .get(profile_name)
        .with_context(|| format!("unknown profile '{profile_name}'"))?;

    let uid = getuid().as_raw();
    let state_path = create_state_file(uid)?;
    let mut state = RuntimeState {
        selected_profile: profile_name.to_string(),
        ..RuntimeState::default()
    };

    if let Err(e) = apply_start(cfg, profile, &mut state) {
        let _ = restore_all(cfg, profile, &state);
        std::fs::remove_file(&state_path).ok();
        return Err(e).context("startup apply failed");
    }
    state.save(&state_path)?;

    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    // SAFETY: pre_exec runs in child after fork and before exec for deterministic setpgid.
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setpgid(Pid::from_raw(0), Pid::from_raw(0)).map_err(std::io::Error::other)
        });
    }

    let child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn {}", argv[0]))
        .map_err(|e| {
            let _ = restore_all(cfg, profile, &state);
            e
        })?;

    let child_pid = i32::try_from(child.id()).context("child pid out of range")?;
    state.child_pid = Some(child_pid);
    state.child_pgid = Some(child_pid);
    state.save(&state_path)?;

    if let Some(proc_cfg) = &profile.process
        && let Err(e) = renice_pid(child_pid, proc_cfg.nice)
    {
        if cfg.global.require_all {
            restore_all(cfg, profile, &state)?;
            return Err(e).context("failed to set process priority");
        }
        eprintln!("hoist: warning: failed to set nice={}: {e}", proc_cfg.nice);
    }

    let mut sigset = SigSet::empty();
    sigset.add(Signal::SIGINT);
    sigset.add(Signal::SIGTERM);
    pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&sigset), None)?;
    let sfd = SignalFd::with_flags(&sigset, SfdFlags::SFD_NONBLOCK)?;

    let mut reniced: BTreeSet<i32> = BTreeSet::new();
    let mut last_scan = Instant::now();

    let code = loop {
        if let Some(sig) = read_signal(&sfd)? {
            eprintln!("hoist: forwarding signal {sig} to child process group");
            if let Some(pgid) = state.child_pgid {
                kill_process_group(pgid, sig).ok();
            }
        }

        if let Some(proc_cfg) = &profile.process
            && proc_cfg.renice_descendants
            && last_scan.elapsed() >= Duration::from_millis(proc_cfg.renice_interval_ms)
        {
            last_scan = Instant::now();
            if let Ok(desc) = discover_descendants(child_pid) {
                for pid in desc {
                    if reniced.insert(pid)
                        && let Err(e) = renice_pid(pid, proc_cfg.nice)
                        && cfg.global.require_all
                    {
                        restore_all(cfg, profile, &state)?;
                        return Err(e).context("failed to renice descendant");
                    }
                }
            }
        }

        let poll_interval = Duration::from_millis(cfg.global.poll_interval_ms.unwrap_or(700));
        match waitpid(Pid::from_raw(child_pid), Some(WaitPidFlag::WNOHANG))? {
            WaitStatus::StillAlive => std::thread::sleep(poll_interval),
            WaitStatus::Exited(_, code) => break code,
            WaitStatus::Signaled(_, sig, _) => break 128 + sig as i32,
            _ => {}
        }
    };

    if let Some(pgid) = state.child_pgid {
        wait_for_process_group_exit(pgid, cfg.global.poll_interval_ms.unwrap_or(700));
    }

    restore_all(cfg, profile, &state)?;
    std::fs::remove_file(&state_path).ok();
    Ok(code)
}

fn read_signal(sfd: &SignalFd) -> Result<Option<Signal>> {
    match sfd.read_signal()? {
        Some(info) => Ok(Signal::try_from(info.ssi_signo as i32).ok()),
        None => Ok(None),
    }
}

fn apply_start(cfg: &Config, profile: &Profile, state: &mut RuntimeState) -> Result<()> {
    if let Some(cpu) = &profile.cpu {
        if !cpu.enabled() {
            eprintln!("hoist: cpu tweaks disabled by profile");
        } else {
            let prev = tuned_active()?;
            state.previous_tuned_profile = prev;
            eprintln!("hoist: applying tuned profile {}", cpu.enter_profile);
            let result = tuned_set_profile(&cpu.enter_profile);
            if let Err(e) = result {
                if cfg.global.require_all {
                    return Err(e);
                }
                eprintln!("hoist: warning: {e}");
            } else {
                state.cpu_applied = true;
            }
        }
    }

    if let Some(gpu) = &profile.gpu {
        if !gpu.enabled() {
            eprintln!("hoist: gpu tweaks disabled by profile");
        } else {
            eprintln!("hoist: warning: gpu tweaks enabled; use at your own risk");
            if let Ok(prev) = read_gpu_level(gpu.card) {
                state.previous_amdgpu_level = Some(prev.as_str().to_string());
            }
            eprintln!(
                "hoist: setting amdgpu card{} level {}",
                gpu.card,
                gpu.enter_level.as_str()
            );
            let card = gpu.card.to_string();
            let result = run_pkexec_gpuctl(&[
                "apply",
                "--card",
                &card,
                "--level",
                gpu.enter_level.as_str(),
            ]);
            if let Err(e) = result {
                if cfg.global.require_all {
                    return Err(e);
                }
                eprintln!("hoist: warning: {e}");
            } else {
                state.gpu_applied = true;
            }
        }
    }

    if let Some(hooks) = &profile.hooks {
        for cmd in &hooks.start {
            run_argv(cmd)?;
            state.start_hooks_ran = true;
        }
        for cmd in &hooks.start_sh {
            run_shell(cmd)?;
            state.start_hooks_ran = true;
        }
    }

    if let Some(proc_cfg) = &profile.process
        && proc_cfg.nice < -10
    {
        eprintln!(
            "hoist: warning: configured nice={} is below packaged policy (@hoist - nice -15)",
            proc_cfg.nice
        );
    }

    Ok(())
}

fn restore_all(_cfg: &Config, profile: &Profile, state: &RuntimeState) -> Result<()> {
    if let Some(cpu) = &profile.cpu
        && cpu.enabled()
        && state.cpu_applied
    {
        let target = choose_cpu_restore_target(cpu, state);
        if let Some(target_profile) = target {
            eprintln!("hoist: restoring tuned profile {target_profile}");
            if let Err(e) = tuned_set_profile(&target_profile) {
                eprintln!("hoist: warning: cpu restore failed: {e}");
            }
        }
    }

    if let Some(gpu) = &profile.gpu
        && gpu.enabled()
        && state.gpu_applied
    {
        let target = choose_gpu_restore_target(gpu, state);
        if let Some(level) = target {
            eprintln!("hoist: restoring amdgpu level {level}");
            let card = gpu.card.to_string();
            if let Err(e) = run_pkexec_gpuctl(&["revert", "--card", &card, "--level", &level]) {
                eprintln!("hoist: warning: gpu restore failed: {e}");
            }
        }
    }

    if let Some(hooks) = &profile.hooks
        && state.start_hooks_ran
    {
        for cmd in &hooks.stop {
            if let Err(e) = run_argv(cmd) {
                eprintln!("hoist: warning: stop hook failed: {e}");
            }
        }
        for cmd in &hooks.stop_sh {
            if let Err(e) = run_shell(cmd) {
                eprintln!("hoist: warning: stop hook failed: {e}");
            }
        }
    }

    Ok(())
}

fn wait_for_process_group_exit(pgid: i32, poll_interval_ms: u64) {
    let poll_interval = Duration::from_millis(poll_interval_ms);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match process_group_has_members(pgid) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                eprintln!("hoist: warning: failed to inspect process group {pgid}: {e}");
                break;
            }
        }
        if Instant::now() >= deadline {
            eprintln!("hoist: warning: timed out waiting for process group {pgid} to exit");
            break;
        }
        std::thread::sleep(poll_interval);
    }
}

fn choose_cpu_restore_target(
    cpu: &crate::config::CpuConfig,
    state: &RuntimeState,
) -> Option<String> {
    if cpu.restore_previous {
        state
            .previous_tuned_profile
            .clone()
            .or_else(|| cpu.restore_to_profile.clone())
            .or_else(|| cpu.fallback_exit_profile.clone())
    } else {
        cpu.restore_to_profile
            .clone()
            .or_else(|| cpu.fallback_exit_profile.clone())
    }
}

fn choose_gpu_restore_target(
    gpu: &crate::config::GpuConfig,
    state: &RuntimeState,
) -> Option<String> {
    if gpu.restore_previous {
        state
            .previous_amdgpu_level
            .clone()
            .or_else(|| {
                gpu.restore_to_level
                    .as_ref()
                    .map(|l| l.as_str().to_string())
            })
            .or_else(|| {
                gpu.fallback_exit_level
                    .as_ref()
                    .map(|l| l.as_str().to_string())
            })
    } else {
        gpu.restore_to_level
            .as_ref()
            .map(|l| l.as_str().to_string())
            .or_else(|| {
                gpu.fallback_exit_level
                    .as_ref()
                    .map(|l| l.as_str().to_string())
            })
    }
}

pub fn inspect_group_membership() -> bool {
    let uid = getuid();
    let Some(user) = User::from_uid(uid).ok().flatten() else {
        return false;
    };
    if user.gid.as_raw() == 0 {
        return true;
    }
    let Ok(cname) = std::ffi::CString::new(user.name.as_bytes()) else {
        return false;
    };
    nix::unistd::getgrouplist(cname.as_c_str(), user.gid)
        .map(|groups| {
            groups
                .iter()
                .any(|g| g.as_raw() == group_hoist_gid().unwrap_or(u32::MAX))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AmdGpuLevel, Config, CpuConfig, Global, GpuConfig, GpuKind};
    use std::collections::BTreeMap;

    #[test]
    fn cpu_restore_target_prefers_previous_then_restore_to_then_fallback() {
        let cpu = CpuConfig {
            enabled: None,
            enter_profile: "throughput-performance".into(),
            restore_previous: true,
            restore_to_profile: Some("balanced".into()),
            fallback_exit_profile: Some("powersave".into()),
        };
        let mut state = RuntimeState::default();
        state.previous_tuned_profile = Some("latency-performance".into());
        assert_eq!(
            choose_cpu_restore_target(&cpu, &state).as_deref(),
            Some("latency-performance")
        );

        state.previous_tuned_profile = None;
        assert_eq!(
            choose_cpu_restore_target(&cpu, &state).as_deref(),
            Some("balanced")
        );
    }

    #[test]
    fn cpu_restore_target_uses_restore_to_when_restore_previous_disabled() {
        let cpu = CpuConfig {
            enabled: None,
            enter_profile: "throughput-performance".into(),
            restore_previous: false,
            restore_to_profile: Some("balanced".into()),
            fallback_exit_profile: Some("powersave".into()),
        };
        let mut state = RuntimeState::default();
        state.previous_tuned_profile = Some("latency-performance".into());
        assert_eq!(
            choose_cpu_restore_target(&cpu, &state).as_deref(),
            Some("balanced")
        );
    }

    #[test]
    fn gpu_restore_target_uses_restore_to_when_restore_previous_disabled() {
        let gpu = GpuConfig {
            enabled: None,
            kind: GpuKind::Amdgpu,
            card: 1,
            enter_level: AmdGpuLevel::High,
            restore_previous: false,
            restore_to_level: Some(AmdGpuLevel::Auto),
            fallback_exit_level: Some(AmdGpuLevel::Low),
        };
        let mut state = RuntimeState::default();
        state.previous_amdgpu_level = Some("high".into());
        assert_eq!(
            choose_gpu_restore_target(&gpu, &state).as_deref(),
            Some("auto")
        );
    }

    #[test]
    fn cpu_restore_target_uses_fallback_when_restore_previous_disabled_and_restore_to_missing() {
        let cpu = CpuConfig {
            enabled: None,
            enter_profile: "throughput-performance".into(),
            restore_previous: false,
            restore_to_profile: None,
            fallback_exit_profile: Some("balanced".into()),
        };
        let mut state = RuntimeState::default();
        state.previous_tuned_profile = Some("latency-performance".into());
        assert_eq!(
            choose_cpu_restore_target(&cpu, &state).as_deref(),
            Some("balanced")
        );
    }

    #[test]
    fn gpu_restore_target_uses_fallback_when_restore_previous_disabled_and_restore_to_missing() {
        let gpu = GpuConfig {
            enabled: None,
            kind: GpuKind::Amdgpu,
            card: 1,
            enter_level: AmdGpuLevel::High,
            restore_previous: false,
            restore_to_level: None,
            fallback_exit_level: Some(AmdGpuLevel::Auto),
        };
        let mut state = RuntimeState::default();
        state.previous_amdgpu_level = Some("high".into());
        assert_eq!(
            choose_gpu_restore_target(&gpu, &state).as_deref(),
            Some("auto")
        );
    }

    #[test]
    fn wrapper_child_lifecycle_waits_for_process_group_members() {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "default".to_string(),
            crate::config::Profile {
                cpu: None,
                gpu: None,
                process: None,
                hooks: None,
            },
        );
        let cfg = Config {
            global: Global {
                poll_interval_ms: Some(20),
                require_all: false,
                log_level: None,
                default_profile: "default".to_string(),
            },
            profile: profiles,
        };

        let start = Instant::now();
        let code = run_command(
            &cfg,
            "default",
            &[
                "/bin/sh".to_string(),
                "-c".to_string(),
                "sleep 0.35 & exit 0".to_string(),
            ],
        )
        .expect("run command");
        let elapsed = start.elapsed();

        assert_eq!(code, 0);
        assert!(
            elapsed >= Duration::from_millis(300),
            "expected wrapper-aware wait, got {elapsed:?}"
        );
    }
}

fn group_hoist_gid() -> Option<u32> {
    let content = std::fs::read_to_string("/etc/group").ok()?;
    for line in content.lines() {
        let mut parts = line.split(':');
        if parts.next() == Some("hoist") {
            let _passwd = parts.next();
            let gid = parts.next()?.parse::<u32>().ok()?;
            return Some(gid);
        }
    }
    None
}

pub fn helper_exists() -> bool {
    gpuctl_exists()
}

pub fn helper_path() -> &'static str {
    GPUCTL_PATH
}
