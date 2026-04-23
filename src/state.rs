use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeState {
    pub selected_profile: String,
    pub previous_tuned_profile: Option<String>,
    pub previous_amdgpu_level: Option<String>,
    pub cpu_applied: bool,
    pub gpu_applied: bool,
    pub start_hooks_ran: bool,
    pub child_pid: Option<i32>,
    pub child_pgid: Option<i32>,
}

impl RuntimeState {
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string(self)?;
        write_secure(path, &raw)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let raw =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Ok(toml::from_str(&raw)?)
    }
}

pub fn runtime_dir(uid: u32) -> PathBuf {
    let preferred_parent = Path::new("/run/user").join(uid.to_string());
    if preferred_parent.is_dir() {
        return preferred_parent.join("hoist");
    }
    std::env::temp_dir().join(format!("hoist-{uid}"))
}

static NEXT_NONCE: AtomicU64 = AtomicU64::new(1);

fn write_secure(path: &Path, content: &str) -> Result<()> {
    let Some(parent) = path.parent() else {
        bail!("state path has no parent: {}", path.display());
    };
    let tmp_name = format!(
        ".tmp-{}-{}",
        std::process::id(),
        NEXT_NONCE.fetch_add(1, Ordering::Relaxed)
    );
    let tmp = parent.join(tmp_name);
    let mut f = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    use std::io::Write as _;
    f.write_all(content.as_bytes())
        .with_context(|| format!("writing {}", tmp.display()))?;
    f.sync_all()
        .with_context(|| format!("syncing {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))?;
    Ok(())
}

/// Returns the paths of orphaned state files: those whose recorded child PID is no longer alive.
pub fn find_orphaned_state_files(uid: u32) -> Vec<PathBuf> {
    let dir = runtime_dir(uid);
    let Ok(entries) = fs::read_dir(&dir) else {
        return vec![];
    };
    let mut orphans = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Ok(state) = RuntimeState::load(&path) else {
            continue;
        };
        let Some(child_pid) = state.child_pid else {
            orphans.push(path);
            continue;
        };
        if !pid_is_alive(child_pid) {
            orphans.push(path);
        }
    }
    orphans
}

fn pid_is_alive(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

pub fn create_state_file(uid: u32) -> Result<PathBuf> {
    let dir = runtime_dir(uid);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).ok();

    for _ in 0..32 {
        let nonce = NEXT_NONCE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let name = format!("state-{}-{}-{nonce}.toml", std::process::id(), nanos);
        let path = dir.join(name);
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path);
        match file {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("creating {}", path.display())),
        }
    }

    bail!("failed to allocate unique state file in {}", dir.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = RuntimeState {
            selected_profile: "default".into(),
            previous_tuned_profile: Some("balanced".into()),
            ..RuntimeState::default()
        };
        let path = create_state_file(424242).expect("path");
        s.save(&path).expect("save");
        let got = RuntimeState::load(&path).expect("load");
        std::fs::remove_file(&path).ok();
        assert_eq!(got.previous_tuned_profile.as_deref(), Some("balanced"));
    }

    #[test]
    fn create_state_file_is_unique() {
        let a = create_state_file(424242).expect("first");
        let b = create_state_file(424242).expect("second");
        assert_ne!(a, b);
        std::fs::remove_file(a).ok();
        std::fs::remove_file(b).ok();
    }

    #[test]
    fn state_file_permissions_are_owner_only() {
        let path = create_state_file(424242).expect("path");
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        std::fs::remove_file(path).ok();
    }
}
