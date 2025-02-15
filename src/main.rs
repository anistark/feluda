mod cli;
mod config;
mod licenses;
mod parser;
mod reporter;
mod table;

use clap::Parser;
use cli::Cli;
use parser::parse_root;
use reporter::generate_report;
use std::error::Error;
use table::App;

fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    if args.debug {
        cli::set_debug_mode(true);
    }
    let analyzed_data = parse_root(&args.path, args.language.as_deref());
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
