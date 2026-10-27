//! `pacod-bench`: the benchmark's service (`serve`), and the command line that runs a suite
//! without a database or a UI (`run`, `plan`).

use std::time::Duration;

use clap::{Parser, Subcommand};
use pacod_bench::config::Config;
use pacod_bench::{cli, serve};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Parser)]
#[command(name = "pacod-bench", version, about = "Benchmarks pacod-gw, its proxy mode and other gateways")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// The API, the run queue and the database (the default).
    Serve,
    /// Runs a suite on a target and writes its report, without a database or a UI.
    Run(cli::RunArgs),
    /// Prints the cells a suite makes on a kind of target, and the estimate.
    Plan(cli::PlanArgs),
    /// Reads a report file and checks it as pacod-gw does.
    Validate(cli::ValidateArgs),
    /// Asks a running service whether it is healthy (the image's healthcheck).
    Healthcheck {
        #[arg(default_value = "http://127.0.0.1:8190/api/v1/healthz")]
        url: String,
    },
}

fn main() -> std::process::ExitCode {
    let args = Args::parse();
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("pacod-bench: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let result = match args.command.unwrap_or(Command::Serve) {
        Command::Serve => {
            init_log("bench-api", false);
            runtime.block_on(async {
                let env = |name: &str| std::env::var(name).ok();
                serve(Config::from_env(&env)?, signal()).await
            })
        }
        // The command line's own lines go to stderr; the log goes there too.
        Command::Run(a) => {
            init_log("pacod-bench", true);
            runtime.block_on(cli::run(a))
        }
        Command::Plan(a) => cli::plan(a),
        Command::Validate(a) => cli::validate(a),
        Command::Healthcheck { url } => runtime.block_on(healthcheck(&url)),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = format!("{e:#}"), "pacod-bench stopped");
            eprintln!("pacod-bench: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// The log of the pacod logging convention: RUST_LOG sets the level, BENCH_LOG_FORMAT the format
/// (json or text), SERVICE_NAME names the service.
fn init_log(service: &str, to_stderr: bool) {
    let settings = pacod_gw_logging::Settings::from_env(service, "info,sqlx=warn", "BENCH_LOG_FORMAT");
    if to_stderr {
        pacod_gw_logging::install(settings, std::io::stderr);
    } else {
        pacod_gw_logging::install(settings, std::io::stdout);
    }
}

async fn healthcheck(url: &str) -> anyhow::Result<()> {
    let r = reqwest::Client::new().get(url).timeout(Duration::from_secs(3)).send().await?;
    anyhow::ensure!(r.status().is_success(), "{url} answered {}", r.status());
    Ok(())
}

async fn signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
}
