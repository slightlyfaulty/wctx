mod service;
mod providers;
mod debouncer;

use anyhow::Result;

#[derive(clap::Args, Clone, Debug)]
pub struct Args {
	/// Specify the window provider instead of auto-detecting
	#[arg(short, long, value_enum)]
	provider: Option<providers::WindowProvider>,
}

pub async fn run(args: Args) -> Result<()> {
	// use a channel to signal when the dbus service is ready and send a proxy client to providers
	let (tx, rx) = tokio::sync::oneshot::channel();

	let service_task = tokio::spawn(service::serve(tx));
	let provider_task = tokio::spawn(providers::serve(args.provider, rx));

	let result = tokio::select! {
        res = service_task => res?,
        res = provider_task => res?,
    };

	result
}
