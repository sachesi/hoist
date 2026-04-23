use anyhow::{Context, Result, bail};
use clap::Parser;
use hoist::cli::{Cli, HoistSubcommand};
use hoist::config::{load_config, resolve_config_path};
use hoist::runtime::{helper_exists, helper_path, inspect_group_membership, run_command};
use hoist::state::find_orphaned_state_files;
use hoist::system::{read_gpu_level, tuned_active};
use nix::unistd::getuid;
use tracing_subscriber::EnvFilter;

fn init_tracing(log_level: Option<&str>) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level.unwrap_or("info")));
    tracing_subscriber::fmt()
        .without_time()
        .with_target(false)
        .with_env_filter(filter)
        .init();
}

fn main() {
    if let Err(e) = real_main() {
        tracing::error!("{e:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.subcommand {
        Some(HoistSubcommand::ValidateConfig { config }) => {
            init_tracing(None);
            let (paths, _cfg) = load_config(config.as_deref())?;
            println!("hoist: config is valid: {}", paths.selected.display());
            return Ok(());
        }
        Some(HoistSubcommand::PrintConfigPaths) => {
            init_tracing(None);
            let paths = resolve_config_path(cli.config.as_deref())?;
            println!(
                "system_config={} exists={}",
                paths.system.display(),
                paths.system_exists
            );
            println!(
                "user_config={} exists={}",
                paths.user.display(),
                paths.user_exists
            );
            println!("selected_config={}", paths.selected.display());
            return Ok(());
        }
        Some(HoistSubcommand::Inspect) => {
            init_tracing(None);
            inspect(cli.config.as_deref(), cli.profile.as_deref())?;
            return Ok(());
        }
        Some(HoistSubcommand::HelperInfo) => {
            init_tracing(None);
            println!("gpuctl_path={}", helper_path());
            println!("gpuctl_exists={}", helper_exists());
            return Ok(());
        }
        Some(HoistSubcommand::Cleanup) => {
            init_tracing(None);
            let uid = getuid().as_raw();
            let orphans = find_orphaned_state_files(uid);
            if orphans.is_empty() {
                println!("hoist: no orphaned state files found");
            } else {
                for path in &orphans {
                    match std::fs::remove_file(path) {
                        Ok(()) => println!("hoist: removed {}", path.display()),
                        Err(e) => tracing::warn!("failed to remove {}: {e}", path.display()),
                    }
                }
            }
            return Ok(());
        }
        None => {}
    }

    if cli.command.is_empty() {
        bail!("missing command. usage: hoist [--config PATH] [--profile NAME] <command> [args...]");
    }

    let (paths, cfg) = load_config(cli.config.as_deref())?;
    init_tracing(cfg.global.log_level.as_deref());
    tracing::info!("selected config {}", paths.selected.display());
    let profile = cli
        .profile
        .as_deref()
        .unwrap_or(cfg.global.default_profile.as_str());

    let code = run_command(&cfg, profile, &cli.command)?;
    std::process::exit(code)
}

fn inspect(config: Option<&std::path::Path>, profile_cli: Option<&str>) -> Result<()> {
    let (paths, cfg) = load_config(config)?;
    let profile = profile_cli.unwrap_or(cfg.global.default_profile.as_str());
    let p = cfg
        .profile
        .get(profile)
        .with_context(|| format!("unknown profile '{profile}'"))?;

    println!("selected_config={}", paths.selected.display());
    println!("selected_profile={profile}");
    println!("require_all={}", cfg.global.require_all);
    let gpuctl = helper_path();
    println!(
        "gpuctl_path={} exists={}",
        gpuctl,
        std::path::Path::new(gpuctl).exists()
    );
    println!("user_in_hoist_group={}", inspect_group_membership());

    match tuned_active()? {
        Some(v) => println!("tuned_active={v}"),
        None => println!("tuned_active=unavailable"),
    }

    if let Some(gpu) = &p.gpu {
        match read_gpu_level(gpu.card) {
            Ok(level) => println!("amdgpu_card{}_level={}", gpu.card, level.as_str()),
            Err(_) => println!("amdgpu_card{}_level=unavailable", gpu.card),
        }
    }

    Ok(())
}
