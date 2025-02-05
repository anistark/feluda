mod cli;
mod licenses;
mod parser;
mod reporter;
mod table;

use clap::Parser;
use cli::{clear_last_line, Cli};
use parser::parse_dependencies;
use reporter::generate_report;
use spinners::{Spinner, Spinners};
use std::error::Error;
use table::App;

fn main() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    let mut sp = Spinner::new(Spinners::Dots10, "🔎".into());
    let analyzed_data = parse_dependencies(&args.path);
    sp.stop();
    clear_last_line();
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
