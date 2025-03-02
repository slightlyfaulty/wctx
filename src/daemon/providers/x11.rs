use super::*;
use crate::daemon::debouncer::Debouncer;
use std::env;
use std::collections::{HashMap, HashSet};
use anyhow::Result;
use tokio::time::Duration;
use x11rb_async::connection::Connection;
use x11rb_async::rust_connection::RustConnection;
use x11rb_async::protocol::{Event, xproto::*, randr::*};
use x11rb_async::protocol::randr::ConnectionExt as _;

pub fn detect() -> Option<WindowProvider> {
	if env::var("XDG_SESSION_TYPE").unwrap_or_default() == "x11" {
		Some(WindowProvider::X11)
	} else {
		None
	}
}

pub async fn serve(service: &ServiceProxy<'_>) -> Result<()> {
	let mut x = X11::new(service).await?;

	// register window events
	let event_mask = ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY | EventMask::FOCUS_CHANGE | EventMask::ENTER_WINDOW | EventMask::PROPERTY_CHANGE);
	x.conn.change_window_attributes(x.root, &event_mask).await?;

	let event_mask = event_mask.event_mask(EventMask::FOCUS_CHANGE | EventMask::ENTER_WINDOW | EventMask::PROPERTY_CHANGE);

	for win_id in x.conn.query_tree(x.root).await?.reply().await?.children {
		x.cascade_event_mask(win_id, &event_mask).await?;
	}

	// register randr events
	x.conn.randr_select_input(x.root, NotifyMask::SCREEN_CHANGE | NotifyMask::OUTPUT_CHANGE | NotifyMask::CRTC_CHANGE).await?;

	// flush to send to X11 server
	x.conn.flush().await?;

	// determine initial windows
	if let Some(active_window) = x.query_active_window().await {
		x.set_window(WindowContext::Active, active_window).await?;
	}

	if let Some(pointer_window) = x.query_pointer_window().await {
		x.set_window(WindowContext::Pointer, pointer_window).await?;
	}

	// debouncers for window move events
	let mut active_move_debouncer = Debouncer::new(Duration::from_millis(15));
	let mut pointer_move_debouncer = Debouncer::new(Duration::from_millis(15));

	loop {
		tokio::select! {
			event = x.conn.wait_for_event() => {
				match event? {
					Event::CreateNotify(e) => {
						if e.override_redirect {
							continue;
						}

						if x.conn.change_window_attributes(e.window, &event_mask).await.is_ok() {
							x.conn.flush().await?;
						}
					},
					Event::FocusIn(e) => {
						if e.mode != NotifyMode::NORMAL || e.detail != NotifyDetail::NONLINEAR_VIRTUAL {
							continue;
						}

						if e.event == x.active_window.id || e.event == x.active_window.top_id {
							continue;
						}

						let window = if e.event == x.pointer_window.id || e.event == x.pointer_window.top_id {
							x.pointer_window.clone()
						} else {
							let Some(win_match) = x.resolve_window_match(e.event).await else {
								continue;
							};

							if win_match.0 == x.active_window.id {
								continue;
							}

							x.get_window(e.event, win_match).await
						};

						x.set_window(WindowContext::Active, window).await?;
					},
					Event::EnterNotify(e) => {
						if e.event == x.pointer_window.id || e.event == x.pointer_window.top_id || e.child == x.pointer_window.id {
							continue;
						}

						let window = if e.event == x.active_window.id || e.event == x.active_window.top_id {
							let mut window = x.active_window.clone();
							// need to recalculate display when moving window under mouse (e.g. with keyboard)
							// from another display, because active window display won't be updated just yet
							window.display = x.get_window_display(window.id).await.unwrap_or_default();
							window
						} else {
							let Some(win_match) = x.resolve_window_match(e.event).await else {
								continue;
							};

							if win_match.0 == x.pointer_window.id {
								continue;
							}

							x.get_window(e.event, win_match).await
						};

						x.set_window(WindowContext::Pointer, window).await?;
					},
					Event::PropertyNotify(e) => {
						if e.window != x.active_window.id && e.window != x.pointer_window.id {
							continue;
						}

						if e.atom == x.atoms.WM_NAME {
							let new_title = x.get_window_title(e.window).await.unwrap_or_default();

							if e.window == x.active_window.id && new_title != x.active_window.title {
								x.update_window(WindowContext::Active, XUpdateProp::Title(new_title)).await?;
							} else if e.window == x.pointer_window.id && new_title != x.pointer_window.title {
								x.update_window(WindowContext::Pointer, XUpdateProp::Title(new_title)).await?;
							}
						} else if e.atom == x.atoms.WM_STATE {
							let new_state = x.get_window_state(e.window).await.unwrap_or_default();

							if e.window == x.active_window.id && new_state != x.active_window.state {
								x.update_window(WindowContext::Active, XUpdateProp::State(new_state)).await?;
							} else if e.window == x.pointer_window.id && new_state != x.pointer_window.state {
								x.update_window(WindowContext::Pointer, XUpdateProp::State(new_state)).await?;
							}
						}
					},
					Event::RandrNotify(_) | Event::RandrScreenChangeNotify(_) => {
						x.displays = get_displays(&x.conn, x.root).await?;
					}
					Event::ConfigureNotify(e) => {
						if e.override_redirect {
							continue;
						}

						if e.window == x.active_window.top_id {
							active_move_debouncer.push(e);
						} else if e.window == x.pointer_window.top_id {
							pointer_move_debouncer.push(e);
						}
					}
					_ => {}
				}
			}
			Some(e) = active_move_debouncer.next() => {
				if e.window != x.active_window.top_id {
					continue;
				}

				let new_display = x.calc_window_display(e.x, e.y, e.width, e.height).unwrap_or_default();

				if new_display != x.active_window.display {
					x.update_window(WindowContext::Active, XUpdateProp::Display(new_display)).await?;
				}
			}
			Some(e) = pointer_move_debouncer.next() => {
				if e.window != x.pointer_window.top_id {
					continue;
				}

				let new_display = x.calc_window_display(e.x, e.y, e.width, e.height).unwrap_or_default();

				if new_display != x.pointer_window.display {
					x.update_window(WindowContext::Pointer, XUpdateProp::Display(new_display)).await?;
				}
			}
		}
	}
}

