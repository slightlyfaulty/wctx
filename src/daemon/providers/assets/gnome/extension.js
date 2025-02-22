/*
 * Window Context (wctx) extension for GNOME
 * Copyright 2025 Saul Fautley (https://github.com/slightlyfaulty)
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 2 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program. If not, see <http://www.gnu.org/licenses/>.
 */

import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js'
import Gio from 'gi://Gio'
import GLib from 'gi://GLib'

const WINDOW_TYPES = [
	'NORMAL',
	'DESKTOP',
	'DOCK',
	'DIALOG',
	'DIALOG', // MODAL_DIALOG
	'TOOLBAR',
	'MENU',
	'UTILITY',
	'SPLASH', // SPLASHSCREEN
	'DROPDOWN_MENU',
	'POPUP_MENU',
	'TOOLTIP',
	'NOTIFICATION',
	'COMBO',
	'DND',
	'OVERRIDE', // OVERRIDE_OTHER
]

export default class WctxExtension extends Extension {
	dbus = null
	windows = null
	signals = new Map()
	timeout = null

	enable() {
		Gio.DBusProxy.new_for_bus(
			Gio.BusType.SESSION,
			Gio.DBusProxyFlags.DO_NOT_LOAD_PROPERTIES
			| Gio.DBusProxyFlags.DO_NOT_CONNECT_SIGNALS,
			null,
			'org.wctx',
			'/',
			'org.wctx.Windows',
			null,
			(proxy) => {
				if (proxy) {
					this.dbus = proxy
					this.start()
				}
			},
		)
	}

	disable() {
		for (const [object, signals] of this.signals) {
			if (!object) continue

			for (const signal of signals) {
				object.disconnect(signal)
			}
		}

		if (this.timeout) {
			clearTimeout(this.timeout)
		}

		this.dbus = null
		this.windows = null
		this.signals.clear()
		this.timeout = null
	}

	connectSignal(object, signal, callback) {
		let handlers = this.signals.get(object) || []
		handlers.push(object.connect(signal, callback))
		this.signals.set(object, handlers)
	}

	start() {
		this.windows = {
			active: {},
			pointer: {},
		}

		for (let actor of global.get_window_actors()) {
			const content = this.getActorContent(actor)
			if (!content) continue

			let meta = actor.meta_window

			this.watchWindow(meta, actor, content)

			if (meta.has_focus()) {
				this.setWindow('active', meta)
			}

			if (content.has_pointer) {
				this.setWindow('pointer', meta)
			}
		}

		this.connectSignal(global.window_manager, 'map', (wm, actor) => {
			this.watchWindow(actor.meta_window, actor)
		})

		this.connectSignal(global.display, 'focus-window', (display, meta) => {
			if (meta && meta.get_wm_class()) {
				this.setWindow('active', meta)
			}
		})

		this.connectSignal(global.display, 'window-entered-monitor', (display, monitor, meta) => {
			this.updateWindow(meta, 'display', monitor.toString())
		})
	}

	watchWindow(meta, actor, content) {
		if (!meta || !actor) {
			return
		}

		if (!content) {
			content = this.getActorContent(actor)
			if (!content) return
		}

		if (this.signals.get(meta)) {
			return
		}

		this.connectSignal(content, 'leave-event', () => {
			this.setWindow('pointer')
		})

		this.connectSignal(content, 'enter-event', () => {
			this.setWindow('pointer', meta)
		})

		this.connectSignal(meta, 'notify::title', () => {
			this.updateWindow(meta, 'title', meta.title)
		})

		this.connectSignal(meta, 'notify::wm-class', () => {
			this.updateWindow(meta, 'name', meta.get_wm_class())
			this.updateWindow(meta, 'class', meta.get_wm_class_instance())
		})

		this.connectSignal(meta, 'notify::window-type', () => {
			this.updateWindow(meta, 'type', this.getWindowType(meta))
		})

		this.connectSignal(meta, 'notify::fullscreen', () => {
			this.updateWindow(meta, 'state', this.getWindowState(meta))
		})

		this.connectSignal(meta, 'notify::maximized-horizontally', () => {
			this.updateWindow(meta, 'state', this.getWindowState(meta))
		})

		this.connectSignal(meta, 'notify::maximized-vertically', () => {
			this.updateWindow(meta, 'state', this.getWindowState(meta))
		})

		this.connectSignal(actor, 'destroy', () => {
			for (const object of [meta, actor, content]) {
				const signals = this.signals.get(object)
				if (!signals) continue;

				for (const signal of signals) {
					object.disconnect(signal)
				}

				this.signals.delete(object)
			}
		})
	}

