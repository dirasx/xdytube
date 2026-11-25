#!/bin/bash
set -euo pipefail

# Test an exact committed working version without opening the application.
cd "$(dirname "$0")/.."
project_root="$PWD"
version="${1:?Usage: bash scripts/test-working-version.sh v0.1.N}"
[[ "$version" =~ ^v0\.1\.[0-9]+$ ]] || { echo "Invalid working version" >&2; exit 1; }
commit=$(git log --all --diff-filter=A --format=%H -- "versions/$version/README.md" | tail -1)
[[ -n "$commit" ]] || { echo "Working version not found: $version" >&2; exit 1; }
source_dir=$(mktemp -d "${TMPDIR:-/tmp}/xdytube-headless-test.XXXXXXXX")
trap 'rm -rf "$source_dir"' EXIT
git archive "$commit" | tar -x -C "$source_dir"
export STREAMTUBE_DATA_DIR="$source_dir/profile"
# Separate Cargo targets prevent a cached binary from another checkout being
# mistaken for this commit when archives contain old file timestamps.
export CARGO_TARGET_DIR="${XDYTUBE_TEST_TARGET_DIR:-${TMPDIR:-/tmp}/xdytube-test-targets}/$commit"
printf 'Version: %s\nCommit: %s\nRunning automated tests without an application window.\n' "$version" "$commit"
cd "$source_dir"
set +e
cargo test --release --locked 2>&1 | tee "$source_dir/results.log"
result=${PIPESTATUS[0]}
set -e
mkdir -p "$project_root/dashboard/test-results"
python3 - "$version" "$commit" "$result" "$source_dir/results.log" "$project_root/dashboard/test-results/$version.json" <<'PY'
import datetime, json, pathlib, re, sys
version, commit, result, log, output = sys.argv[1:]
text = pathlib.Path(log).read_text(errors="replace")
matches = re.findall(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored', text)
report = {"version": version, "commit": commit, "passed": int(matches[-1][1]) if matches else 0,
          "failed": int(matches[-1][2]) if matches else 0, "ignored": int(matches[-1][3]) if matches else 0,
          "status": "passed" if result == "0" and matches and matches[-1][0] == "ok" else "failed",
          "checked_at": datetime.datetime.now().isoformat(timespec="seconds"),
          "scope": "Автотесты; аккаунт и нативный интерфейс отдельно не проверены"}
pathlib.Path(output).write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
PY
if [[ -f "$project_root/dashboard/update.py" ]]; then python3 "$project_root/dashboard/update.py"; fi
printf '\nAutomated tests only. Real account and native visual interactions require separate verification.\n'
exit "$result"
