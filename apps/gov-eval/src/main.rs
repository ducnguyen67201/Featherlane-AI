use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use governance_domain::{EvidenceBundle, PolicyPack, RunVerdict};
use governance_evaluator::evaluate_pack;

#[derive(Debug, Parser)]
#[command(name = "gov-eval", about = "Featherlane governance evaluation CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    ListPolicies {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        api_url: String,
    },
    Evaluate {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        api_url: String,
        #[arg(long)]
        policy_pack_id: String,
        #[arg(long)]
        evidence: PathBuf,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
        #[arg(long)]
        fail_on_inconclusive: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Json,
    Junit,
    Html,
}

#[tokio::main]
async fn main() -> ExitCode {
    match execute(Cli::parse()).await {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(io::stderr(), "error: {error:#}");
            ExitCode::from(2)
        }
    }
}

async fn execute(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::ListPolicies { api_url } => {
            let policies = reqwest::get(format!("{api_url}/v1/policy-packs"))
                .await?
                .error_for_status()?
                .text()
                .await?;
            writeln!(io::stdout(), "{policies}")?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Evaluate {
            api_url,
            policy_pack_id,
            evidence,
            format,
            fail_on_inconclusive,
        } => {
            let pack: PolicyPack =
                reqwest::get(format!("{api_url}/v1/policy-packs/{policy_pack_id}"))
                    .await?
                    .error_for_status()?
                    .json()
                    .await
                    .context("API response was not a persisted policy pack")?;
            pack.ensure_publishable()
                .context("database policy pack is not approved")?;
            let evidence_input = fs::read_to_string(&evidence)
                .with_context(|| format!("failed to read {}", evidence.display()))?;
            let bundle: EvidenceBundle = serde_json::from_str(&evidence_input)
                .context("evidence JSON does not match schema version 1.0")?;
            let summary = evaluate_pack(bundle.eval_run_id, &pack.rules, &bundle);
            let rendered = match format {
                OutputFormat::Json => governance_reports::render_json(&summary)?,
                OutputFormat::Junit => governance_reports::render_junit(&summary),
                OutputFormat::Html => governance_reports::render_html(&summary),
            };
            writeln!(io::stdout(), "{rendered}")?;
            Ok(match summary.verdict {
                RunVerdict::Fail => ExitCode::from(1),
                RunVerdict::Inconclusive if fail_on_inconclusive => ExitCode::from(1),
                RunVerdict::Pass | RunVerdict::Inconclusive => ExitCode::SUCCESS,
            })
        }
    }
}
