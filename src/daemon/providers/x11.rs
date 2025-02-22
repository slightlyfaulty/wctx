use super::*;
use crate::daemon::debouncer::Debouncer;
use std::env;
use std::collections::{HashMap, HashSet};
use anyhow::Result;
use tokio::time::Duration;
use x11rb_async::connection::Connection;
use x11rb_async::rust_connection::RustConnection;
use x11rb_async::protocol::{xproto::*, Event, randr::*};
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
	let mut active_window = x.get_active_window().await.unwrap_or_default();
	let mut pointer_window = x.get_pointer_window().await.unwrap_or_default();

	if active_window.id > 0 { x.send_window(WindowContext::Active, &active_window).await? }
	if pointer_window.id > 0 { x.send_window(WindowContext::Pointer, &pointer_window).await? }

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
						if e.detail != NotifyDetail::NONLINEAR_VIRTUAL || e.mode != NotifyMode::NORMAL {
							continue;
						}

						if e.event == active_window.id || e.event == active_window.top_id {
							continue;
						}

						if e.event == pointer_window.id || e.event == pointer_window.top_id {
							active_window = pointer_window.clone();
						} else {
							let win_match = x.resolve_window_match(e.event).await;

							let Some(win_match) = win_match else {
								continue;
							};

							if win_match.0 == active_window.id {
								continue;
							}

							active_window = x.get_window(e.event, win_match).await;
						}

						x.send_window(WindowContext::Active, &active_window).await?;
					},
					Event::EnterNotify(e) => {
						if e.event == pointer_window.id || e.event == pointer_window.top_id || e.child == pointer_window.id {
							continue;
						}

						if e.event == active_window.id || e.event == active_window.top_id {
							pointer_window = active_window.clone();
							pointer_window.display = x.get_window_display(pointer_window.id).await.unwrap_or_default();
						} else {
							let Some(win_match) = x.resolve_window_match(e.event).await else {
								continue;
							};

							if win_match.0 == pointer_window.id {
								continue;
							}

							pointer_window = x.get_window(e.event, win_match).await;
						}

						x.send_window(WindowContext::Pointer, &pointer_window).await?;
					},
					Event::PropertyNotify(e) => {
						if e.window != active_window.id && e.window != pointer_window.id {
							continue;
						}

						if e.atom == x.atoms.WM_NAME {
							let new_title = x.get_window_title(e.window).await.unwrap_or_default();

							if e.window == active_window.id && new_title != active_window.title {
								if active_window.id == pointer_window.id {
									active_window.title = new_title.clone();
									pointer_window.title = new_title;
									x.update_window(WindowContext::Both, "title", &active_window.title).await?;
								} else {
									active_window.title = new_title;
									x.update_window(WindowContext::Active, "title", &active_window.title).await?;
								}
							}
							else if e.window == pointer_window.id && new_title != pointer_window.title {
								pointer_window.title = new_title;
								x.update_window(WindowContext::Pointer, "title", &pointer_window.title).await?;
							}
						} else if e.atom == x.atoms.WM_WINDOW_ROLE {
							let new_role = x.get_window_role(e.window).await.unwrap_or_default();

							if e.window == active_window.id && new_role != active_window.role {
								if active_window.id == pointer_window.id {
									active_window.role = new_role.clone();
									pointer_window.role = new_role;
									x.update_window(WindowContext::Both, "role", &active_window.role).await?;
								} else {
									active_window.role = new_role;
									x.update_window(WindowContext::Active, "role", &active_window.role).await?;
								}
							}
							else if e.window == pointer_window.id && new_role != pointer_window.role {
								pointer_window.role = new_role;
								x.update_window(WindowContext::Pointer, "role", &pointer_window.role).await?;
							}
						} else if e.atom == x.atoms.WM_STATE {
							let new_state = x.get_window_state(e.window).await.unwrap_or_default();

							if e.window == active_window.id && new_state != active_window.state {
								if active_window.id == pointer_window.id {
									active_window.state = new_state.clone();
									pointer_window.state = new_state;
									x.update_window(WindowContext::Both, "state", &active_window.state.to_string()).await?;
								} else {
									active_window.state = new_state;
									x.update_window(WindowContext::Active, "state", &active_window.state.to_string()).await?;
								}
							}
							else if e.window == pointer_window.id && new_state != pointer_window.state {
								pointer_window.state = new_state;
								x.update_window(WindowContext::Pointer, "state", &pointer_window.state.to_string()).await?;
							}
						}
					},
					Event::ConfigureNotify(e) => {
						if e.override_redirect {
							continue;
						}

						if e.window == active_window.top_id {
							active_move_debouncer.push(e);
						} else if e.window == pointer_window.top_id {
							pointer_move_debouncer.push(e);
						}
					}
					Event::RandrNotify(_) | Event::RandrScreenChangeNotify(_) => {
						x.displays = get_displays(&x.conn, x.root).await?;
					}
					_ => {}
				}
			}
			Some(e) = active_move_debouncer.next() => {
				if e.window != active_window.top_id {
					continue;
				}

				let new_display = x.calc_window_display(e.x, e.y, e.width, e.height).unwrap_or_default();

				if new_display != active_window.display {
					if pointer_window.id == active_window.id {
						active_window.display = new_display;
						pointer_window.display = active_window.display.clone();
						x.send_window(WindowContext::Both, &pointer_window).await?;
					} else {
						active_window.display = new_display;
						x.send_window(WindowContext::Active, &active_window).await?;
					}
				}
			}
			Some(e) = pointer_move_debouncer.next() => {
				if e.window != pointer_window.top_id {
					continue;
				}

				let new_display = x.calc_window_display(e.x, e.y, e.width, e.height).unwrap_or_default();

				if new_display != pointer_window.display {
					pointer_window.display = new_display;
					x.send_window(WindowContext::Pointer, &pointer_window).await?;
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
	atoms: Atoms,
	displays: Vec<XDisplay>,
	service: &'a ServiceProxy<'a>,
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

		let displays = get_displays(&conn, root).await?;
		let atoms = Atoms::load(&conn).await?;

		Ok(X11 {
			conn,
			root,
			atoms,
			displays,
			service,
		})
	}

	async fn send_window(&self, context: WindowContext, window: &XWindow) -> Result<()> {
		self.service.windows.set_window(context, window.as_map()).await
			.map_err(|err| err.into())
	}

	async fn update_window(&self, context: WindowContext, key: &str, value: &str) -> Result<()> {
		self.service.windows.update_window(context, key, value).await
			.map_err(|err| err.into())
	}

	async fn get_window(&self, top_id: Window, win_match: PartialMatch) -> XWindow {
		if win_match.0 == 0 {
			return XWindow::default();
		}

		let id = win_match.0;
		let pid = self.get_window_pid(id).await.unwrap_or_default();
		let title = self.get_window_title(id).await.unwrap_or_default();
		let r#type = self.get_window_type(id).await.unwrap_or_default();
		let role = self.get_window_role(id).await.unwrap_or_default();
		let state = self.get_window_state(id).await.unwrap_or_default();
		let display = self.get_window_display(id).await.unwrap_or_default();

		XWindow::new(win_match, top_id, pid, title, r#type, role, state, display)
	}

	async fn get_active_window(&self) -> Option<XWindow> {
		let win_id = self.get_window_prop(self.root, self.atoms.ACTIVE_WINDOW, AtomEnum::WINDOW).await?.value32()?.next()?;

		if win_id == 0 {
			return None;
		}

		let win_match = self.resolve_window_match(win_id).await?;
		let window = self.get_window(win_id, win_match).await;

		Some(window)
	}

	async fn get_pointer_window(&self) -> Option<XWindow> {
		let win_id = self.conn.query_pointer(self.root).await.ok()?.reply().await.ok()?.child;

		if win_id == 0 {
			return None;
		}

		let win_match = self.resolve_window_match(win_id).await?;
		let window = self.get_window(win_id, win_match).await;

		Some(window)
	}

	async fn cascade_event_mask(&self, win_id: Window, event_mask: &ChangeWindowAttributesAux) -> Result<()> {
		if win_id == 0 {
			return Ok(());
		}

		self.conn.change_window_attributes(win_id, &event_mask).await?;

		// if window has a valid class we shouldn't cascade any further as we may get events for meta/proxy windows that we don't want events for
		if let Some(reply) = self.get_window_prop(win_id, AtomEnum::WM_CLASS, AtomEnum::STRING).await {
			if reply.value_len > 0 {
				return Ok(());
			}
		}

		for child_id in self.conn.query_tree(win_id).await?.reply().await?.children {
			Box::pin(self.cascade_event_mask(child_id, &event_mask)).await?;
		}

		Ok(())
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
	where A: Into<Atom> + Send + 'static, B: Into<Atom> + Send + 'static {
		let reply = self.conn.get_property(false, win_id, atom_prop, atom_type, 0, 1024).await.ok()?.reply().await.ok()?;

		if reply.value_len == 0 {
			return None
		}

		Some(reply)
	}

	async fn get_window_pid(&self, win_id: Window) -> Option<u32> {
		let reply = self.get_window_prop(win_id, self.atoms.WM_PID, AtomEnum::CARDINAL).await?;
		let value = reply.value32()?.next()?;

		Some(value)
	}

	async fn get_window_title(&self, win_id: Window) -> Option<Box<str>> {
		let result = self.get_window_prop(win_id, self.atoms.WM_NAME, self.atoms.UTF8_STRING).await;

		/*if result.is_none() {
			result = get_window_prop(x.conn, win, AtomEnum::WM_NAME, AtomEnum::STRING).await;
		}*/

		Some(std::str::from_utf8(&result?.value).ok()?.into())
	}

	async fn get_window_type(&self, win_id: Window) -> Option<WindowType> {
		let reply = self.get_window_prop(win_id, self.atoms.WM_WINDOW_TYPE, AtomEnum::ATOM).await?;

		let value = reply.value32()?.next()?;
		let win_type = self.atoms.window_types.get(&value)?;

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
	window_types: HashMap<Atom, WindowType>,
}

impl Atoms {
	async fn load(conn: &RustConnection) -> Result<Self> {
		Ok(Self {
			UTF8_STRING:             Self::get_atom(&conn, b"UTF8_STRING").await?,
			ACTIVE_WINDOW:           Self::get_atom(&conn, b"_NET_ACTIVE_WINDOW").await?,
			WM_NAME:                 Self::get_atom(&conn, b"_NET_WM_NAME").await?,
			WM_PID:                  Self::get_atom(&conn, b"_NET_WM_PID").await?,
			WM_STATE:                Self::get_atom(&conn, b"_NET_WM_STATE").await?,
			WM_STATE_MAXIMIZED_HORZ: Self::get_atom(&conn, b"_NET_WM_STATE_MAXIMIZED_HORZ").await?,
			WM_STATE_MAXIMIZED_VERT: Self::get_atom(&conn, b"_NET_WM_STATE_MAXIMIZED_VERT").await?,
			WM_STATE_FULLSCREEN:     Self::get_atom(&conn, b"_NET_WM_STATE_FULLSCREEN").await?,
			WM_WINDOW_ROLE:          Self::get_atom(&conn, b"WM_WINDOW_ROLE").await?,
			WM_WINDOW_TYPE:          Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE").await?,
			window_types:            Self::load_window_types(&conn).await?,
		})
	}

	async fn get_atom(conn: &RustConnection, name: &[u8]) -> Result<Atom> {
		Ok(conn.intern_atom(false, name).await?.reply().await?.atom)
	}

	async fn load_window_types(conn: &RustConnection) -> Result<HashMap<u32, WindowType>> {
		Ok(HashMap::from([
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE").await?,               WindowType::Combo),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_DESKTOP").await?,       WindowType::Desktop),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_DIALOG").await?,        WindowType::Dialog),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_DND").await?,           WindowType::DND),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_DOCK").await?,          WindowType::Dock),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_DROPDOWN_MENU").await?, WindowType::DropdownMenu),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_MENU").await?,          WindowType::Menu),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_NORMAL").await?,        WindowType::Normal),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_NOTIFICATION").await?,  WindowType::Notification),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_POPUP_MENU").await?,    WindowType::PopupMenu),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_SPLASH").await?,        WindowType::Splash),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_TOOLBAR").await?,       WindowType::Toolbar),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_TOOLTIP").await?,       WindowType::Tooltip),
			(Self::get_atom(&conn, b"_NET_WM_WINDOW_TYPE_UTILITY").await?,       WindowType::Utility),
		]))
	}
}
