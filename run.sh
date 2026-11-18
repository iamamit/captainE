#!/usr/bin/env bash
# One command to install what captainE needs (first time only) and start it.
#   ./run.sh                 open the editor (optionally: ./run.sh my-project.json)
#   ./run.sh cli <args>      the command line, e.g. ./run.sh cli --help
set -euo pipefail
cd "$(dirname "$0")"

say() { printf '\033[1;35m==>\033[0m %s\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }
os="$(uname -s)"

# --- Rust ---------------------------------------------------------------------------------------------------
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
if ! have cargo; then
  say "Installing Rust (rustup, current user only)"
  have curl || { echo "curl is needed to install Rust"; exit 1; }
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  . "$HOME/.cargo/env"
fi

# --- FFmpeg and system libraries -----------------------------------------------------------------------------
case "$os" in
  Darwin)
    if ! have ffmpeg || ! have ffprobe; then
      if ! have brew; then
        say "Installing Homebrew (needed for FFmpeg; it may ask for your password)"
        /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
        [ -x /opt/homebrew/bin/brew ] && eval "$(/opt/homebrew/bin/brew shellenv)"
        [ -x /usr/local/bin/brew ] && eval "$(/usr/local/bin/brew shellenv)"
      fi
      say "Installing FFmpeg"
      brew install ffmpeg
    fi
    ;;
  Linux)
    if ! have ffmpeg || ! have pkg-config || [ ! -f "$HOME/.cache/captaine-deps-ok" ]; then
      if have apt-get; then
        say "Installing FFmpeg and window/audio libraries (apt, needs sudo)"
        sudo apt-get update -qq
        sudo apt-get install -y ffmpeg pkg-config libssl-dev libasound2-dev libxkbcommon-dev libwayland-dev \
          libx11-dev libxcursor-dev libxi-dev libxrandr-dev libgl1-mesa-dev
      elif have dnf; then
        say "Installing FFmpeg and libraries (dnf, needs sudo)"
        sudo dnf install -y ffmpeg pkgconf-pkg-config openssl-devel alsa-lib-devel libxkbcommon-devel \
          wayland-devel libX11-devel libXcursor-devel libXi-devel libXrandr-devel mesa-libGL-devel
      elif have pacman; then
        say "Installing FFmpeg and libraries (pacman, needs sudo)"
        sudo pacman -S --needed --noconfirm ffmpeg pkgconf openssl alsa-lib libxkbcommon wayland libx11 \
          libxcursor libxi libxrandr mesa
      else
        echo "Install FFmpeg with your package manager, then run this again."; exit 1
      fi
      mkdir -p "$HOME/.cache" && touch "$HOME/.cache/captaine-deps-ok"
    fi
    ;;
  MINGW*|MSYS*|CYGWIN*)
    if ! have ffmpeg; then
      if have winget; then say "Installing FFmpeg (winget)"; winget install -e --id Gyan.FFmpeg
      elif have choco; then say "Installing FFmpeg (choco)"; choco install ffmpeg -y
      else echo "Install FFmpeg (https://ffmpeg.org/download.html), then run this again."; exit 1; fi
      echo "Open a new terminal so FFmpeg is on PATH, then run this again."; exit 0
    fi
    ;;
esac

# --- build (instant when nothing changed) and start ----------------------------------------------------------
if [ "${1:-}" = "cli" ]; then
  shift
  cargo build --release -q -p captaine-cli
  exec target/release/captaine "$@"
fi
say "Building captainE (the first build takes a few minutes)"
cargo build --release -q -p captaine-app
say "Starting captainE"
exec target/release/captaine-app "$@"