async fn get_displays(conn: &RustConnection, root: Window) -> Result<Vec<XDisplay>> {
	let reply = conn.randr_get_monitors(root, true).await?.reply().await?;
	let mut monitors: Vec<XDisplay> = Vec::new();

	for m in reply.monitors {
		let reply = conn.get_atom_name(m.name).await?.reply().await?;
		let name: Box<str> = std::str::from_utf8(&reply.name)?.into();

		monitors.push(XDisplay {
			name,
			x: m.x,
			y: m.y,
			w: m.width as i16,
			h: m.height as i16,
		})
	}

	Ok(monitors)
}

struct X11<'a> {
	conn: RustConnection,
	root: Window,
	service: &'a ServiceProxy<'a>,
	atoms: Atoms,
	window_types: HashMap<Atom, WindowType>,
	displays: Vec<XDisplay>,
	active_window: XWindow,
	pointer_window: XWindow,
}

impl<'a> X11<'a> {
	async fn new(service: &'a ServiceProxy<'_>) -> Result<Self> {
		let (conn, screen_num, drive) = RustConnection::connect(None).await?;
		let root = conn.setup().roots[screen_num].root;

		tokio::spawn(async move {
			match drive.await {
				Err(e) => anyhow!("Error while driving the connection: {}", e),
				_ => unreachable!(),
			}
		});

		concurrent!(
			let atoms = Atoms::load(&conn),
			let window_types = Atoms::load_window_types(&conn),
			let displays = get_displays(&conn, root),
		);

		Ok(X11 {
			conn,
			root,
			service,
			atoms: atoms?,
			window_types: window_types?,
			displays: displays?,
			active_window: XWindow::default(),
			pointer_window: XWindow::default(),
		})
	}

