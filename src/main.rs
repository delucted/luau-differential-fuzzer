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
}

fn main() -> Result<()> {
    let args = Args::parse();

    // println!("fuel: {}; release: {}", args.fuel, args.release);
    //
    // let runner = Runner::for_release(&args.release)?.with_o0()?;
    //
    // let result = runner.run("print('hello')");
    //
    // for c in result.unwrap().stdout {
    //     print!("{}", c as char)
    // }

    let runner_o0 = Runner::for_release(&args.release)?.with_o0()?;
    let runner_o2 = Runner::for_release(&args.release)?.with_o2()?;
    let mut ast_gen = AstGenerator::new(args.fuel);

    let mut checked = 0;
    let mut discrepancies = 0;
    loop {
        print!("\x1B[2J\x1B[H");
        println!("Luau Differential Fuzzer | {} checked | {} discrepancies", checked, discrepancies);
        let sample = ast_gen.gen_ast().print();
        let a = &runner_o0.run(&sample)?;
        let b = &runner_o2.run(&sample)?;
        if let Some(diffs) = Comparator::compare(a, b) {
            discrepancies += 1;
            Report::report(&sample, a, b, diffs)?;
        }
        checked += 1;
    }

    Ok(())
}
