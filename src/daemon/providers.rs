mod x11;
mod kwin;
mod gnome;

use crate::types::*;
use super::service::ServiceProxy;
use anyhow::{anyhow, Result};
use color_print::*;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::oneshot::Receiver;
use tokio::time::{sleep, Duration};

#[derive(Copy, Clone, Debug, clap::ValueEnum, strum::Display)]
#[clap(rename_all = "lowercase")]
pub enum WindowProvider {
	X11,
	KWin,
	GNOME,
}

pub async fn serve(provider: Option<WindowProvider>, rx: Receiver<ServiceProxy<'_>>) -> Result<()> {
	let provider = provider
		.or_else(x11::detect)
		.or_else(kwin::detect)
		.or_else(gnome::detect)
		.ok_or_else(|| anyhow!("No supported window provider detected."))?;

	cprintln!("<b!>Using window provider: <w><s>{}", provider);

	// wait for dbus to be ready and get a service proxy for providers that need it
	let service = rx.await?;

	let result = match provider {
		WindowProvider::X11 => x11::serve(&service).await,
		WindowProvider::KWin => kwin::serve().await,
		WindowProvider::GNOME => gnome::serve(&service).await,
	};
	
	if result.is_err() {
		ceprintln!("<r!>Window provider <s>{}</> failed.", provider);
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
