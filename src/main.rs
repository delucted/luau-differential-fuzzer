pub mod code_gen;
pub mod runner;
pub mod comparator;
pub mod report;
pub mod util;
mod reducer;

use clap::Parser;
use anyhow::Result;
use crate::code_gen::ast::Printer;
use crate::code_gen::ast_gen::AstGenerator;
use crate::comparator::Comparator;
use crate::report::Report;
use crate::runner::run::Runner;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use std::thread;

/// A Luau Differential Fuzzer that emits potential bugs within the Luau compiler.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Amount of fuel code-gen has. The greater this value, the larger the average code-gen.
    #[arg(long, default_value_t = 500)]
    fuel: u32,

    /// Force installation of a Luau release.
    #[arg(short, long, default_value_t = "0.738".to_string())]
    release: String,

    /// Amount of workers to spawn.
    #[arg(short, long, default_value_t = 8)]
    workers: u8
}

struct Stats {
    checked: AtomicU64,
    discrepancies: AtomicU64,
    stop: AtomicBool,
}

fn worker(id: u64, args: &Args, stats: &Stats) -> Result<()> {
    let runner_o0 = Runner::for_release(&args.release)?.with_o0()?;
    let runner_o2 = Runner::for_release(&args.release)?.with_o2()?;
    let mut ast_gen = AstGenerator::new(args.fuel);

    while !stats.stop.load(Ordering::Relaxed) {
        let sample = ast_gen.gen_ast().print();

        let o0 = runner_o0.run(&sample)?;
        let o2 = runner_o2.run(&sample)?;

        if let Some(diffs) = Comparator::compare(&o0, &o2) {
            stats.discrepancies.fetch_add(1, Ordering::Relaxed);
            Report::report(&sample, &o0, &o2, diffs)?;
        }
        stats.checked.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let stats = Stats {
        checked: AtomicU64::new(0),
        discrepancies: AtomicU64::new(0),
        stop: AtomicBool::new(false),
    };

    thread::scope(|s| {
        let handles: Vec<_> = (0..args.workers)
            .map(|id| {
                let (args, stats) = (&args, &stats);
                s.spawn(move || worker(id as u64, args, stats))
            })
            .collect();

        while !stats.stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(250));
            print!(
                "\r{} checked | {} discrepancies",
                stats.checked.load(Ordering::Relaxed),
                stats.discrepancies.load(Ordering::Relaxed),
            );
            let _ = std::io::stdout();
        }

        for h in handles {
            match h.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => eprintln!("\nworker error: {e}"),
                Err(_) => eprintln!("\nworker panicked"),
            }
        }
    });

    Ok(())
}