"""Exercise the real Rust libraries with disposable files and child processes.

Usage: python tools/storage-probe/verify.py [--backend sqlite|lancedb]
Data and the JSON report remain under target/storage-probe/runs for inspection.
"""
import argparse
import concurrent.futures
import json
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time


ROOT = Path(__file__).resolve().parents[2]


def request(op, tenant="t1", workspace="w1", id="shared", **extra):
    return dict(op=op, tenant=tenant, workspace=workspace, id=id, **extra)


class Client:
    def __init__(self, args):
        self.errors = tempfile.TemporaryFile(mode="w+t", encoding="utf-8")
        self.proc = subprocess.Popen(
            args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=self.errors, text=True, encoding="utf-8", bufsize=1,
        )
        self.lines = queue.Queue()

        def read():
            for line in self.proc.stdout:
                self.lines.put(line)
            self.lines.put(None)

        self.reader = threading.Thread(target=read, daemon=True)
        self.reader.start()

    def receive(self):
        line = self.lines.get(timeout=30)
        if line is None:
            self.proc.wait(timeout=5)
            self.errors.seek(0)
            raise RuntimeError(self.errors.read())
        return json.loads(line)

    def send(self, value):
        self.proc.stdin.write(json.dumps(value) + "\n")
        self.proc.stdin.flush()
        return self.receive()

    def kill(self):
        if self.proc.poll() is None:
            self.proc.kill()
        self.proc.wait(timeout=10)

    def close(self):
        self.kill()
        self.reader.join(timeout=5)
        self.proc.stdin.close()
        self.proc.stdout.close()
        self.errors.close()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


class Probe:
    def __init__(self, binary, backend, directory):
        self.binary, self.backend = binary, backend
        self.path = directory / (backend + (".db" if backend == "sqlite" else ""))
        self.results = []

    def args(self, value):
        return [str(self.binary), self.backend, str(self.path), json.dumps(value)]

    def run(self, value, reject=False):
        result = subprocess.run(self.args(value), capture_output=True, text=True,
                                encoding="utf-8", timeout=30)
        if reject:
            assert result.returncode != 0, "a conflicting process unexpectedly opened/wrote"
            return result.stderr.strip()
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout.strip())

    def record(self, name, **details):
        entry = dict(backend=self.backend, check=name, passed=True, **details)
        self.results.append(entry)
        print(json.dumps(entry, ensure_ascii=False), flush=True)

    def expect(self, value, expected):
        actual = self.run(value)["values"]
        assert actual == expected, (value, actual, expected)

    def basic(self):
        self.run(dict(op="init"))
        self.run(dict(op="init"))
        scopes = [("t1", "w1", 11), ("t1", "w2", 22),
                  ("t2", "w1", 33), ("t'3", "w'3", 44)]
        for tenant, workspace, value in scopes:
            self.run(request("put", tenant, workspace, value=value))
        for tenant, workspace, value in scopes:
            self.expect(request("get", tenant, workspace), [value])
            self.expect(request("search", tenant, workspace), [value])
        self.run(request("delete"))
        self.expect(request("get"), [])
        for tenant, workspace, value in scopes[1:]:
            self.expect(request("get", tenant, workspace), [value])
        self.run(request("put", value=11))
        self.record("repeat_init_scoped_read_search_delete_reopen")

    def concurrent_files(self):
        with Client(self.args(dict(op="serve"))) as owner:
            assert owner.receive()["ready"]
            assert owner.send(request("get"))["values"] == [11]
            self.run(request("put", id="fresh", value=99))
            assert owner.send(request("get", id="fresh"))["values"] == [99]
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                writes = [pool.submit(self.run, request("put", id=f"parallel{i}", value=i))
                          for i in range(2)]
                for write in writes:
                    write.result()
            for i in range(2):
                assert owner.send(request("get", id=f"parallel{i}"))["values"] == [i]
        self.record("two_process_writes_and_long_lived_reader_refresh")

    def acknowledged_crash(self):
        with Client(self.args(dict(op="serve"))) as owner:
            assert owner.receive()["ready"]
            assert owner.send(request("put", id="committed", value=55))["ok"]
            owner.kill()
        self.expect(request("get", id="committed"), [55])
        self.record("acknowledged_write_survives_process_kill")

    def sqlite_lock(self):
        with Client(self.args(request("hold-write", value=77))) as writer:
            assert writer.receive()["uncommitted"]
            self.expect(request("get"), [11])
            error = self.run(request("put", value=88), reject=True)
            assert "locked" in error.lower(), error
            writer.kill()
        self.expect(request("get"), [11])
        self.run(request("put", value=12))
        self.expect(request("get"), [12])
        self.record("wal_reader_writer_contention_and_uncommitted_crash", contention=error)



def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--backend", choices=["sqlite", "lancedb"])
    parser.add_argument("--binary", type=Path,
                        default=ROOT / "target/storage-probe/debug/origence-storage-probe.exe")
    args = parser.parse_args()
    parent = ROOT / "target/storage-probe/runs"
    parent.mkdir(parents=True, exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="run-", dir=parent))
    report = dict(started=time.strftime("%Y-%m-%dT%H:%M:%S%z"), binary=str(args.binary.resolve()),
                  directory=str(directory))
    print(f"Report/data: {directory}", flush=True)
    try:
        for backend in ([args.backend] if args.backend else ["sqlite", "lancedb"]):
            probe = Probe(args.binary.resolve(), backend, directory)
            # Preserve completed checks even when a subsequent check fails.
            report[backend] = probe.results
            probe.basic()
            if backend in ("sqlite", "lancedb"):
                probe.concurrent_files()
            probe.acknowledged_crash()
            if backend == "sqlite":
                probe.sqlite_lock()
        report["passed"] = True
    except Exception as error:
        report["passed"] = False
        report["error"] = repr(error)
        raise
    finally:
        (directory / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2),
                                               encoding="utf-8")


if __name__ == "__main__":
    main()
