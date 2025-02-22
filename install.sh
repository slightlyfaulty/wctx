#!/bin/bash

set -e

echo "Building wctx..."
cargo build --release

echo "Installing binary..."
sudo install -Dm755 target/release/wctx /usr/bin/wctx

echo "Installing systemd service..."
sudo install -Dm644 wctx.service /usr/lib/systemd/user/wctx.service

echo "Installation complete!"

read -p "Would you like to enable and start the wctx service? [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "Enabling and starting wctx service..."
    systemctl --user enable --now wctx
    echo "Service enabled and started!"
else
    echo "To enable and start the wctx service later, run:"
    echo "systemctl --user enable --now wctx"
fi
