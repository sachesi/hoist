# hoist

`hoist` is a minimal Linux CLI that applies temporary performance tweaks to one launched command and restores previous state when it exits.

## Features

- No daemon/service.
- Frontend binary: `/usr/bin/hoist`.
- CPU profile switching uses direct `tuned-adm` calls (no pkexec helper).
- GPU level switching uses privileged helper `/usr/libexec/hoist-gpuctl` via `pkexec` + polkit.
- Process renice and optional descendant renice scanning via `/proc`.
- Non-root hooks from selected TOML config (argv and inline shell snippets).
- Best-effort restore on normal exit and on SIGINT/SIGTERM.

## Privilege model

- `hoist` itself runs unprivileged.
- CPU profile changes rely on `tuned-adm` behavior on the host.
- AMDGPU level changes are performed only through `/usr/libexec/hoist-gpuctl`, invoked by `pkexec`.
- The helper requires root (`EUID=0`) and only accepts structured CLI arguments.
- Policy and group scoping are shipped in:
  - `/usr/share/polkit-1/actions/io.github.hoist.policy`
  - `/etc/polkit-1/rules.d/50-hoist.rules`
  - `/usr/lib/sysusers.d/hoist.conf`
  - `/etc/security/limits.d/10-hoist.conf`

## Build

```bash
cargo build --release
```

Binaries:
- `target/release/hoist`
- `target/release/hoist-gpuctl`

## Usage

```bash
hoist [--config PATH] [--profile NAME] <command> [args...]
hoist validate-config [--config PATH]
hoist print-config-paths
hoist inspect
hoist helper-info
```

## Steam launch options (native + Flatpak Steam)

`hoist` can be used as a Steam launch option wrapper:

```bash
hoist -- %command%
```

For GameMode-style launch options with environment prefixes:

```bash
MANGOHUD=1 DXVK_CONFIG_FILE="$HOME/.dxvk/dxvk.conf" OBS_VKCAPTURE=1 hoist -- %command%
```

This works in native Steam and keeps `hoist` CLI usage unchanged. For Flatpak Steam, wrapper/process lifetime behavior can differ, so `hoist` waits for the launched process group to fully exit before restoring tweaks.

## Config selection behavior

1. `--config PATH` uses only `PATH`.
2. Else if `~/.config/hoist/default.toml` exists, it fully replaces `/etc/hoist/default.toml`.
3. Else `/etc/hoist/default.toml` is used.

No merge is performed.

Packaged default config path: `/etc/hoist/default.toml`.
User override path: `~/.config/hoist/default.toml`.

## Shell completions

When installed from the RPM package, shell completions are installed for bash, fish, and zsh.
