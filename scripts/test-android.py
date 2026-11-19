#!/usr/bin/env python3
"""Exercise an installed tester APK and its instrumentation on a fresh emulator."""
import argparse
import base64
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import sys
import tempfile
import time

root = Path(__file__).resolve().parents[1]
os.chdir(root)
args = argparse.ArgumentParser(description=__doc__)
args.add_argument("serial", help="adb serial for the disposable emulator")
serial = args.parse_args().serial
if not serial.startswith("emulator-"):
    sys.exit("Choose a disposable emulator.")
adb = [str(Path(os.environ["ANDROID_HOME"]) / "platform-tools/adb"), "-s", serial]
server = root / "build/rust/debug/yappers-server"

def read_manifest(path, proc):
    for _ in range(300):
        if path.exists() and path.stat().st_size:
            return json.loads(path.read_text())
        if proc.poll() is not None:
            raise RuntimeError("The fixture home stopped during startup")
        time.sleep(.1)
    raise TimeoutError("The fixture home did not start")

def peer_result(proc):
    with selectors.DefaultSelector() as selector:
        selector.register(proc.stdout, selectors.EVENT_READ)
        if not selector.select(timeout=60):
            raise TimeoutError("Native peer timed out")
    return json.loads(proc.stdout.readline())

with tempfile.TemporaryDirectory(prefix="yap-android-qa-") as tmp:
    fixture = Path(tmp)
    manifest = fixture / "manifest.json"
    command = [server, "public-home", "--directory", fixture / "home", "--manifest", manifest]
    with open(fixture / "server.log", "w") as log:
        bootstrap = subprocess.Popen(command, stdout=log, stderr=log)
        try:
            home = read_manifest(manifest, bootstrap)["join"]["serverAddr"]["id"]
        finally:
            bootstrap.send_signal(signal.SIGINT)
            bootstrap.wait(timeout=12)
        manifest.unlink()
        env = dict(os.environ, YAP_USERNAME_DOMAINS=home + "=neighborhood.test", YAP_REPORT_INBOX=str(fixture / "reports.sqlite"),
                   YAP_PUSH_PROVIDERS="fcm", YAP_PUSH_WORKER=str(root / "website/qa/record-push.py"),
                   YAP_TEST_PUSH_ROUTE=str(fixture / "push-route"))
        host = subprocess.Popen(command, stdout=log, stderr=log, env=env)
        peer = check = None
        try:
            data = read_manifest(manifest, host)
            peer = subprocess.Popen([sys.executable, "website/qa/native-inbox.py", manifest], stdin=subprocess.PIPE,
                                    stdout=subprocess.PIPE, text=True)
            assert peer_result(peer) == "ready"
            encoded = base64.b64encode(json.dumps(data).encode()).decode()
            check = subprocess.Popen(adb + ["shell", "am", "instrument", "-w", "-e", "fixture", encoded,
                                     "-e", "expectedHome", home, "app.yappers.android.test/app.yappers.qa.DisposableHomeCheck"],
                                     stdout=subprocess.PIPE, text=True)
            deadline = time.monotonic() + 200
            replied = False
            while check.poll() is None and time.monotonic() < deadline:
                if not replied:
                    print(json.dumps(dict(kind="echo", expect="Android says hello 🌈", reply="Hello from native 🌈")), file=peer.stdin, flush=True)
                    replied = "Android says hello 🌈" in peer_result(peer)
                time.sleep(1)
            output = check.communicate(timeout=5)[0]
            print(output, end="")
            if "INSTRUMENTATION_CODE: -1" not in output or not replied:
                sys.exit(1)
            print(json.dumps(dict(kind="reply", headline="Android meetup")), file=peer.stdin, flush=True)
            peer_result(peer)
            route_file = fixture / "push-route"
            for _ in range(100):
                if route_file.exists(): break
                time.sleep(.2)
            route = json.loads(route_file.read_text())["route"]
            assert len(route) == 64 and all(ch in "0123456789abcdef" for ch in route)
            subprocess.run(adb + ["shell", "am", "force-stop", "app.yappers.android"], check=True)
            subprocess.run(adb + ["shell", "am", "start", "-n", "app.yappers.android/app.yappers.YapActivity",
                                  "-a", "app.yappers.OPEN_INBOX", "--es", "route", route], check=True, stdout=subprocess.DEVNULL)
            for _ in range(32):
                dump = subprocess.check_output(adb + ["exec-out", "uiautomator", "dump", "/dev/tty"], text=True)
                if "Following up on your post" in dump and "Android meetup" in dump:
                    print("PASS cold Android notification intent opened its authenticated reply thread.")
                    break
                time.sleep(.5)
            else:
                raise AssertionError("Cold Android notification intent did not open the reply")
        finally:
            if check is not None and check.poll() is None:
                check.kill()
                check.wait()
            if peer is not None:
                peer.stdin.close()
                peer.wait(timeout=15)
            host.send_signal(signal.SIGINT)
            host.wait(timeout=15)
