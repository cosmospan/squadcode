#!/bin/sh
# Gives a headless server a desktop you can see from the Squad Code app, and
# starts it. The app runs this over SSH when you press "See its screen".
#
# - A light desktop (Xfce) on a virtual screen (Xvfb), so nothing needs a
#   monitor or a graphics card.
# - x11vnc shares that screen on the server's own 127.0.0.1 only, with a
#   password, so nothing is opened to the internet: the app reaches it through
#   an SSH tunnel.
#
# Installing needs root or sudo without a password; once installed, any login
# can start it. It's safe to run again: what's running is left alone. Lines
# it prints: `say: <progress>`, then `port=<vnc port>` and `pass=<password>`
# last.
set -eu

say() { printf 'say: %s\n' "$*"; }
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }

DISPLAY_NUM="${SQUAD_DISPLAY:-77}"
PORT=$((5900 + DISPLAY_NUM))
GEOM="${SQUAD_GEOMETRY:-1600x1000}"
dir="$HOME/.squadcode/desktop"
mkdir -p "$dir"
chmod 700 "$dir"

[ "$(uname -s)" = Linux ] || fail "this server runs $(uname -s); the desktop needs Linux."

need=""
command -v Xvfb >/dev/null 2>&1 || need="$need xvfb"
command -v x11vnc >/dev/null 2>&1 || need="$need x11vnc"
command -v startxfce4 >/dev/null 2>&1 || need="$need xfce4 xfce4-terminal dbus-x11"

if [ -n "$need" ]; then
  if [ "$(id -u)" = 0 ]; then R=""; elif sudo -n true 2>/dev/null; then R="sudo -n"; else
    fail "installing a desktop needs root, or sudo without a password. Install these once yourself:$need"
  fi
  if command -v apt-get >/dev/null 2>&1; then
    say "Installing a light desktop there (a few minutes the first time)…"
    $R apt-get update -qq >/dev/null 2>&1 || true
    # shellcheck disable=SC2086
    DEBIAN_FRONTEND=noninteractive $R apt-get install -y -qq --no-install-recommends $need >/dev/null 2>&1 \
      || fail "apt-get couldn't install:$need"
  elif command -v dnf >/dev/null 2>&1; then
    say "Installing a light desktop there (a few minutes the first time)…"
    $R dnf install -y -q xorg-x11-server-Xvfb x11vnc @xfce-desktop-environment >/dev/null 2>&1 \
      || fail "dnf couldn't install the desktop"
  else
    fail "no apt-get or dnf here. Install Xvfb, x11vnc and Xfce yourself, then try again."
  fi
fi

# A password for the screen, made once and readable only by you. The tunnel
# already keeps strangers out; this keeps out other logins on the server.
if [ ! -s "$dir/pass" ]; then
  (umask 077; LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c 8 > "$dir/pass")
fi
pass=$(cat "$dir/pass")
x11vnc -storepasswd "$pass" "$dir/vncpass" >/dev/null 2>&1

running() { [ -s "$dir/$1.pid" ] && kill -0 "$(cat "$dir/$1.pid")" 2>/dev/null; }

if ! running xvfb; then
  say "Starting its screen…"
  nohup Xvfb ":$DISPLAY_NUM" -screen 0 "${GEOM}x24" -nolisten tcp >"$dir/xvfb.log" 2>&1 &
  echo $! > "$dir/xvfb.pid"
  sleep 1
fi
if ! running session; then
  say "Starting the desktop…"
  DISPLAY=":$DISPLAY_NUM" nohup dbus-launch --exit-with-session startxfce4 >"$dir/session.log" 2>&1 &
  echo $! > "$dir/session.pid"
  sleep 2
fi
if ! running vnc; then
  nohup x11vnc -display ":$DISPLAY_NUM" -localhost -rfbport "$PORT" -rfbauth "$dir/vncpass" \
    -forever -shared -noxdamage -quiet >"$dir/vnc.log" 2>&1 &
  echo $! > "$dir/vnc.pid"
  sleep 1
fi
running vnc || fail "the screen sharer stopped: $(tail -1 "$dir/vnc.log" 2>/dev/null)"

echo "port=$PORT"
echo "pass=$pass"
