mod app;
use anyhow::{Result, bail};
use clap::Parser;
use std::path::PathBuf;
use terminal_showcase::{
    headless,
    presentation::{self, Demo, View},
};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Independent terminal capability explorer and visual smoke-test showcase"
)]
pub struct Args {
    /// Start at overview, colors, unicode, typography, graphics, control, input or integration.
    #[arg(long, default_value = "overview")]
    demo: String,
    /// Freeze a reproducible frame; S toggles this interactively.
    #[arg(long)]
    screenshot: bool,
    /// Query the live stdin/stdout endpoint and run objective cursor exercises, then exit.
    #[arg(long, conflicts_with = "headless")]
    probe: bool,
    /// Exit nonzero after exporting when a live objective check fails. Silence stays Unknown.
    #[arg(long, requires = "probe")]
    check: bool,
    /// Emit a machine-readable report (in probe/headless mode).
    #[arg(long)]
    json: bool,
    /// Run deterministic independent synthetic protocol regressions without a TTY.
    #[arg(long)]
    headless: bool,
    /// Supply a bounded binary terminal response fixture in headless mode.
    #[arg(long, requires = "headless")]
    responses: Option<PathBuf>,
    /// Export live protocol and visual verdicts here on E and on exit.
    #[arg(long)]
    report: Option<PathBuf>,
    /// List available demonstrations without a TTY.
    #[arg(long)]
    list: bool,
    /// Animation frames per second; graphics update at most four times per second.
    #[arg(long,default_value_t=24,value_parser=clap::value_parser!(u16).range(1..=60))]
    fps: u16,
    /// Stop after this many seconds (useful for automated live smoke runs).
    #[arg(long)]
    duration: Option<f64>,
    /// Headless fixture viewport width.
    #[arg(long,default_value_t=120,value_parser=clap::value_parser!(u16).range(20..=500))]
    columns: u16,
    /// Headless fixture viewport height.
    #[arg(long,default_value_t=36,value_parser=clap::value_parser!(u16).range(10..=200))]
    rows: u16,
}
fn run() -> Result<()> {
    let args = Args::parse();
    let demo = Demo::parse(&args.demo)
        .ok_or_else(|| anyhow::anyhow!("unknown demo {:?}; use --list", args.demo))?;
    if let Some(seconds) = args.duration {
        if !seconds.is_finite() || seconds <= 0.0 || seconds > 86400.0 {
            bail!("--duration must be finite and between 0 and 86400 seconds");
        }
    }
    if args.list {
        for demo in Demo::ALL {
            println!("{:<14} {}", demo.id(), demo.title());
        }
        return Ok(());
    }
    if args.headless {
        let responses = if let Some(path) = &args.responses {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(path)?
                .take(1_048_577)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 1_048_576 {
                bail!("response fixture exceeds 1 MiB");
            }
            Some(bytes)
        } else {
            None
        };
        let result = headless::run(args.columns, args.rows, responses.as_deref());
        if args.json {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else if args.screenshot {
            let frame = presentation::render(&result.report, &View::new(demo, true));
            for row in frame {
                print!("{row}\x1b[0m\r\n");
            }
        } else {
            for check in &result.checks {
                println!(
                    "{}  {}",
                    if check.passed { "PASS" } else { "FAIL" },
                    check.name
                );
            }
        }
        if result.checks.iter().any(|c| !c.passed) {
            bail!("headless regression failed");
        }
        return Ok(());
    }
    if args.json && !args.probe {
        bail!("--json requires --probe or --headless; use --report for interactive export");
    }
    app::run(args, demo)
}
fn main() {
    if let Err(error) = run() {
        eprintln!("terminal-showcase: {error:#}");
        std::process::exit(1);
    }
}
