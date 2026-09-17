"""Real Windows source I/O regression; optional large local Git source, isolated runtime.

Writes only a newly created output directory. Never resets tasks or mutates the source.
"""
import argparse
import csv
import ctypes
import io
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time
from contextlib import contextmanager
from ctypes import wintypes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source", type=Path, help="Existing local Git repository to clone")
    parser.add_argument("--timeout", type=int, default=1800)
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("this harness requires Windows file sharing and ACLs")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    repo = output / "repo"
    binary = args.binary.resolve()
    env = {k: v for k, v in os.environ.items() if not k.startswith("RELAY_KNOWLEDGE_")}
    env["RELAY_KNOWLEDGE_HOME"] = str(output / "runtime")
    metrics = []

    def run(label, command):
        started = time.monotonic()
        completed = subprocess.run([str(a) for a in command], cwd=repo if repo.exists() else output,
                                   env=env, capture_output=True, timeout=args.timeout)
        (output / (label + ".stdout.json")).write_bytes(completed.stdout)
        (output / (label + ".stderr.txt")).write_bytes(completed.stderr)
        metric = {"step": label, "seconds": round(time.monotonic() - started, 3), "exit": completed.returncode}
        metrics.append(metric)
        (output / "metrics.json").write_text(json.dumps(metrics, indent=2), encoding="utf-8")
        print(json.dumps(metric), flush=True)
        assert completed.returncode == 0, completed.stderr.decode("utf-8", "replace")
        return completed.stdout

    def cli(label, *command):
        return json.loads(run(label, [binary, "repo", *command, "--format", "json"]))

    if args.source:
        run("clone", ["git", "-c", "core.longpaths=true", "clone", "--local", "--no-hardlinks", args.source.resolve(), repo])
    else:
        repo.mkdir()
        (repo / "src").mkdir()
        for index in range(128):
            (repo / "src" / (f"Source{index}.java")).write_text(f"class Source{index} {{ int value() {{ return {index}; }} }}", encoding="utf-8")
        run("init", ["git", "init"])
        run("add", ["git", "add", "."])
        run("commit", ["git", "-c", "user.name=Source IO", "-c", "user.email=io@example.invalid", "commit", "-m", "Add isolated source fixture"])
    paths = subprocess.check_output(["git", "ls-files", "-z"], cwd=repo).decode("utf-8").split("\0")
    chosen = next(path for path in paths if path.endswith(".java"))
    target = repo / chosen
    original = target.read_bytes()
    cli("register", "register", repo, "--alias", "source-io")
    baseline = cli("baseline", "index", "source-io", "--ref", "HEAD")
    baseline_count = baseline["summary"]["indexed_file_count"]
    target.write_bytes(original + b"\n// Source I/O isolation regression.\n")
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    kernel.CreateFileW.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    sid = next(csv.reader(io.StringIO(subprocess.check_output(["whoami", "/user", "/fo", "csv", "/nh"]).decode(errors="replace"))))[1]

    @contextmanager
    def fault(kind):
        if kind == "sharing":
            handle = kernel.CreateFileW(str(target), 0x80000000, 0, None, 3, 0x80, None)
            assert handle != ctypes.c_void_p(-1).value, ctypes.get_last_error()
            try:
                yield 32
            finally:
                assert kernel.CloseHandle(handle)
        else:
            assert target.resolve().is_relative_to(repo.resolve())
            run("deny", ["icacls", target, "/deny", "*" + sid + ":(R)"])
            try:
                yield 5
            finally:
                run("restore-acl", ["icacls", target, "/remove:d", "*" + sid])

    for kind in ["sharing", "acl"]:
        with fault(kind) as code:
            probe = kernel.CreateFileW(str(target), 0x80000000, 7, None, 3, 0x80, None)
            observed = ctypes.get_last_error()
            if probe != ctypes.c_void_p(-1).value:
                kernel.CloseHandle(probe)
                raise AssertionError("real operating-system read failure was not observed")
            assert observed == code, observed
            partial = cli(kind, "index", "source-io", "--ref", "worktree")
            summary = partial["summary"]
            assert summary["indexed_file_count"] == baseline_count - 1, summary
            assert summary["progress"]["io_skipped_file_count"] == 1, summary
            assert summary["deleted_path_count"] == 0, summary
            diagnostics = cli(kind + "-diagnostics", "diagnostics", "source-io", "--ref", summary["resolved_commit_sha"], "--path", chosen)
            diagnostic = next(d for d in diagnostics["diagnostics"] if d.get("io"))
            assert diagnostic["io"]["raw_os_error"] == code, diagnostic
            assert diagnostics["content_integrity"]["state"] == "partial", diagnostics
            assert not diagnostics["scope"]["stale"], diagnostics
        restored = cli(kind + "-repaired", "index", "source-io", "--ref", "worktree")
        assert restored["summary"]["indexed_file_count"] == baseline_count
        assert restored["summary"]["progress"]["io_skipped_file_count"] == 0
        db = output / "runtime/data/relay-knowledge.sqlite"
        with sqlite3.connect(db.as_uri() + "?mode=ro", uri=True) as connection:
            tasks = connection.execute("SELECT state, attempt_count, last_error_message FROM code_repository_index_tasks").fetchall()
            assert all(row[0] == "succeeded" and row[1] == 1 for row in tasks), tasks
            (output / (kind + "-tasks.json")).write_text(json.dumps(tasks, indent=2), encoding="utf-8")
    print("source_io: sharing=32 acl=5 partial+repair succeeded; os error 1 is covered only by deterministic unit tests", flush=True)


if __name__ == "__main__":
    main()
