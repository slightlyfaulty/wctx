mod x11;
mod kwin;
mod gnome;

use crate::types::*;
use super::service::ServiceProxy;
use anyhow::{anyhow, Result};
use colored::Colorize;
use strum::VariantNames;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::oneshot::Receiver;
use tokio::time::{sleep, Duration};

#[derive(Copy, Clone, Debug, clap::ValueEnum, strum::Display, strum::VariantNames)]
#[clap(rename_all = "lowercase")]
pub enum WindowProvider {
	X11,
	KWin,
	GNOME,
}

pub async fn serve(provider: Option<WindowProvider>, rx: Receiver<ServiceProxy<'_>>) -> Result<()> {
	let Some(provider) = provider
		.or_else(x11::detect)
		.or_else(kwin::detect)
		.or_else(gnome::detect)
		else {
			eprintln!(
				"{} No supported window provider detected. Currently supports: {}\n\n{}\n{}",
				"Error:".bright_red().bold(),
				WindowProvider::VARIANTS.join(", "),
				"If you would like to help get support added for your desktop, please feel free to post, comment or contribute:".bright_yellow(),
				"https://github.com/slightlyfaulty/wctx/issues"
			);
			std::process::exit(126);
		};

	println!("{} {}", "Using window provider:".bright_blue(), provider.to_string().white().bold());

	// wait for dbus to be ready and get a service proxy for providers that need it
	let service = rx.await?;

	let result = match provider {
		WindowProvider::X11 => x11::serve(&service).await,
		WindowProvider::KWin => kwin::serve().await,
		WindowProvider::GNOME => gnome::serve(&service).await,
	};

	if result.is_err() {
		eprintln!("{}", format!("Window provider {} failed.", provider.to_string().bold()).bright_red());
	}

	result
}

pub async fn wait_for_exit() {
	let mut sigint = signal(SignalKind::interrupt()).unwrap();
	let mut sighup = signal(SignalKind::hangup()).unwrap();
	let mut sigterm = signal(SignalKind::terminate()).unwrap();
	let mut sigquit = signal(SignalKind::quit()).unwrap();

	tokio::select! {
		_ = sigint.recv() => {}
		_ = sighup.recv() => {}
		_ = sigterm.recv() => {}
		_ = sigquit.recv() => {}
	}
}
