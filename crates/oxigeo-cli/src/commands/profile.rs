//! Performance profiling command

use anyhow::{Context, Result};
use clap::Args;

use crate::util::profiler::{Operation, Profiler};

/// Profile a geospatial operation
#[derive(Args, Debug)]
pub struct ProfileArgs {
    /// Input file path
    #[arg(value_name = "INPUT")]
    pub input: String,

    /// Operation to profile (open, read-features, read-bands, stats)
    #[arg(value_name = "OPERATION", default_value = "open")]
    pub operation: String,

    /// Number of iterations
    #[arg(long, short = 'n', default_value = "10")]
    pub iterations: usize,

    /// Export results to JSON file
    #[arg(long, short = 'o')]
    pub output: Option<String>,

    /// Print results as JSON to stdout instead of a text table
    #[arg(long)]
    pub json: bool,
}

/// Execute profile command
pub fn execute(args: ProfileArgs, output_format: crate::OutputFormat) -> Result<()> {
    let op: Operation = args
        .operation
        .parse()
        .with_context(|| format!("Failed to parse operation '{}'", args.operation))?;

    println!("Profiling operation: {op}");
    println!("Input:      {}", args.input);
    println!("Iterations: {}", args.iterations);
    println!();

    let profiler = run_profiler(&op, &args.input, args.iterations)?;

    let use_json = args.json || matches!(output_format, crate::OutputFormat::Json);
    if use_json {
        let json = profiler
            .export_json()
            .context("Failed to export profiler report as JSON")?;
        println!("{json}");
    } else {
        print!("{}", profiler.report());
    }

    if let Some(ref output_path) = args.output {
        let json = profiler
            .export_json()
            .context("Failed to serialise profiler report for file export")?;
        std::fs::write(output_path, &json)
            .with_context(|| format!("Failed to write profiler results to '{output_path}'"))?;
        println!("Results exported to: {output_path}");
    }

    Ok(())
}

/// Run the operation `iterations` times, recording wall-clock durations.
fn run_profiler(op: &Operation, input: &str, iterations: usize) -> Result<Profiler> {
    let mut profiler = Profiler::new(format!("{op}@{input}"));
    for i in 0..iterations {
        profiler.start();
        op.execute(input)
            .with_context(|| format!("Iteration {i} of operation '{op}' failed"))?;
        profiler.stop();

        if i > 0 && i % 10 == 0 {
            println!("  completed {i} / {iterations} iterations …");
        }
    }
    Ok(profiler)
}
