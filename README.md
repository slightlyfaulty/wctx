# wctx

A simple Linux CLI tool and D-Bus service to provide real-time information about the current **active** window (focused window) or **pointer** window (under the mouse cursor) on Wayland and X11.

`wctx` consists of two components:

1. A userspace daemon that interfaces with the desktop environment
2. A CLI client for querying window details from the daemon in various formats

### Currently supported desktop environments

- X11
- KDE 6
- GNOME 45+

See [issues](https://github.com/slightlyfaulty/wctx/issues?q=is%3Aissue%20state%3Aopen%20label%3A%22desktop%20support%22) for status of support for other desktop environments.

## Installation

A [wctx](https://aur.archlinux.org/packages/wctx) package is available for Arch Linux in the AUR:

```bash
yay -S wctx
```

### Manual installation

The easiest way to build and install wctx is with the provided install script. Make sure you already have `rust` and `cargo` installed.

```bash
git clone https://github.com/slightlyfaulty/wctx
cd wctx
sh install.sh
```

This will:

1. Build the binary using cargo
2. Install it to `/usr/bin/wctx`
3. Install the systemd service file to `/usr/lib/systemd/user/wctx.service`
4. Optionally enable and start the daemon service

If you prefer to install it yourself:

```bash
# Build the binary
cargo build --release

# Copy the binary to a location in your PATH
sudo cp target/release/wctx /usr/bin/

# Copy the systemd service file
sudo cp wctx.service /usr/lib/systemd/user/

# Enable and start the service
systemctl --user enable --now wctx
```

## Usage

```bash
wctx <CONTEXT> [PROPERTY] [OPTIONS]
```

### Basic Commands

Query active window information in JSON format:

```bash
wctx active -f json
```

Query a specific property of the pointer window:

```bash
wctx pointer title
```

Monitor the pointer window:

```bash
wctx pointer --watch
```

### Window Contexts

- `active`: Currently focused window
- `pointer`: Window under the mouse cursor

### Window Properties

|             | Type           | Example Value          |
|-------------|----------------|------------------------|
| **id**      | `string`       | 182452228              |
| **name**    | `string`       | google-chrome          |
| **class**   | `string`       | google-chrome          |
| **pid**     | `integer`      | 152479                 |
| **title**   | `string`       | Google - Google Chrome |
| **type**    | `window type`  | NORMAL                 |
| **role**    | `string`       | browser                |
| **state**   | `window state` | MAXIMIZED              |
| **display** | `string`       | DisplayPort-1          |

Note that some property values will differ between desktop environments.

### Output Formats

Use the `-f` or `--format` option to specify the output format:

- `flat` (default)
- `dict`
- `json`
- `toml`
- `csv`

Example:

```bash
wctx pointer -f dict
```

### Running the Daemon

The daemon should typically be managed through systemd:

```bash
systemctl --user enable --now wctx
```

But you can also run it manually:

```bash
wctx daemon

# or specify the window provider explicitly
wctx daemon --provider kwin
```

## Contributing

Contributions are welcome! Please feel free to submit bug reports or pull requests.
