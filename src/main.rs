mod types;
mod daemon;
mod query;

use clap::Parser;
use color_print::*;

#[derive(clap::Parser)]
#[command(version, about, long_about = None, args_conflicts_with_subcommands = true, disable_help_subcommand = true, flatten_help = true)]
struct Cli {
	#[command(subcommand)]
	command: Option<Command>,

	#[clap(flatten)]
	args: query::Args,
}

#[derive(clap::Subcommand, Clone)]
enum Command {
	#[command(hide = true)]
	Query(query::Args),
	Daemon(daemon::Args),
}

#[tokio::main]
async fn main() {
	let cli = Cli::parse();
	let command = cli.command.unwrap_or_else(|| Command::Query(cli.args));

	let result = match command {
		Command::Query(args) => query::run(args).await,
		Command::Daemon(args) => daemon::run(args).await,
	};

	if let Err(err) = result {
		ceprintln!("<r!><s>Error:</></> {}", err);
		std::process::exit(1); // general error
	}
}
