#!/bin/bash
# Rebuild and reopen captainE whenever a source file is saved: tools/dev.sh [project.json]
# (Rust cannot swap code into a running window, so this restarts it; the project reopens where you left it.)
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/.."
stamp() { find crates -name '*.rs' -o -name 'Cargo.toml' -o -name '*.py' | xargs stat -f %m | sort -n | tail -1; }
pid=""
trap '[ -n "$pid" ] && kill $pid 2>/dev/null; rm -f "$HOME/.captaine/running"; exit 0' INT TERM
while true; do
  last=$(stamp)
  if cargo build --release -p captaine-app 2>&1 | grep -E "^(error|warning: unused)" -A6; [ "${PIPESTATUS[0]}" -eq 0 ]; then
    target/release/captaine-app "$@" & pid=$!
    echo "== running (save a file to reload) =="
  else
    echo "== build failed; fix it and save =="; pid=""
  fi
  while [ "$(stamp)" = "$last" ]; do sleep 1; done
  [ -n "$pid" ] && kill $pid 2>/dev/null && wait $pid 2>/dev/null
  rm -f "$HOME/.captaine/running"   # a restart is not a crash
  echo "== change detected, rebuilding =="
done
