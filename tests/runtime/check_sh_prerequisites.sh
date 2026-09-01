#!/usr/bin/env sh
set -eu

repository_root="$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)"
fixture_root="$repository_root/tests/fixtures/check_sh"
test_workspace="$(mktemp -d)"
fixture_bin="$test_workspace/bin"
test_output="$test_workspace/output"
mkdir "$fixture_bin"
ln -s "$fixture_root/fake_python3.sh" "$fixture_bin/python3"
ln -s "$fixture_root/fake_rustup.sh" "$fixture_bin/rustup"
trap 'rm -f -- "$test_output" "$fixture_bin/python3" "$fixture_bin/rustup"; rmdir -- "$fixture_bin" "$test_workspace"' EXIT HUP INT TERM

run_deep_preflight() {
  rustup_case="$1"
  expected_status="$2"
  expected_output="$3"

  set +e
  CHECK_SH_RUSTUP_CASE="$rustup_case" \
    PATH="$fixture_bin:$PATH" \
    sh "$repository_root/check.sh" --deep >"$test_output" 2>&1
  actual_status=$?
  set -e

  if [ "$actual_status" -ne "$expected_status" ]; then
    echo "Expected status $expected_status for $rustup_case, got $actual_status." >&2
    sed -n '1,80p' "$test_output" >&2
    exit 1
  fi
  if ! grep -Fq "$expected_output" "$test_output"; then
    echo "Expected output for $rustup_case: $expected_output" >&2
    sed -n '1,80p' "$test_output" >&2
    exit 1
  fi
}

# The target-independent rust-src name must pass prerequisite validation. The
# fixture python status proves the script continued into the stable gates.
run_deep_preflight complete 23 "Running documentation structure and link check..."

# A dated nightly is not the +nightly alias used by every deep command.
run_deep_preflight dated-only 1 "The deep profile requires the nightly Rust toolchain."

run_deep_preflight missing-rust-src 1 \
  "The deep profile requires nightly rust-src for instrumented standard-library builds."

echo "check.sh deep-profile prerequisite tests passed."
