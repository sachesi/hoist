use anyhow::{Context, Result, bail};
use clap::Parser;
use hoist::cli::{Cli, HoistSubcommand};
use hoist::config::{load_config, resolve_config_path};
use hoist::runtime::{helper_exists, helper_path, inspect_group_membership, run_command};
use hoist::system::{read_gpu_level, tuned_active};

fn main() {
    if let Err(e) = real_main() {
        eprintln!("hoist: {e:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.subcommand {
        Some(HoistSubcommand::ValidateConfig { config }) => {
            let (paths, _cfg) = load_config(config.as_deref())?;
            println!("hoist: config is valid: {}", paths.selected.display());
            return Ok(());
        }
        Some(HoistSubcommand::PrintConfigPaths) => {
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
            inspect(cli.config.as_deref(), cli.profile.as_deref())?;
            return Ok(());
        }
        Some(HoistSubcommand::HelperInfo) => {
            println!("gpuctl_path={}", helper_path());
            println!("gpuctl_exists={}", helper_exists());
            return Ok(());
        }
        None => {}
    }

    if cli.command.is_empty() {
        bail!("missing command. usage: hoist [--config PATH] [--profile NAME] <command> [args...]");
    }

    let (paths, cfg) = load_config(cli.config.as_deref())?;
    eprintln!("hoist: selected config {}", paths.selected.display());
    let profile = cli
        .profile
        .as_deref()
        .unwrap_or(cfg.global.default_profile.as_str());

    let code = run_command(&cfg, profile, &cli.command, &cli.env)?;
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
