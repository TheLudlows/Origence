"""Linux synthetic-child syscall fault tests; never exhaust a real filesystem."""
import errno
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest


WRITER = r'''
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <sys/uio.h>
#include <unistd.h>
int main(int argc, char **argv) {
    if (argc != 5) return 10;
    int fd = open(argv[1], O_WRONLY | O_CREAT | O_TRUNC, 0600);
    int other = open(argv[2], O_WRONLY | O_CREAT | O_TRUNC, 0600);
    int outside = open(argv[3], O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (fd < 0 || other < 0 || outside < 0) return 11;
    if (write(other, "ok", 2) != 2 || write(outside, "ok", 2) != 2) return 12;
    for (int i = 0; i < 3; i++) {
        errno = 0;
        struct iovec v = {.iov_base = "x", .iov_len = 1};
        size_t count = strstr(argv[4], "zero") && i == 1 ? 0 : 1;
        struct iovec zero_v[2] = {{.iov_base = "x", .iov_len = 0}, {.iov_base = "x", .iov_len = 0}};
        struct iovec multi[2] = {v, v};
        struct iovec overflow[2] = {{.iov_base = "x", .iov_len = SIZE_MAX}, v};
        ssize_t n = !strcmp(argv[4], "multi-writev") ? writev(fd, multi, 2) :
                    !strcmp(argv[4], "overflow-writev") && i == 1 ? writev(fd, overflow, 2) :
                    !strcmp(argv[4], "zero-writev") ? writev(fd, count ? &v : zero_v, count ? 1 : 2) :
                    !strcmp(argv[4], "zero-pwrite") ? pwrite(fd, "x", count, i) :
                    !strcmp(argv[4], "zero-write") ? write(fd, "x", count) :
                    !strcmp(argv[4], "pwrite") ? pwrite(fd, "x", 1, i) :
                    !strcmp(argv[4], "pwrite64") ? pwrite64(fd, "x", 1, i) :
                    !strcmp(argv[4], "writev") ? writev(fd, &v, 1) : write(fd, "x", 1);
        printf("%ld:%d\n", (long)n, errno);
    }
    if (write(other, "ok", 2) != 2 || write(outside, "ok", 2) != 2) return 13;
    close(fd); close(other); close(outside);
    return 0;
}
'''


@unittest.skipUnless(sys.platform.startswith("linux") and shutil.which("gcc"),
                     "Linux and gcc required for LD_PRELOAD fixture")
class MaintenanceIoFaultTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.build = tempfile.TemporaryDirectory(prefix="maintenance-shim-build-")
        cls.addClassCleanup(cls.build.cleanup)
        build = Path(cls.build.name)
        cls.shim = build / "fault.so"
        cls.writer = build / "writer"
        source = Path(__file__).with_name("maintenance_io_fault.c")
        subprocess.run(["gcc", "-Wall", "-Wextra", "-Werror", "-shared", "-fPIC",
                        "-pthread", "-o", str(cls.shim), str(source)], check=True, capture_output=True)
        (build / "writer.c").write_text(WRITER)
        subprocess.run(["gcc", "-Wall", "-Wextra", "-Werror", "-o", str(cls.writer),
                        str(build / "writer.c")], check=True, capture_output=True)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="maintenance-shim-case-")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "native"
        self.root.mkdir()
        self.sentinel = self.root / ".maintenance-io-fault-ok"
        self.sentinel.touch()
        self.target = self.root / "data" / "part.lance"
        self.target.parent.mkdir()
        self.other = self.root / "unmatched"
        # Deliberately shares ROOT's prefix, but is outside its path boundary.
        self.outside = self.base / "native-other"
        self.marker = self.base / "marker"

    def environment(self, mode="enospc", after="1"):
        return {**os.environ, "LD_PRELOAD": str(self.shim),
                "OC_MAINTENANCE_FAULT_ROOT": str(self.root),
                "OC_MAINTENANCE_FAULT_MATCH": "/data/",
                "OC_MAINTENANCE_FAULT_MODE": mode,
                "OC_MAINTENANCE_FAULT_AFTER": after,
                "OC_MAINTENANCE_FAULT_MARKER": str(self.marker)}

    def command(self, operation="write"):
        return [str(self.writer), str(self.target), str(self.other), str(self.outside), operation]

    def run_writer(self, env, operation="write"):
        return subprocess.run(self.command(operation), env=env, text=True,
                              capture_output=True, check=True, timeout=10)

    def test_enospc_after_success_all_interposed_writes(self):
        for operation in ("write", "pwrite", "pwrite64", "writev"):
            with self.subTest(operation=operation):
                result = self.run_writer(self.environment(), operation)
                self.assertEqual(result.stdout.splitlines(), ["1:0", f"-1:{errno.ENOSPC}", f"-1:{errno.ENOSPC}"])
                self.assertEqual(self.target.read_bytes(), b"x")
                self.assertEqual(self.other.read_bytes(), b"okok")
                self.assertEqual(self.outside.read_bytes(), b"okok")
                marker = self.marker.read_text()
                self.assertIn("successful_matching_writes=1\n", marker)
                self.assertIn("relative_path=/data/part.lance\n", marker)
                self.assertIn("requested_bytes=1\n", marker)
                self.marker.unlink()

    def test_zero_byte_calls_neither_trigger_nor_count_as_payload(self):
        for operation in ("zero-write", "zero-pwrite", "zero-writev"):
            with self.subTest(operation=operation):
                result = self.run_writer(self.environment(), operation)
                self.assertEqual(result.stdout.splitlines(), ["1:0", "0:0", f"-1:{errno.ENOSPC}"])
                self.assertEqual(self.target.read_bytes(), b"x")
                marker = self.marker.read_text()
                self.assertIn("successful_matching_writes=1\n", marker)
                self.assertIn("requested_bytes=1\n", marker)
                self.marker.unlink()

    def test_writev_sums_requested_bytes_without_overflow(self):
        result = self.run_writer(self.environment(), "multi-writev")
        self.assertEqual(result.stdout.splitlines(), ["2:0", f"-1:{errno.ENOSPC}", f"-1:{errno.ENOSPC}"])
        self.assertIn("requested_bytes=2\n", self.marker.read_text())
        self.marker.unlink()
        result = self.run_writer(self.environment(), "overflow-writev")
        self.assertEqual(result.stdout.splitlines(), ["1:0", f"-1:{errno.EINVAL}", f"-1:{errno.ENOSPC}"])
        self.assertIn("successful_matching_writes=1\n", self.marker.read_text())
        self.assertIn("requested_bytes=1\n", self.marker.read_text())

    def test_pause_stops_whole_process_before_next_write_then_kill(self):
        with subprocess.Popen(self.command(), env=self.environment("pause"),
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE) as child:
            try:
                deadline = time.monotonic() + 10
                stopped = False
                while time.monotonic() < deadline and child.poll() is None:
                    status = Path(f"/proc/{child.pid}/status").read_text()
                    if self.marker.exists() and "\nState:\tT" in status:
                        stopped = True
                        break
                    time.sleep(0.01)
                self.assertTrue(stopped, "child did not reach a stopped matched native write")
                self.assertEqual(self.target.read_bytes(), b"x")
                self.assertEqual(self.other.read_bytes(), b"ok")
                self.assertEqual(self.outside.read_bytes(), b"ok")
                self.assertIn("mode=pause\n", self.marker.read_text())
            finally:
                child.kill()
                child.communicate(timeout=10)
            self.assertEqual(child.returncode, -signal.SIGKILL)

    def test_normal_child_without_opt_in_is_unaffected(self):
        env = self.environment()
        del env["OC_MAINTENANCE_FAULT_ROOT"]
        self.assertEqual(self.run_writer(env).stdout.splitlines(), ["1:0"] * 3)
        self.assertEqual(self.target.read_bytes(), b"xxx")
        self.assertFalse(self.marker.exists())

    def test_after_zero_fails_first_write(self):
        result = self.run_writer(self.environment(after="0"))
        self.assertEqual(result.stdout.splitlines(), [f"-1:{errno.ENOSPC}"] * 3)
        self.assertEqual(self.target.read_bytes(), b"")

    def test_missing_or_symlink_sentinel_disables_injection(self):
        self.sentinel.unlink()
        for symlink in (False, True):
            with self.subTest(symlink=symlink):
                if symlink:
                    backing = self.base / "sentinel-backing"
                    backing.touch()
                    self.sentinel.symlink_to(backing)
                self.assertEqual(self.run_writer(self.environment()).stdout.splitlines(), ["1:0"] * 3)
                self.assertFalse(self.marker.exists())

    def test_invalid_configuration_and_marker_fail_closed(self):
        for key, value in (("OC_MAINTENANCE_FAULT_AFTER", "-1"),
                           ("OC_MAINTENANCE_FAULT_AFTER", "18446744073709551616"),
                           ("OC_MAINTENANCE_FAULT_MODE", "unknown"),
                           ("OC_MAINTENANCE_FAULT_MATCH", ""),
                           ("OC_MAINTENANCE_FAULT_MARKER", str(self.root / "marker")),
                           ("OC_MAINTENANCE_FAULT_MARKER", str(self.base / "missing" / "marker"))):
            with self.subTest(key=key, value=value):
                env = self.environment()
                env[key] = value
                self.assertEqual(self.run_writer(env).stdout.splitlines(), ["1:0"] * 3)
                self.assertFalse(self.marker.exists())
        self.marker.write_text("preserve")
        self.assertEqual(self.run_writer(self.environment()).stdout.splitlines(), ["1:0"] * 3)
        self.assertEqual(self.marker.read_text(), "preserve")


if __name__ == "__main__":
    unittest.main()
