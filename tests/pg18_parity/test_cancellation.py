"""Reference-process cleanup under external runner cancellation."""

import json
import os
import pathlib
import select
import signal
import socket
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class CancellationTests(unittest.TestCase):
    def should_record_cli_cancellation_as_unavailable_and_retire_reference(self):
        # Arrange
        folder = pathlib.Path(tempfile.mkdtemp(prefix="pg18-cli-", dir=ROOT / "target")) / "run"
        with socket.socket() as unavailable_cassie, (folder.parent / "runner.log").open("wb") as log:
            unavailable_cassie.bind(("127.0.0.1", 0))
            unavailable_cassie.listen(1)  # Hold startup unanswered until cancellation.
            env = os.environ.copy()
            env["PG18_CANCEL_TEST_DSN"] = f"host=127.0.0.1 port={unavailable_cassie.getsockname()[1]} user=root dbname=postgres sslmode=disable"
            runner = subprocess.Popen(
                [sys.executable, "-m", "tools.pg18_parity.run", "--reference-bin-dir",
                 "/opt/homebrew/opt/postgresql@18/bin", "--libpq",
                 "/opt/homebrew/opt/postgresql@18/lib/postgresql/libpq.dylib", "--cassie-dsn-env",
                 "PG18_CANCEL_TEST_DSN", "--cassie-revision", "769d4db8fb7e60f0a29fade73137433a20e5a817",
                 "--output", str(folder)], cwd=ROOT, env=env, stdout=log, stderr=log)
            pid_file = folder / "ref/data/postmaster.pid"
            try:
                deadline = time.monotonic() + 45
                while time.monotonic() < deadline:
                    if pid_file.exists() and pid_file.read_text().splitlines()[-1].strip() == "ready":
                        break
                    if runner.poll() is not None:
                        self.fail("runner exited before its reference became ready")
                    time.sleep(0.02)
                else:
                    self.fail("reference readiness deadline")
                # Act
                runner.terminate()
                runner.wait(timeout=20)
                receipt = json.loads((folder / "receipt.json").read_text())
                # Assert
                self.assertEqual(runner.returncode, 2)
                self.assertEqual(receipt["bootstrap"], "unavailable")
                self.assertEqual(receipt["qualification"], "incomplete")
                self.assertFalse(pid_file.exists())
            finally:
                if runner.poll() is None:
                    runner.terminate()
                    runner.wait(timeout=20)
                if pid_file.exists():
                    os.kill(int(pid_file.read_text().splitlines()[0]), signal.SIGTERM)

    def should_stop_owned_reference_when_the_runner_receives_sigterm(self):
        # Arrange
        folder = pathlib.Path(tempfile.mkdtemp(prefix="pg18-cancel-", dir=ROOT / "target")) / "ref"
        script = """import json,pathlib,time
from tools.pg18_parity.libpq import Libpq
from tools.pg18_parity.reference import reference
manifest=json.loads(pathlib.Path('tests/corpus/postgresql18/reference.json').read_text())
with reference('/opt/homebrew/opt/postgresql@18/bin',manifest,pathlib.Path(PATH),Libpq('/opt/homebrew/opt/postgresql@18/lib/postgresql/libpq.dylib')):
 print('ready',flush=True)
 time.sleep(60)
""".replace("PATH", repr(str(folder)))
        with (folder.parent / "runner.log").open("wb") as log:
            runner = subprocess.Popen([sys.executable, "-c", script], cwd=ROOT,
                                      stdout=subprocess.PIPE, stderr=log)
            reference_pid = None
            try:
                ready, _, _ = select.select([runner.stdout], [], [], 45)
                self.assertTrue(ready, "reference must signal actual readiness")
                self.assertEqual(runner.stdout.readline(), b"ready\n")
                pid_file = folder / "data/postmaster.pid"
                reference_pid = int(pid_file.read_text().splitlines()[0])
                # Act
                runner.terminate()
                runner.wait(timeout=20)
                # Assert
                self.assertFalse(pid_file.exists(), "SIGTERM must retire the owned PostgreSQL server")
            finally:
                if runner.poll() is None:
                    runner.terminate()
                    runner.wait(timeout=20)
                runner.stdout.close()
                # A failing RED probe must clean up its own confirmed leaked child.
                if reference_pid and (folder / "data/postmaster.pid").exists():
                    os.kill(reference_pid, signal.SIGTERM)
                    for _ in range(100):
                        if not (folder / "data/postmaster.pid").exists():
                            break
                        time.sleep(0.05)


if __name__ == "__main__":
    loader = unittest.TestLoader()
    loader.testMethodPrefix = "should_"
    result = unittest.TextTestRunner(verbosity=2).run(loader.loadTestsFromTestCase(CancellationTests))
    sys.exit(not result.wasSuccessful())
