use super::*;
use std::env;
use std::path::PathBuf;
//use std::io::{self, Write};
use zbus::{Connection, proxy};

const EXT_UUID: &str = "wctx@slightlyfaulty.github.io";

const EXT_FILES: &[&[u8]] = &[
	include_bytes!("assets/gnome/extension.js"),
	include_bytes!("assets/gnome/metadata.json"),
];

pub fn detect() -> Option<WindowProvider> {
	if env::var("XDG_SESSION_DESKTOP").unwrap_or_default() == "gnome" {
		Some(WindowProvider::GNOME)
	} else {
		None
	}
}

pub async fn serve(service: &ServiceProxy<'_>) -> Result<()> {
	let connection = Connection::session().await?;
	let extensions = ShellExtensionsProxy::new(&connection).await?;

	let enabled = extensions.enableExtension(EXT_UUID).await?;

	if !enabled {
		let ext_dir = get_extensions_dir()?.join(EXT_UUID);

		if tokio::fs::try_exists(&ext_dir).await? {
			return Err(anyhow!(cformat!("Failed to enable GNOME Shell extension \"<y!><s>{}</></>\". Please check that it's installed and loaded.", EXT_UUID)));
		}

		cprintln!("<y!>Installing GNOME Shell helper extension...");
		
		tokio::fs::create_dir(&ext_dir).await?;
		tokio::fs::write(ext_dir.join("extension.js"), EXT_FILES[0]).await?;
		tokio::fs::write(ext_dir.join("metadata.json"), EXT_FILES[1]).await?;

		cprintln!("<b!>Extension installed successfully! Please log out and log back in to activate it.");
		
		service.application.set_status("The GNOME Shell helper extension was installed. Please log out and log back in to activate it.".into()).await?;

		// TODO: Use this code when the extension gets accepted in the GNOME Shell Extensions directory
		/*cprint!("<y!>Installing GNOME Shell helper extension...");
		io::stdout().flush()?;

		let result = extensions.installRemoteExtension(EXT_UUID).await;

		if result.is_err() {
			cprintln!(" <r!><s>FAILED!");
			return Err(anyhow!("Unable to install from the GNOME Shell Extensions directory. Please install it manually: https://extensions.gnome.org/extension/62220/wctx/"));
		}

		cprintln!(" <g!><s>SUCCESS!");
		let enabled = extensions.enableExtension(EXT_UUID).await?;

		if !enabled {
			return Err(anyhow!(cformat!("Failed to enable GNOME Shell extension \"<y!><s>{}</></>\". Please check that it's installed correctly.", EXT_UUID)));
		}*/
	}

	wait_for_exit().await;

	let _ = extensions.disableExtension(EXT_UUID).await;

	Ok(())
}

fn get_extensions_dir() -> Result<PathBuf> {
	if let Ok(dir) = env::var("XDG_DATA_HOME") {
		Ok(PathBuf::from(dir).join("gnome-shell/extensions"))
	} else {
		match dirs::home_dir() {
			Some(home) => Ok(home.join(".local/share/gnome-shell/extensions")),
			None => Err(anyhow!("Cannot find home directory"))
		}
	}
}

#[proxy(
	interface = "org.gnome.Shell.Extensions",
	default_service = "org.gnome.Shell.Extensions",
	default_path = "/org/gnome/Shell/Extensions",
)]
trait ShellExtensions {
	async fn enableExtension(&self, uuid: &str) -> zbus::Result<bool>;
	async fn disableExtension(&self, uuid: &str) -> zbus::Result<bool>;
	//async fn installRemoteExtension(&self, uuid: &str) -> zbus::Result<String>;
}
