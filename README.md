# hoist

`hoist` is a Linux CLI wrapper that applies temporary system tuning to one launched command and restores state when that command exits.

## What hoist does

- Runs one target command and waits for it to finish.
- Applies optional CPU tuning via `tuned-adm` while the target is running.
- Applies optional AMDGPU performance-level tuning via `hoist-gpuctl` (`pkexec`/polkit) while the target is running.
- Optionally adjusts process priority (`nice`) and descendant priorities.
- Executes optional start/stop hooks from config.
- Performs best-effort restore on normal exit and on SIGINT/SIGTERM.

## Privilege model

- `hoist` itself runs unprivileged.
- CPU profile changes use `tuned-adm` on the host.
- GPU changes are performed only through `/usr/libexec/hoist-gpuctl` via `pkexec`.
- Policy and access scoping are shipped with the package (`polkit`, sysusers, limits files).

## Build

```bash
cargo build --release
```

Produced binaries:

- `target/release/hoist`
- `target/release/hoist-gpuctl`

## Command usage

```bash
hoist [--config PATH] [--profile NAME] <command> [args...]
hoist validate-config [--config PATH]
hoist print-config-paths
hoist inspect
hoist helper-info
```

## Config behavior

Config file selection:

1. `--config PATH` uses only `PATH`.
2. Else if `~/.config/hoist/default.toml` exists, it fully replaces `/etc/hoist/default.toml`.
3. Else `/etc/hoist/default.toml` is used.

No merge is performed.

Paths:

- System config: `/etc/hoist/default.toml`
- User override: `~/.config/hoist/default.toml`

### CPU/GPU enable toggles

Per-profile CPU and GPU blocks support `enabled`:

- `cpu.enabled`: defaults to `true` when omitted.
- `gpu.enabled`: defaults to `false` when omitted.

When GPU tuning is enabled, hoist prints a warning indicating it should be used with care.

## Steam launch option usage

Use hoist as the launch wrapper command:

```bash
hoist -- %command%
```

Environment prefixes are also supported by Steam shell launch options, for example:

```bash
VAR1=value1 VAR2=value2 hoist -- %command%
```

## Flatpak Steam notes and limitations

- The public launch command remains the same:

  ```bash
  hoist -- %command%
  ```

- In some Flatpak Steam setups, `hoist` may not be resolvable inside the sandbox by default (`/bin/sh: hoist: command not found`).
- Current status: this is a known limitation unless sandbox PATH/host binary visibility is configured in the Flatpak environment.
- hoist does not introduce an alternate public command name for Flatpak.

## Lifecycle behavior

`hoist` waits for the direct child process and then also waits for remaining members of the launched process group (bounded wait) before restore. This improves behavior for wrapper-style launch chains.
