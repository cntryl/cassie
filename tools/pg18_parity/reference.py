"""Own the lifecycle of an immutable, local PostgreSQL reference process."""

from contextlib import contextmanager
import hashlib
import pathlib
import signal
import subprocess
import time


def verify_artifact(bin_dir, manifest):
    for name, expected in manifest["executables"].items():
        path = pathlib.Path(bin_dir) / name
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError(f"reference artifact mismatch: {name}")


def interrupt_runner(_signal, _frame):
    raise KeyboardInterrupt("reference runner interrupted")


@contextmanager
def reference(bin_dir, manifest, output, libpq):
    verify_artifact(bin_dir, manifest)
    output = pathlib.Path(output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    data, sockets = output / "data", output / "socket"
    sockets.mkdir(mode=0o700)
    if len(str(sockets).encode()) > 85:
        raise ValueError("reference socket path is too long; use a shorter output path")
    with (output / "initdb.log").open("wb") as log:
        subprocess.run([str(pathlib.Path(bin_dir) / "initdb"), "-D", str(data),
                        "-A", "trust", "-U", "root", "--no-locale", "-E", "UTF8"],
                       stdout=log, stderr=subprocess.STDOUT, check=True, timeout=30)
    with (output / "postgres.log").open("wb") as log:
        process = subprocess.Popen([str(pathlib.Path(bin_dir) / "postgres"), "-D", str(data),
                                    "-k", str(sockets), "-p", "55432", "-c", "listen_addresses="],
                                   stdout=log, stderr=subprocess.STDOUT)
        previous_handler = None
        try:
            previous_handler = signal.signal(signal.SIGTERM, interrupt_runner)
            # libpq keyword values require their own escaping, unrelated to shell quoting.
            escaped = str(sockets).replace("\\", "\\\\").replace("'", "\\'")
            dsn = f"host='{escaped}' port=55432 user=root dbname=postgres sslmode=disable"
            for _ in range(100):
                if process.poll() is not None:
                    raise RuntimeError("reference process exited during startup")
                try:
                    with libpq.connect(dsn) as conn:
                        version = libpq.api.PQserverVersion(conn.handle)
                        if version != manifest["server_version_num"]:
                            raise ValueError("reference server version mismatch")
                    break
                except ConnectionError:
                    time.sleep(0.05)
            else:
                raise TimeoutError("reference server did not become ready")
            yield dsn
        finally:
            if previous_handler is not None:
                signal.signal(signal.SIGTERM, signal.SIG_IGN)
            try:
                process.terminate()
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            finally:
                if previous_handler is not None:
                    signal.signal(signal.SIGTERM, previous_handler)
            if process.returncode != 0:
                raise RuntimeError("reference did not shut down cleanly")
