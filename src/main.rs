mod cli;
mod parser;
mod licenses;
mod reporter;
mod table;

use clap::Parser;
use cli::Cli;
use parser::parse_dependencies;
use reporter::generate_report;
use ratatui;
use color_eyre;
use table::App;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    let analyzed_data = parse_dependencies(&args.path);
    if args.gui {
        color_eyre::install()?;
        let terminal = ratatui::init();
        let app_result = App::new(analyzed_data).run(terminal);
        ratatui::restore();
        Ok(app_result?)
    } else {
        generate_report(analyzed_data, args.json, args.verbose, args.strict);
        Ok(())
    }
}