	setWindow(context, meta) {
		if (meta === this.windows[context].meta) {
			return
		}

		this.windows[context] = meta ? this.getWindowData(meta) : {}

		if (context === 'pointer' && !this.windows[context].meta) {
			if (this.timeout) {
				clearTimeout(this.timeout)
			}

			this.timeout = setTimeout(() => {
				if (!this.windows[context].meta) {
					this.sendWindow(context)
				}
			}, 50)
		} else {
			this.sendWindow(context)
		}
	}

	sendWindow(context) {
		const window = this.windows[context]

		const data = {
			id: GLib.Variant.new_string(window.id || ''),
			name: GLib.Variant.new_string(window.name || ''),
			class: GLib.Variant.new_string(window.class || ''),
			pid: GLib.Variant.new_int32(window.pid || 0),
			title: GLib.Variant.new_string(window.title || ''),
			type: GLib.Variant.new_string(window.type || ''),
			role: GLib.Variant.new_string(window.role || ''),
			state: GLib.Variant.new_string(window.state || ''),
			display: GLib.Variant.new_string(window.display || ''),
		};

		this.dbus.call(
			'SetWindow',
			GLib.Variant.new_tuple([
				GLib.Variant.new_string(context),
				new GLib.Variant('a{sv}', data),
			]),
			Gio.DBusCallFlags.NONE,
			-1,
			null,
			null,
		)
	}

	updateWindow(meta, key, value) {
		let window, context

		if (meta === this.windows.active.meta && meta === this.windows.pointer.meta) {
			window = this.windows.active
			context = 'both'
		} else if (meta === this.windows.active.meta) {
			window = this.windows.active
			context = 'active'
		} else if (meta === this.windows.pointer.meta) {
			window = this.windows.pointer
			context = 'pointer'
		} else {
			return
		}

		if (window[key] === value) {
			return
		}

		window[key] = value

		this.dbus.call(
			'UpdateWindow',
			GLib.Variant.new_tuple([
				GLib.Variant.new_string(context),
				GLib.Variant.new_string(key),
				GLib.Variant.new_string(value),
			]),
			Gio.DBusCallFlags.NONE,
			-1,
			null,
			null,
		)
	}

	getActorContent(actor) {
		let content = actor

		while (content && !content.reactive) {
			content = content.first_child
		}

		return content
	}

	getWindowData(meta) {
		return {
			meta,
			id: meta.get_id().toString(),
			name: meta.get_wm_class(),
			class: meta.get_wm_class_instance(),
			pid: meta.get_pid(),
			title: meta.title,
			type: this.getWindowType(meta),
			role: meta.get_role() || '',
			state: this.getWindowState(meta),
			display: meta.get_monitor().toString(),
		}
	}

	getWindowType(meta) {
		return WINDOW_TYPES[meta.window_type] || WINDOW_TYPES[0]
	}

	getWindowState(meta) {
		let state = 'NORMAL'

		if (meta.fullscreen) {
			state = 'FULLSCREEN'
		} else if (meta.maximized_horizontally && meta.maximized_vertically) {
			state = 'MAXIMIZED'
		}

		return state
	}
}
