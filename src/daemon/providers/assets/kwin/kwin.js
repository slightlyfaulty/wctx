/* global workspace, QTimer, callDBus */

const WINDOW_TYPES = [
	'NORMAL',
	'DESKTOP',
	'DOCK',
	'TOOLBAR',
	'MENU',
	'DIALOG',
	'OVERRIDE',
	'DROPDOWN_MENU', // TopMenu
	'UTILITY',
	'SPLASH',
	'DROPDOWN_MENU',
	'POPUP_MENU',
	'TOOLTIP',
	'NOTIFICATION',
	'COMBO',
	'DND',
	'UTILITY', // OnScreenDisplay
	'NOTIFICATION', // CriticalNotification
	'UTILITY', // AppletPopup
]

const windows = {
	active: null,
	pointer: null,
}

checkActiveWindow()
checkPointerWindow()

workspace.windowList().forEach(addWindowListeners)
workspace.windowAdded.connect(addWindowListeners)
workspace.windowActivated.connect(checkActiveWindow)

// poll for pointer window until kwin scripts can add a window enter signal
const timer = new QTimer()
timer.interval = 50
timer.timeout.connect(checkPointerWindow)
timer.start()

function checkActiveWindow() {
	sendWindow('active', workspace.activeWindow)
}

function checkPointerWindow() {
	// this is very fast on Wayland, not so much on X11 but if you're running X11 just use the X11 provider
	sendWindow('pointer', workspace.windowAt(workspace.cursorPos)[0])
}

function addWindowListeners(window) {
	window.windowClassChanged.connect(() => updateWindow(window, 'class'))
	window.captionChanged.connect(() => updateWindow(window, 'title'))
	window.windowRoleChanged.connect(() => updateWindow(window, 'role'))
	window.fullScreenChanged.connect(() => updateWindow(window, 'state'))
	window.outputChanged.connect(() => updateWindow(window, 'display'))

	// KDE 6.3.1+
	window.maximizedChanged && window.maximizedChanged.connect(() => updateWindow(window, 'state'))
}

function getWindowData(window, key) {
	if (!windows) return

	if (key === undefined) {
		return {
			id: window.internalId.toString().slice(1, 9), // first part of uuid
			name: window.resourceName,
			class: window.resourceClass,
			pid: window.pid,
			title: window.caption,
			type: getWindowType(window),
			role: window.windowRole,
			state: getWindowState(window),
			display: window.output.name,
		}
	} else {
		// changeable properties
		switch (key) {
			case 'class': return window.resourceClass
			case 'title': return window.caption
			case 'role': return window.windowRole
			case 'state': return getWindowState(window)
			case 'display': return window.output.name
		}
	}
}

function sendWindow(ctx, window) {
	if (!window || window === windows[ctx]) {
		return
	}

	windows[ctx] = window

	const dict = getWindowData(window)

	callDBus('org.wctx', '/', 'org.wctx.Windows', 'SetWindow', ctx, dict)
}

function updateWindow(window, key) {
	let context

	if (window === windows.active && window === windows.pointer) {
		context = 'both'
	} else if (window === windows.active) {
		context = 'active'
	} else if (window === windows.pointer) {
		context = 'pointer'
	} else {
		return
	}

	const value = getWindowData(window, key)
	if (value == null) return

	callDBus('org.wctx', '/', 'org.wctx.Windows', 'UpdateWindow', context, key, value)
}

function getWindowType(window) {
	return WINDOW_TYPES[window.windowType] || WINDOW_TYPES[0]
}

function getWindowState(window) {
	if (window.fullScreen) {
		return 'FULLSCREEN'
	/*} else if (window.maximizeMode === 1) {
		return 'VERTICAL'
	} else if (window.maximizeMode === 2) {
		return 'HORIZONTAL'*/
	} else if (window.maximizeMode === 3) {
		return 'MAXIMIZED'
	} else {
		return 'NORMAL'
	}
}
