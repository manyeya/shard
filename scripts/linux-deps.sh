#!/bin/sh
# The system libraries Shard needs to build on Debian or Ubuntu: what GPUI links (xkbcommon, xcb,
# fontconfig, freetype). Wayland and Vulkan are loaded at run time, so only the .deb asks for them.
set -eu
sudo apt-get update
sudo apt-get install -y --no-install-recommends pkg-config libxkbcommon-dev libxkbcommon-x11-dev \
	libxcb1-dev libx11-xcb-dev libwayland-dev libfontconfig-dev libfreetype-dev
