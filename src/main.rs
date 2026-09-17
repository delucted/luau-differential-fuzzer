pub mod code_gen;
pub mod runner;
pub mod comparator;
pub mod report;
pub mod util;
mod reducer;

use clap::Parser;
use anyhow::Result;
use crate::code_gen::ast_gen::AstGenerator;

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

    let mut ast_gen = AstGenerator::new(args.fuel);

    println!("{:?}", ast_gen.gen_ast());

    Ok(())
}
