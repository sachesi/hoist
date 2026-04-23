Name:           hoist
Version:        0.2.5
Release:        1%{?dist}
Summary:        Minimal per-command Linux performance tweak wrapper

License:        MIT
URL:            https://github.com/sachesi/hoist
Source0:        %{url}/archive/refs/tags/v%{version}.tar.gz#/%{name}-%{version}.tar.gz
Source1:        %{name}-%{version}-vendor.tar.zst

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  systemd-rpm-macros
Requires:       polkit
Requires:       tuned

%description
hoist launches one command with temporary performance tweaks, then restores previous state.

%prep
%autosetup -n %{name}-%{version}
tar -xaf %{SOURCE1}

mkdir -p .cargo
cat > .cargo/config.toml <<'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF

%build
export CARGO_HOME=$PWD/.cargo-home
cargo build --release --frozen --offline --bins

%install
install -Dpm 0755 target/release/hoist %{buildroot}%{_bindir}/hoist
install -Dpm 0755 target/release/hoist-gpuctl %{buildroot}%{_libexecdir}/hoist-gpuctl

install -Dpm 0644 packaging/usr/share/bash-completion/completions/hoist \
    %{buildroot}%{_datadir}/bash-completion/completions/hoist
install -Dpm 0644 packaging/usr/share/fish/vendor_completions.d/hoist.fish \
    %{buildroot}%{_datadir}/fish/vendor_completions.d/hoist.fish
install -Dpm 0644 packaging/usr/share/zsh/site-functions/_hoist \
    %{buildroot}%{_datadir}/zsh/site-functions/_hoist

install -Dpm 0644 packaging/etc/hoist/default.toml \
    %{buildroot}%{_sysconfdir}/hoist/default.toml
install -Dpm 0644 packaging/etc/security/limits.d/10-hoist.conf \
    %{buildroot}%{_sysconfdir}/security/limits.d/10-hoist.conf

install -Dpm 0644 packaging/usr/lib/sysusers.d/hoist.conf \
    %{buildroot}%{_prefix}/lib/sysusers.d/hoist.conf

install -Dpm 0644 packaging/usr/share/polkit-1/actions/io.github.hoist.policy \
    %{buildroot}%{_datadir}/polkit-1/actions/io.github.hoist.policy
install -Dpm 0644 packaging/etc/polkit-1/rules.d/50-hoist.rules \
    %{buildroot}%{_sysconfdir}/polkit-1/rules.d/50-hoist.rules

%pre
%sysusers_create_compat %{_sysusersdir}/hoist.conf

%files
%license LICENSE*
%doc README.md

%{_bindir}/hoist
%{_libexecdir}/hoist-gpuctl

%config(noreplace) %{_sysconfdir}/hoist/default.toml
%config(noreplace) %{_sysconfdir}/security/limits.d/10-hoist.conf
%config(noreplace) %{_sysconfdir}/polkit-1/rules.d/50-hoist.rules

%{_prefix}/lib/sysusers.d/hoist.conf
%{_datadir}/polkit-1/actions/io.github.hoist.policy

%{_datadir}/bash-completion/completions/hoist
%{_datadir}/fish/vendor_completions.d/hoist.fish
%{_datadir}/zsh/site-functions/_hoist

%changelog
* Wed Apr 23 2026 hoist maintainers
- 0.2.5
- add configurable helper_timeout_secs to [global] config (default 8s)
- add structured logging via tracing crate; respects log_level config and RUST_LOG env var
- add integration test suite (tests/integration.rs)
- add hoist cleanup subcommand to remove orphaned state files

* Thu Apr 23 2026 hoist maintainers
- 0.2.4
- add per-profile CPU/GPU enabled toggles (CPU default enabled, GPU default disabled)
- improve wrapper lifecycle handling by waiting for launched process-group members before restore
- refresh README for hoist-focused usage and Flatpak limitation notes