	async fn set_window(&mut self, context: WindowContext, window: XWindow) -> Result<()> {
		let window = match context {
			WindowContext::Active => {
				self.active_window = window;
				&self.active_window
			},
			WindowContext::Pointer => {
				self.pointer_window = window;
				&self.pointer_window
			},
			WindowContext::Both => {
				self.active_window = window.clone();
				self.pointer_window = window;
				&self.active_window
			}
		};

		self.service.windows.set_window(context, window.as_map()).await.map_err(Into::into)
	}

	async fn update_window(&mut self, mut context: WindowContext, prop: XUpdateProp) -> Result<()> {
		if context == WindowContext::Active && self.active_window.id == self.pointer_window.id {
			context = WindowContext::Both;
		}

		let (key, value) = match context {
			WindowContext::Active => self.active_window.update(prop),
			WindowContext::Pointer => self.pointer_window.update(prop),
			WindowContext::Both => {
				self.active_window.update(prop.clone());
				self.pointer_window.update(prop)
			},
		};

		self.service.windows.update_window(context, key, value).await.map_err(Into::into)
	}

	async fn get_window(&self, top_id: Window, win_match: PartialMatch) -> XWindow {
		if win_match.0 == 0 {
			return XWindow::default();
		}

		let id = win_match.0;

		concurrent!(
			let pid = self.get_window_pid(id),
			let title = self.get_window_title(id),
			let r#type = self.get_window_type(id),
			let role = self.get_window_role(id),
			let state = self.get_window_state(id),
			let display = self.get_window_display(id),
		);

		XWindow::new(
			win_match,
			top_id,
			pid.unwrap_or_default(),
			title.unwrap_or_default(),
			r#type.unwrap_or_default(),
			role.unwrap_or_default(),
			state.unwrap_or_default(),
			display.unwrap_or_default(),
		)
	}

	async fn query_active_window(&self) -> Option<XWindow> {
		let win_id = self.get_window_prop(self.root, self.atoms.ACTIVE_WINDOW, AtomEnum::WINDOW).await?.value32()?.next()?;

		if win_id == 0 {
			return None;
		}

		let win_match = self.resolve_window_match(win_id).await?;
		let window = self.get_window(win_id, win_match).await;

		Some(window)
	}

	async fn query_pointer_window(&self) -> Option<XWindow> {
		let win_id = self.conn.query_pointer(self.root).await.ok()?.reply().await.ok()?.child;

		if win_id == 0 {
			return None;
		}

		let win_match = self.resolve_window_match(win_id).await?;
		let window = self.get_window(win_id, win_match).await;

		Some(window)
	}

	async fn cascade_event_mask(&self, win_id: Window, event_mask: &ChangeWindowAttributesAux) -> Result<bool> {
		if win_id == 0 {
			return Ok(false);
		}

		self.conn.change_window_attributes(win_id, &event_mask).await?;

		// if window is valid we shouldn't cascade any further as we may get events for meta/proxy windows that we don't want
		if self.is_valid_window(win_id).await {
			return Ok(true);
		}

		for child_id in self.conn.query_tree(win_id).await?.reply().await?.children {
			if Box::pin(self.cascade_event_mask(child_id, &event_mask)).await? {
				return Ok(true);
			}
		}

		Ok(false)
	}

	async fn resolve_window_match(&self, win_id: Window) -> Option<PartialMatch> {
		if win_id == 0 {
			return None;
		}

		if let win_match @ Some(_) = self.get_window_match(win_id).await {
			return win_match;
		}

		for child in self.conn.query_tree(win_id).await.ok()?.reply().await.ok()?.children {
			if let found @ Some(_) = Box::pin(self.resolve_window_match(child)).await {
				return found;
			}
		}

		None
	}

