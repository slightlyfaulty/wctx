use crate::types::*;
use std::fmt::Display;
use anyhow::{anyhow, Result};
use color_print::*;
use colored_json::to_colored_json_auto;
use futures_lite::stream::StreamExt;
use serde::Serialize;
use zbus::{Connection, proxy};

#[derive(clap::Args, Clone, Debug)]
pub struct Args {
	/// The window context to query
	#[arg(required = true)]
	context: Option<QueryContext>,

	/// Query a single property value
	property: Option<QueryProperty>,

	/// Output format
	#[arg(short, long, value_enum, default_value_t = QueryFormat::default())]
	format: QueryFormat,

	/// Monitor and output window changes
	#[arg(short, long)]
	watch: bool,
}

#[derive(Copy, Clone, Debug, clap::ValueEnum)]
pub enum QueryContext {
	Active,
	Pointer,
}

#[derive(Copy, Clone, Default, Debug, clap::ValueEnum, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum QueryFormat {
	#[default]
	Flat,
	Dict,
	JSON,
	TOML,
	CSV,
}

#[derive(Copy, Clone, Debug, clap::ValueEnum, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum QueryProperty {
	ID,
	Name,
	Class,
	PID,
	Title,
	Type,
	Role,
	State,
	Display,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum WindowProperty<'a> {
	ID(&'a str),
	Name(&'a str),
	Class(&'a str),
	PID(u32),
	Title(&'a str),
	Type(WindowType),
	Role(&'a str),
	State(WindowState),
	Display(&'a str),
}

impl Display for WindowProperty<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::ID(v) => write!(f, "{}", v),
			Self::Name(v) => write!(f, "{}", v),
			Self::Class(v) => write!(f, "{}", v),
			Self::PID(v) => write!(f, "{}", v),
			Self::Title(v) => write!(f, "{}", v),
			Self::Type(v) => write!(f, "{}", v),
			Self::Role(v) => write!(f, "{}", v),
			Self::State(v) => write!(f, "{}", v),
			Self::Display(v) => write!(f, "{}", v),
		}
	}
}

impl WindowDict {
	fn prop(&self, prop: QueryProperty) -> WindowProperty {
		match prop {
			QueryProperty::ID => WindowProperty::ID(&self.id),
			QueryProperty::Name => WindowProperty::Name(&self.name),
			QueryProperty::Class => WindowProperty::Class(&self.class),
			QueryProperty::PID => WindowProperty::PID(self.pid),
			QueryProperty::Title => WindowProperty::Title(&self.title),
			QueryProperty::Type => WindowProperty::Type(self.r#type),
			QueryProperty::Role => WindowProperty::Role(&self.role),
			QueryProperty::State => WindowProperty::State(self.state),
			QueryProperty::Display => WindowProperty::Display(&self.display),
		}
	}
}

struct Printer {
	window: Option<WindowDict>,
	property: Option<QueryProperty>,
	format: QueryFormat,
	output: String,
	linebreak: bool,
	first: bool,
}

impl Printer {
	fn new(property: Option<QueryProperty>, format: QueryFormat, watch: bool) -> Self {
		let linebreak = if property.is_some() {
			!matches!(format, QueryFormat::TOML | QueryFormat::CSV)
		} else {
			watch && matches!(format, QueryFormat::Dict | QueryFormat::JSON | QueryFormat::TOML)
		};

		Self {
			window: None,
			property,
			format,
			output: Default::default(),
			linebreak,
			first: true,
		}
	}

	fn print(&mut self, window: WindowDict) {
		let Ok(output) = self.format(&window) else {
			return;
		};

		let mut print = true;

		if let Some(last_window) = &self.window {
			if window.id == last_window.id && output == self.output {
				print = false;
			}
		}

		self.window = Some(window);

		if print {
			self.output = output;
			self.first = false;

			if self.linebreak {
				println!("{}", self.output);
			} else {
				print!("{}", self.output);
			}
		}
	}

	fn format(&self, window: &WindowDict) -> Result<String> {
		if let Some(qp) = self.property {
			let prop = window.prop(qp);

			match self.format {
				QueryFormat::Flat => {
					Ok(format!("{}", prop))
				}
				QueryFormat::Dict => {
					Ok(cformat!("<b!>{}:</> {}", qp, prop))
				}
				QueryFormat::TOML => {
					toml::to_string(&prop).map_err(|e| e.into())
				}
				QueryFormat::JSON => {
					serde_json::to_value(prop)
						.map(|v| to_colored_json_auto(&v).unwrap_or_default())
						.map_err(|e| e.into())
				}
				QueryFormat::CSV => {
					let mut wtr = csv::WriterBuilder::new()
						.has_headers(false)
						.from_writer(vec![]);

					if self.first {
						wtr.serialize(qp.to_string())?;
					}

					wtr.serialize(prop)?;
					String::from_utf8(wtr.into_inner()?).map_err(|e| e.into())
				}
			}
		} else {
			match self.format {
				QueryFormat::Flat => {
					Ok(cformat!("\
						<b!>id:</> {}<k!>,</> \
						<b!>name:</> {}<k!>,</> \
						<b!>class:</> {}<k!>,</> \
						<b!>pid:</> {}<k!>,</> \
						<b!>title:</> {}<k!>,</> \
						<b!>type:</> {}<k!>,</> \
						<b!>role:</> {}<k!>,</> \
						<b!>state:</> {}<k!>,</> \
						<b!>display:</> {}\n\
					", window.id, window.name, window.class, window.pid, window.title, window.r#type, window.role, window.state, window.display))
				}
				QueryFormat::Dict => {
					Ok(cformat!("\
						<b!>id:</> {}\n\
						<b!>name:</> {}\n\
						<b!>class:</> {}\n\
						<b!>pid:</> {}\n\
						<b!>title:</> {}\n\
						<b!>type:</> {}\n\
						<b!>role:</> {}\n\
						<b!>state:</> {}\n\
						<b!>display:</> {}\n\
					", window.id, window.name, window.class, window.pid, window.title, window.r#type, window.role, window.state, window.display))
				}
				QueryFormat::TOML => {
					toml::to_string(window).map_err(|e| e.into())
				}
				QueryFormat::JSON => {
					serde_json::to_value(window)
						.map(|v| to_colored_json_auto(&v).unwrap_or_default())
						.map_err(|e| e.into())
				}
				QueryFormat::CSV => {
					let mut wtr = csv::WriterBuilder::new()
						.has_headers(self.first)
						.from_writer(vec![]);

					wtr.serialize(window)?;
					String::from_utf8(wtr.into_inner()?).map_err(|e| e.into())
				}
			}
		}
	}
}

#[proxy(
	interface = "org.wctx.Application",
	default_service = "org.wctx",
	default_path = "/"
)]
trait Application {
	#[zbus(property)]
	fn status(&self) -> zbus::Result<String>;
}

#[proxy(
	interface = "org.wctx.Windows",
	default_service = "org.wctx",
	default_path = "/"
)]
trait Windows {
	#[zbus(property)]
	fn active_window(&self) -> zbus::Result<DictMap>;

	#[zbus(property)]
	fn pointer_window(&self) -> zbus::Result<DictMap>;
}

pub async fn run(args: Args) -> Result<()> {
	let connection = Connection::session().await?;
	let application = ApplicationProxy::new(&connection).await?;
	
	let status = application.status().await.map_err(|_| {
		anyhow!(cformat!("Couldn't connect to the wctx daemon. You might need to start it with \"<y!><s>systemctl --user start wctx</></>\" or manually run \"<y!><s>wctx daemon</></>\"."))
	})?;

	if status != "" {
		ceprintln!("<r!><s>Daemon:</></> {}", status);
		std::process::exit(126); // command cannot execute
	}

	let windows = WindowsProxy::new(&connection).await?;
	let window_arg = args.context.unwrap();

	let window = match window_arg {
		QueryContext::Active => windows.active_window().await,
		QueryContext::Pointer => windows.pointer_window().await,
	}?;

	let mut printer = Printer::new(args.property, args.format, args.watch);
	printer.print(window.try_into()?);

	if args.watch {
		let mut stream = match window_arg {
			QueryContext::Active => windows.receive_active_window_changed().await,
			QueryContext::Pointer => windows.receive_pointer_window_changed().await,
		};

		while let Some(changed) = stream.next().await {
			printer.print(changed.get().await?.try_into()?);
		}
	}

	Ok(())
}
