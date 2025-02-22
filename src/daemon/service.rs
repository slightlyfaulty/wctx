use crate::types::*;
use std::future;
use anyhow::{anyhow, Result};
use color_print::*;
use tokio::sync::oneshot::Sender;
use zbus::{connection, interface, fdo, object_server::SignalEmitter};

pub struct ServiceProxy<'a> {
	pub application: ApplicationProxy<'a>,
	pub windows: WindowsProxy<'a>,
}

struct Application {
	status: String,
}

#[interface(
	name = "org.wctx.Application",
	proxy(
		default_path = "/",
		default_service = "org.wctx",
	),
)]
impl Application {
	#[zbus(property)]
	async fn status(&self) -> String {
		self.status.clone()
	}

	#[zbus(property)]
	async fn set_status(&mut self, value: &str) {
		self.status = value.to_string();
	}

	/*async fn debug(&mut self, value: &str) {
		println!("Debug: {}", value);
	}*/
}

struct Windows {
	active_window: WindowDict,
	pointer_window: WindowDict,
}

#[interface(
	name = "org.wctx.Windows",
	proxy(
		default_path = "/",
		default_service = "org.wctx",
	),
)]
impl Windows {
	#[zbus(property)]
	async fn active_window(&self) -> DictMap {
		self.active_window.as_map()
	}

	#[zbus(property)]
	async fn pointer_window(&self) -> DictMap {
		self.pointer_window.as_map()
	}

	async fn set_window(
		&mut self,
		context: WindowContext,
		window: DictMap<'_>,
		#[zbus(signal_emitter)]
		emitter: SignalEmitter<'_>
	) -> fdo::Result<()> {
		let dict = WindowDict::try_from(window)?;

		match context {
			WindowContext::Both => {
				self.active_window = dict.clone();
				self.pointer_window = dict;
				self.active_window_changed(&emitter).await?;
				self.pointer_window_changed(&emitter).await?;
			}
			WindowContext::Active => {
				self.active_window = dict;
				self.active_window_changed(&emitter).await?;
			}
			WindowContext::Pointer => {
				self.pointer_window = dict;
				self.pointer_window_changed(&emitter).await?;
			}
		};

		Ok(())
	}

	async fn update_window(
		&mut self,
		context: WindowContext,
		key: &str,
		value: &str,
		#[zbus(signal_emitter)]
		emitter: SignalEmitter<'_>
	) -> fdo::Result<()> {
		match context {
			WindowContext::Both => {
				self.active_window.update(key, value)?;
				self.pointer_window.update(key, value)?;
				self.active_window_changed(&emitter).await?;
				self.pointer_window_changed(&emitter).await?;
			}
			WindowContext::Active => {
				self.active_window.update(key, value)?;
				self.active_window_changed(&emitter).await?;
			}
			WindowContext::Pointer => {
				self.pointer_window.update(key, value)?;
				self.pointer_window_changed(&emitter).await?;
			}
		};

		Ok(())
	}
}

pub async fn serve(tx: Sender<ServiceProxy<'_>>) -> Result<()> {
	let application = Application {
		status: Default::default(),
	};
	
	let windows = Windows {
		active_window: WindowDict::default(),
		pointer_window: WindowDict::default(),
	};

	let connection = connection::Builder::session()?
		.name("org.wctx")?
		.serve_at("/", application)?
		.serve_at("/", windows)?
		.build().await?;

	let service = ServiceProxy {
		application: ApplicationProxy::new(&connection).await?,
		windows: WindowsProxy::new(&connection).await?,
	};

	if tx.send(service).is_ok() {
		cprintln!("<g>D-Bus service started...");
	} else {
		return Err(anyhow!("Failed to sync D-Bus service with provider thread"));
	}

	future::pending::<Result<()>>().await
}
