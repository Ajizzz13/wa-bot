#!/data/data/com.termux/files/usr/bin/bash
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
    if [ "$(id -u)" -eq 0 ]; then
        printf '%s\n' 'Rust is missing. Run this script from the normal Termux user; pkg cannot run as root.' >&2
        exit 1
    fi
    pkg install -y rust
fi

cargo build --release
install -Dm755 target/release/wa-bot "$PREFIX/bin/wa-bot"

printf 'Installed %s\n' "$PREFIX/bin/wa-bot"
