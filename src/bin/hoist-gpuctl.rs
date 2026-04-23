use clap::Parser;
use hoist::helper::{GpuCtlCli, run_gpuctl};

fn main() {
    let cli = GpuCtlCli::parse();
    if let Err(e) = run_gpuctl(cli.command) {
        eprintln!("hoist-gpuctl: {e:#}");
        std::process::exit(1);
    }
}