	async fn get_window_match(&self, win_id: Window) -> Option<PartialMatch> {
		let reply = self.get_window_prop(win_id, AtomEnum::WM_CLASS, AtomEnum::STRING).await?;

		if reply.value_len == 0 {
			return None
		}

		let mut value = reply.value;
		let mut sep = None;

		for (i, b) in value.iter_mut().enumerate() {
			if *b == 0 {
				if sep == None {
					sep = Some(i);
				}
			} else if *b == b' ' {
				*b = b'-';
			} else {
				b.make_ascii_lowercase();
			}
		}

		let sep = sep?;

		let name = std::str::from_utf8(&value[0..sep]).ok()?.into();
		let class = std::str::from_utf8(&value[(sep+1)..(value.len()-1)]).ok()?.into();

		Some((win_id, name, class))
	}

	async fn get_window_prop<A, B>(&self, win_id: Window, atom_prop: A, atom_type: B) -> Option<GetPropertyReply>
	where
		A: Into<Atom> + Send + 'static,
		B: Into<Atom> + Send + 'static,
	{
		let reply = self.conn.get_property(false, win_id, atom_prop, atom_type, 0, 1024).await.ok()?.reply().await.ok()?;

		if reply.value_len == 0 {
			return None
		}

		Some(reply)
	}

	async fn is_valid_window(&self, win_id: Window) -> bool {
		if let Some(reply) = self.get_window_prop(win_id, AtomEnum::WM_CLASS, AtomEnum::STRING).await {
			if reply.value_len > 0 {
				return true;
			}
		}

		false
	}

	async fn get_window_pid(&self, win_id: Window) -> Option<u32> {
		let reply = self.get_window_prop(win_id, self.atoms.WM_PID, AtomEnum::CARDINAL).await?;
		let value = reply.value32()?.next()?;

		Some(value)
	}

	async fn get_window_title(&self, win_id: Window) -> Option<Box<str>> {
		let result = self.get_window_prop(win_id, self.atoms.WM_NAME, self.atoms.UTF8_STRING).await;

		/*if result.is_none() {
			result = self.get_window_prop(win_id, AtomEnum::WM_NAME, AtomEnum::STRING).await;
		}*/

		Some(std::str::from_utf8(&result?.value).ok()?.into())
	}

	async fn get_window_type(&self, win_id: Window) -> Option<WindowType> {
		let reply = self.get_window_prop(win_id, self.atoms.WM_WINDOW_TYPE, AtomEnum::ATOM).await?;

		let value = reply.value32()?.next()?;
		let win_type = self.window_types.get(&value)?;

		Some(*win_type)
	}

	async fn get_window_role(&self, win_id: Window) -> Option<Box<str>> {
		let result = self.get_window_prop(win_id, self.atoms.WM_WINDOW_ROLE, AtomEnum::STRING).await;

		Some(std::str::from_utf8(&result?.value).ok()?.into())
	}

	async fn get_window_state(&self, win_id: Window) -> Option<WindowState> {
		let reply = self.get_window_prop(win_id, self.atoms.WM_STATE, AtomEnum::ATOM).await?;
		let states: HashSet<u32> = reply.value32()?.collect();

		if states.contains(&self.atoms.WM_STATE_FULLSCREEN) {
			Some(WindowState::Fullscreen)
		} else if states.contains(&self.atoms.WM_STATE_MAXIMIZED_HORZ) && states.contains(&self.atoms.WM_STATE_MAXIMIZED_VERT) {
			Some(WindowState::Maximized)
		} else {
			Some(WindowState::Normal)
		}
	}

	async fn get_window_display(&self, win_id: Window) -> Option<Box<str>> {
		let geometry = self.conn.get_geometry(win_id).await.ok()?.reply().await.ok()?;
		let translate = self.conn.translate_coordinates(win_id, self.root, geometry.x, geometry.y).await.ok()?.reply().await.ok()?;

		self.calc_window_display(translate.dst_x, translate.dst_y, geometry.width, geometry.height)
	}

