# Termux

The prebuilt ARM64 Termux artifact targets `aarch64-linux-android` (Android ARM64/Bionic) and requires Android 7.0 / API 24 or newer: https://tmpfiles.org/wlAiJITgmW1y/wa-bot-termux-aarch64.tar.gz

The Linux x86_64 release does not run on Termux.

```sh
pkg update && pkg upgrade
pkg install -y git

git clone <repository-url> wa-bot
cd wa-bot
chmod +x build-termux.sh
./build-termux.sh
```

The script installs Rust when missing, compiles the release binary for the current Termux architecture, then installs it at `$PREFIX/bin/wa-bot`.

Run it with an existing WhatsApp session database:

```sh
WA_DB="$HOME/whatsapp.db" wa-bot
```

Pair a new session by adding your WhatsApp number:

```sh
WA_DB="$HOME/whatsapp.db" wa-bot --phone 628123456789
```

`whatsapp.db` contains session credentials. Keep it private and back it up before reinstalling Termux.
