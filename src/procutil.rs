use anyhow::{Context, Result};
use nix::libc::pid_t;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::process::Command;

pub fn renice_pid(pid: i32, nice: i32) -> Result<()> {
    let status = Command::new("renice")
        .arg("-n")
        .arg(nice.to_string())
        .arg("-p")
        .arg(pid.to_string())
        .status()
        .context("failed to run renice")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("renice exited with status {status}"))
    }
}

pub fn discover_descendants(root_pid: i32) -> Result<BTreeSet<i32>> {
    if root_pid <= 0 {
        return Ok(BTreeSet::new());
    }
    let mut ppid_map: HashMap<i32, Vec<i32>> = HashMap::new();
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Ok(pid) = name.parse::<i32>() else {
            continue;
        };
        if let Ok(ppid) = read_ppid(pid) {
            ppid_map.entry(ppid).or_default().push(pid);
        }
    }

    let mut out = BTreeSet::new();
    let mut stack = vec![root_pid];
    while let Some(parent) = stack.pop() {
        if let Some(children) = ppid_map.get(&parent) {
            for child in children {
                if out.insert(*child) {
                    stack.push(*child);
                }
            }
        }
    }
    Ok(out)
}

fn read_ppid(pid: i32) -> Result<i32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let after = stat
        .rsplit_once(") ")
        .map(|(_, rest)| rest)
        .context("malformed /proc stat")?;
    let mut fields = after.split_whitespace();
    let _state = fields.next();
    let ppid = fields.next().context("missing ppid")?.parse::<i32>()?;
    Ok(ppid)
}

pub fn kill_process_group(pgid: i32, sig: nix::sys::signal::Signal) -> Result<()> {
    let neg: pid_t = -pgid;
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(neg), sig)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendants_of_invalid_pid_is_empty() {
        let set = discover_descendants(999_999).expect("scan proc");
        assert!(set.is_empty());
    }
}