	fn calc_window_display(&self, x: i16, y: i16, w: u16, h: u16) -> Option<Box<str>> {
		let w = w as i16;
		let h = h as i16;

		// first try to find monitor containing the center point
		let cx = x + (w / 2);
		let cy = y + (h / 2);

		if let Some(d) = self.displays.iter().find(|d| {
			cx >= d.x
			&& cx < d.x + d.w
			&& cy >= d.y
			&& cy < d.y + d.h
		}) {
			return Some(d.name.clone());
		}

		// if center point isn't on any display, find the one with the most window overlap
		let mut matched = None;
		let mut max_overlap_area = 0;

		for d in self.displays.iter() {
			let over_x1 = i16::max(x, d.x);
			let over_y1 = i16::max(y, d.y);
			let over_x2 = i16::min(x + w, d.x + d.w);
			let over_y2 = i16::min(y + h, d.y + d.h);

			if over_x1 < over_x2 && over_y1 < over_y2 {
				let overlap_area = (over_x2 - over_x1) as u32 * (over_y2 - over_y1) as u32;

				if overlap_area > max_overlap_area {
					max_overlap_area = overlap_area;
					matched = Some(d.name.clone());
				}
			}
		}

		matched
	}
}

#[derive(Clone, Debug)]
enum XUpdateProp {
	Title(Box<str>),
	State(WindowState),
	Display(Box<str>),
	// TODO: Are any other properties likely to change?
}

#[derive(Clone, Debug)]
struct XWindow {
	id: Window,
	top_id: Window,
	name: Box<str>,
	class: Box<str>,
	pid: u32,
	title: Box<str>,
	r#type: WindowType,
	role: Box<str>,
	state: WindowState,
	display: Box<str>,
}

impl XWindow {
	fn new(win_match: PartialMatch, top_id: Window, pid: u32, title: Box<str>, r#type: WindowType, role: Box<str>, state: WindowState, display: Box<str>) -> Self {
		let (id, name, class) = win_match;

		Self {
			id,
			top_id,
			name,
			class,
			pid,
			title,
			r#type,
			role,
			state,
			display,
		}
	}

	fn as_map(&self) -> DictMap {
		WindowDict::new(
			&self.id.to_string(),
			&self.name,
			&self.class,
			self.pid,
			&self.title,
			self.r#type,
			&self.role,
			self.state,
			&self.display
		).into()
	}

	fn update(&mut self, prop: XUpdateProp) -> (WindowProp, &str) {
		match prop {
			XUpdateProp::Title(value) => { self.title = value; (WindowProp::Title, &self.title) },
			XUpdateProp::State(value) => { self.state = value; (WindowProp::State, self.state.as_ref()) },
			XUpdateProp::Display(value) => { self.display = value; (WindowProp::Display, &self.display) },
		}
	}
}

impl Default for XWindow {
	fn default() -> Self {
		Self {
			id: 0,
			top_id: 0,
			name: Default::default(),
			class: Default::default(),
			pid: 0,
			title: Default::default(),
			r#type: WindowType::None,
			role: Default::default(),
			state: WindowState::None,
			display: Default::default(),
		}
	}
}

#[derive(Debug)]
struct XDisplay {
	name: Box<str>,
	x: i16,
	y: i16,
	w: i16,
	h: i16,
}

type PartialMatch = (Window, Box<str>, Box<str>);

#[allow(non_snake_case)]
#[derive(Debug)]
struct Atoms {
	UTF8_STRING: Atom,
	ACTIVE_WINDOW: Atom,
	WM_NAME: Atom,
	WM_PID: Atom,
	WM_STATE: Atom,
	WM_STATE_MAXIMIZED_HORZ: Atom,
	WM_STATE_MAXIMIZED_VERT: Atom,
	WM_STATE_FULLSCREEN: Atom,
	WM_WINDOW_ROLE: Atom,
	WM_WINDOW_TYPE: Atom,
}

