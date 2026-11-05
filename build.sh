#!/usr/bin/env bash
set -euo pipefail
. "$HOME/.cargo/env"
cd /content/wa-bot

python3 -u - << 'EOF'
import subprocess, time, os, sys

env = os.environ.copy()
env["PATH"] = os.path.expanduser("~/.cargo/bin:") + env.get("PATH", "")

proc = subprocess.Popen(["cargo", "+nightly", "build", "--release"], cwd="/content/wa-bot", env=env)
while proc.poll() is None:
    sys.stdout.write(". ")
    sys.stdout.flush()
    time.sleep(5)

print("\nBUILD FINISHED:", proc.returncode)
sys.exit(proc.returncode)
EOF

ls -lh target/release/wa-bot
