use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use edgar_13d::db::{last_run, lookup_filings, open, open_work};
use edgar_13d::http::{validate_sleep, LiveFetcher, DEFAULT_SLEEP_SECS};
use edgar_13d::ingest::{ingest_dates, ingest_day};
use edgar_13d::sec_ua::validate_user_agent;

#[derive(Parser)]
#[command(
    name = "edgar-13d",
    about = "Nightly EDGAR Schedule 13D / 13G labels. Labels, not leads.",
    version
)]
struct Cli {
    #[arg(
        long,
        global = true,
        default_value = "data/edgar-13d.sqlite",
        env = "EDGAR_13D_SQLITE"
    )]
    db: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fetch master index day(s) and upsert 13D / 13G accessions
    Ingest {
        /// UTC calendar day (default: yesterday UTC). Conflicts with --from/--to.
        #[arg(long)]
        date: Option<String>,
        /// Inclusive UTC start day. Requires --to.
        #[arg(long, conflicts_with = "date", requires = "to")]
        from: Option<String>,
        /// Inclusive UTC end day. Requires --from.
        #[arg(long, conflicts_with = "date", requires = "from")]
        to: Option<String>,
        #[arg(long, env = "EDGAR_SLEEP_SECS", default_value_t = DEFAULT_SLEEP_SECS)]
        sleep: f64,
        #[arg(long, env = "SEC_USER_AGENT")]
        sec_user_agent: Option<String>,
    },
    /// Last ingest_runs row
    Status,
    /// Filings by issuer CIK, filer CIK, or accession
    Lookup { query: String },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    match run() {
        Ok(code) => code,
        Err(err) => {
            tracing::error!("{err:#}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Ingest {
            date,
            from,
            to,
            sleep,
            sec_user_agent,
        } => {
            validate_sleep(sleep)?;
            let ua = validate_user_agent(sec_user_agent.as_deref().unwrap_or(""))?;
            let days = ingest_dates(date.as_deref(), from.as_deref(), to.as_deref())?;
            let mut db = open_work(&cli.db)?;
            let mut fetcher = LiveFetcher::new(&ua, sleep)?;
            let mut all_ok = true;
            for day in days {
                tracing::info!(date = %day, "ingest day");
                let started = std::time::Instant::now();
                let stats = ingest_day(&mut db, day, &mut fetcher)?;
                tracing::info!(
                    date = %day,
                    status = %stats.status,
                    seen = stats.filings_seen,
                    upserted = stats.filings_upserted,
                    failed = stats.filings_failed,
                    txt_ok = stats.txt_ok,
                    duration_ms = started.elapsed().as_millis() as u64,
                    "ingest finished"
                );
                if stats.status != "ok" {
                    all_ok = false;
                }
            }
            if all_ok {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::from(1))
            }
        }
        Command::Status => {
            let conn = open(&cli.db)?;
            match last_run(&conn)? {
                None => println!("no ingest_runs"),
                Some(r) => {
                    println!(
                        "as_of_date={} status={} seen={} upserted={} failed={} started_at={} finished_at={}",
                        r.as_of_date,
                        r.status,
                        r.filings_seen,
                        r.filings_upserted,
                        r.filings_failed,
                        r.started_at,
                        r.finished_at
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Lookup { query } => {
            let conn = open(&cli.db)?;
            let rows = lookup_filings(&conn, &query)?;
            if rows.is_empty() {
                println!("no filings for {query}");
            } else {
                for f in rows {
                    println!(
                        "{} {} {} issuer={} filer={} {} shares={} percent={}",
                        f.filed_date,
                        f.accession,
                        f.form,
                        f.issuer_cik,
                        f.filer_cik,
                        f.filer_name,
                        f.aggregate_shares.as_deref().unwrap_or("-"),
                        f.percent_of_class.as_deref().unwrap_or("-"),
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