impl Atoms {
	async fn get_atom(conn: &RustConnection, name: &[u8]) -> Result<Atom> {
		Ok(conn.intern_atom(false, name).await?.reply().await?.atom)
	}

	#[allow(non_snake_case)]
	async fn load(conn: &RustConnection) -> Result<Self> {
		concurrent!(
			let UTF8_STRING             = Self::get_atom(&conn, b"UTF8_STRING"),
			let ACTIVE_WINDOW           = Self::get_atom(&conn, b"_NET_ACTIVE_WINDOW"),
			let WM_NAME                 = Self::get_atom(&conn, b"_NET_WM_NAME"),
			let WM_PID                  = Self::get_atom(&conn, b"_NET_WM_PID"),
			let WM_STATE                = Self::get_atom(&conn, b"_NET_WM_STATE"),
			let WM_STATE_MAXIMIZED_HORZ = Self::get_atom(&conn, b"_NET_WM_STATE_MAXIMIZED_HORZ"),
			let WM_STATE_MAXIMIZED_VERT = Self::get_atom(&conn, b"_NET_WM_STATE_MAXIMIZED_VERT"),
			let WM_STATE_FULLSCREEN     = Self::get_atom(&conn, b"_NET_WM_STATE_FULLSCREEN"),
			let WM_WINDOW_ROLE          = Self::get_atom(&conn, b"WM_WINDOW_ROLE"),
			let WM_WINDOW_TYPE          = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE"),
		);

		Ok(Self {
			UTF8_STRING: UTF8_STRING?,
			ACTIVE_WINDOW: ACTIVE_WINDOW?,
			WM_NAME: WM_NAME?,
			WM_PID: WM_PID?,
			WM_STATE: WM_STATE?,
			WM_STATE_MAXIMIZED_HORZ: WM_STATE_MAXIMIZED_HORZ?,
			WM_STATE_MAXIMIZED_VERT: WM_STATE_MAXIMIZED_VERT?,
			WM_STATE_FULLSCREEN: WM_STATE_FULLSCREEN?,
			WM_WINDOW_ROLE: WM_WINDOW_ROLE?,
			WM_WINDOW_TYPE: WM_WINDOW_TYPE?,
		})
	}

	#[allow(non_snake_case)]
	async fn load_window_types(conn: &RustConnection) -> Result<HashMap<u32, WindowType>> {
		concurrent!(
			let COMBO         = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_COMBO"),
			let DESKTOP       = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_DESKTOP"),
			let DIALOG        = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_DIALOG"),
			let DND           = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_DND"),
			let DOCK          = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_DOCK"),
			let DROPDOWN_MENU = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_DROPDOWN_MENU"),
			let MENU          = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_MENU"),
			let NORMAL        = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_NORMAL"),
			let NOTIFICATION  = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_NOTIFICATION"),
			let POPUP_MENU    = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_POPUP_MENU"),
			let SPLASH        = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_SPLASH"),
			let TOOLBAR       = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_TOOLBAR"),
			let TOOLTIP       = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_TOOLTIP"),
			let UTILITY       = Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_UTILITY"),
		);

		Ok(HashMap::from([
			(COMBO?,         WindowType::Combo),
			(DESKTOP?,       WindowType::Desktop),
			(DIALOG?,        WindowType::Dialog),
			(DND?,           WindowType::DND),
			(DOCK?,          WindowType::Dock),
			(DROPDOWN_MENU?, WindowType::DropdownMenu),
			(MENU?,          WindowType::Menu),
			(NORMAL?,        WindowType::Normal),
			(NOTIFICATION?,  WindowType::Notification),
			(POPUP_MENU?,    WindowType::PopupMenu),
			(SPLASH?,        WindowType::Splash),
			(TOOLBAR?,       WindowType::Toolbar),
			(TOOLTIP?,       WindowType::Tooltip),
			(UTILITY?,       WindowType::Utility),
		]))
	}
}
