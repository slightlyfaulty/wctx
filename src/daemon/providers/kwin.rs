use super::*;
use std::env;
use std::path::PathBuf;
use zbus::{Connection, proxy};

const SCRIPT: &[u8] = include_bytes!("assets/kwin/kwin.min.js");

pub fn detect() -> Option<WindowProvider> {
	if env::var("KDE_SESSION_VERSION").unwrap_or_default() != "" {
		Some(WindowProvider::KWin)
	} else {
		None
	}
}

pub async fn serve() -> Result<()> {
	let connection = Connection::session().await?;
	let kwin_scripts = KWinScriptsProxy::new(&connection).await?;
	
	let script_path = write_script().await?;
	let script_path_str = script_path.to_str().unwrap();
	
	let mut is_loaded = true;

	while is_loaded {
		is_loaded = kwin_scripts.is_script_loaded(&script_path_str).await?;

		if is_loaded {
			kwin_scripts.unload_script(&script_path_str).await?;
			sleep(Duration::from_millis(100)).await;
		}
	}

	let script_num = kwin_scripts.load_script(&script_path_str).await?;
	let script_dbus_path = format!("/Scripting/Script{}", script_num);

	sleep(Duration::from_millis(100)).await;

	let script_runner = ScriptRunnerProxy::builder(&connection).path(script_dbus_path)?.build().await?;
	script_runner.run().await?;

	wait_for_exit().await;

	let _ = script_runner.stop().await;
	let _ = tokio::fs::remove_file(script_path).await;
	
	Ok(())
}

async fn write_script() -> Result<PathBuf> {
	let mut path = match env::var("XDG_RUNTIME_DIR").unwrap_or_default().as_ref() {
		"" => env::temp_dir(),
		dir => PathBuf::from(dir),
	};

	path.push("wctx_kwin.js");
	tokio::fs::write(&path, SCRIPT).await?;

	Ok(path)
}

#[proxy(
	interface = "org.kde.kwin.Scripting",
	default_service = "org.kde.KWin",
	default_path = "/Scripting",
)]
trait KWinScripts {
	#[zbus(name = "loadScript")]
	async fn load_script(&self, script: &str) -> zbus::Result<i32>;

	#[zbus(name = "unloadScript")]
	async fn unload_script(&self, script: &str) -> zbus::Result<bool>;

	#[zbus(name = "isScriptLoaded")]
	async fn is_script_loaded(&self, script: &str) -> zbus::Result<bool>;
}

#[proxy(
	interface = "org.kde.kwin.Script",
	default_service = "org.kde.KWin",
	default_path = "/Scripting/Script0",
)]
trait ScriptRunner {
	#[zbus(name = "run")]
	async fn run(&self) -> zbus::Result<()>;

	#[zbus(name = "stop")]
	async fn stop(&self) -> zbus::Result<()>;
}
