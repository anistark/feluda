mod cli;
mod parser;
mod licenses;
mod reporter;

use clap::Parser;
use cli::Cli;
use parser::parse_dependencies;
use reporter::generate_report;

fn main() {
    let args = Cli::parse();
    let analyzed_data = parse_dependencies(&args.path);
    generate_report(analyzed_data, args.json, args.verbose, args.strict);
}
