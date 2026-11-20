#!/usr/bin/env python3
"""Exercise an installed tester APK and its instrumentation on a fresh emulator."""
import argparse
import base64
from contextlib import contextmanager, ExitStack
import json
import os
from pathlib import Path
import re
import selectors
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import xml.etree.ElementTree as ET

root = Path(__file__).resolve().parents[1]
os.chdir(root)
args = argparse.ArgumentParser(description=__doc__)
args.add_argument("serial", help="adb serial for the disposable emulator")
args.add_argument("--deletion-only", action="store_true", help="Seed the two accounts and exercise the deletion UI")
options = args.parse_args()
serial = options.serial
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

@contextmanager
def local_emulator_network():
    # The release public-enrollment client uses n0. Restrict the disposable
    # emulator to loopback, including during cold launches.
    rules = [
        ["iptables", "!", "-o", "lo", "-j", "REJECT"],
        ["ip6tables", "!", "-o", "lo", "-j", "REJECT"],
    ]
    active = []
    try:
        for tool, *rule in rules:
            subprocess.run(adb + ["shell", "su", "0", tool, "-I", "OUTPUT", "1", *rule], check=True)
            active.append((tool, rule))
        yield
    finally:
        subprocess.run(adb + ["shell", "am", "force-stop", "app.yappers.android"], check=True)
        for tool, rule in reversed(active):
            subprocess.run(adb + ["shell", "su", "0", tool, "-D", "OUTPUT", *rule], check=True)

@contextmanager
def emulator_home(data):
    join = data["join"]
    assert join["networkMode"] == "local"
    addr = join["serverAddr"]
    hint = next(h for h in addr["addrs"] if h.get("Ip", "").startswith("127.0.0.1:"))
    home = ("127.0.0.1", int(hint["Ip"].split(":")[1]))
    package = subprocess.check_output(adb + ["shell", "pm", "path", "app.yappers.android.test"], text=True).strip()
    assert package.startswith("package:/")
    proc = subprocess.Popen(adb + ["shell", "-T", "CLASSPATH=" + package.removeprefix("package:"),
                            "app_process", "/system/bin", "app.yappers.qa.EmulatorHomeTunnel", str(home[1])],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    stopped = threading.Event()
    errors = []
    def forward():
        peers = {}
        pending = bytearray()
        try:
            with selectors.DefaultSelector() as sockets:
                sockets.register(proc.stdout, selectors.EVENT_READ)
                while not stopped.is_set():
                    for key, _ in sockets.select(.5):
                        if key.fileobj is proc.stdout:
                            chunk = os.read(proc.stdout.fileno(), 65536)
                            if not chunk:
                                raise EOFError("Android fixture tunnel closed")
                            pending.extend(chunk)
                            while len(pending) >= 4:
                                port, length = struct.unpack_from("!HH", pending)
                                if len(pending) < 4 + length: break
                                if port not in peers:
                                    peer = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                                    peer.bind(("127.0.0.1", 0))
                                    peers[port] = peer
                                    sockets.register(peer, selectors.EVENT_READ, port)
                                peers[port].sendto(pending[4:4 + length], home)
                                del pending[:4 + length]
                        else:
                            packet, sender = key.fileobj.recvfrom(65535)
                            if sender == home:
                                proc.stdin.write(struct.pack("!HH", key.data, len(packet)) + packet)
                                proc.stdin.flush()
        except Exception as failure:
            errors.append(failure)
        finally:
            for peer in peers.values(): peer.close()

    try:
        if proc.stdout.read(4) != bytes(4):
            raise RuntimeError("Android fixture tunnel could not bind")
        worker = threading.Thread(target=forward)
        worker.start()
        try:
            yield {**data, "join": {**join, "networkMode": "n0", "serverAddr": {**addr, "addrs": [hint]}}}
        finally:
            stopped.set()
            worker.join()
            if errors: raise errors[0]
    finally:
        proc.stdin.close()
        proc.wait(timeout=5)
        proc.stdout.close()

def check_account_deletion(join, directory, peer):
    activity = "app.yappers.android/app.yappers.YapActivity"
    def start():
        subprocess.run(adb + ["shell", "am", "force-stop", "app.yappers.android"], check=True)
        subprocess.run(adb + ["shell", "am", "start", "-n", activity], check=True, stdout=subprocess.DEVNULL)

    def control(label, *, tap=False, scroll=False):
        visible = []
        for _ in range(12):
            xml = subprocess.check_output(adb + ["exec-out", "uiautomator", "dump", "/dev/tty"], text=True)
            if "</hierarchy>" not in xml:
                time.sleep(.3)
                continue
            tree = ET.fromstring(xml[:xml.index("</hierarchy>") + len("</hierarchy>")])
            visible = [value for node in tree.iter("node") for value in
                       (node.get("text"), node.get("content-desc")) if value]
            if "Couldn’t delete account" in visible:
                break
            for node in tree.iter("node"):
                if label not in (node.get("text"), node.get("content-desc")): continue
                if tap:
                    x1, y1, x2, y2 = map(int, re.findall(r"\d+", node.get("bounds")))
                    subprocess.run(adb + ["shell", "input", "tap", str((x1+x2)//2), str((y1+y2)//2)], check=True)
                return
            if scroll:
                _, _, w, h = map(int, re.findall(r"\d+", tree.find("node").get("bounds")))
                subprocess.run(adb + ["shell", "input", "swipe", str(w//2), str(3*h//4), str(w//2), str(h//3), "320"], check=True)
        raise AssertionError(f"Missing Android control {label!r}. Visible controls: {visible}")

    reader_join = directory / "reader-join.json"
    reader_join.write_text(json.dumps(join))
    def read_posts():
        result = subprocess.check_output([server, "feed", "--join", reader_join,
                                          "--identity", directory / "deletion-reader.json"])
        return {post["id"]: post["headline"] for post in json.loads(result)["posts"]}

    before = read_posts()
    target = next(pid for pid, headline in before.items() if headline == "Android meetup")
    survivor = next(pid for pid, headline in before.items() if headline == "Native inbox test")
    store = "/data/user/0/app.yappers.android/no_backup"
    def records(path):
        return set(subprocess.check_output(adb + ["shell", "su", "0", "ls", "-A", path], text=True).splitlines())
    assert {"secret-identity-v1", "state"} <= records(store + "/account"), "Missing enrolled device data"
    start()
    control("You", tap=True)
    control("Delete account", tap=True, scroll=True)
    control("Permanently delete account?")
    control("Cancel", tap=True)
    assert target in read_posts(), "Cancel deleted an account's post"
    assert "secret-identity-v1" in records(store + "/account"), "Cancel erased the device identity"
    control("Delete account", tap=True)
    control("Permanently delete account?")
    control("Delete account", tap=True)
    control("Create an account")
    after = read_posts()
    assert target not in after and survivor in after, "Home did not selectively erase the account"
    start()
    control("Create an account")
    assert not records(store + "/account"), "Account files survived restart"
    assert not any(name.startswith("account-deletion-v1") for name in records(store)), "Deletion journal survived restart"
    cleanup = subprocess.check_output(adb + ["shell", "am", "instrument", "-w", "-e", "check", "verify-wipe",
                                      "app.yappers.android.test/app.yappers.qa.DisposableHomeCheck"], text=True)
    assert "INSTRUMENTATION_CODE: -1" in cleanup, cleanup
    print(json.dumps(dict(kind="reply", parent=survivor)), file=peer.stdin, flush=True)
    peer_result(peer)  # The retained account signs and submits a new reply.
    print("PASS Android deletion cancellation, confirmed Home erasure, empty device account store after restart, and retained second account posting.")

with tempfile.TemporaryDirectory(prefix="yap-android-qa-") as tmp, local_emulator_network(), ExitStack() as tunnels:
    fixture = Path(tmp)
    manifest = fixture / "manifest.json"
    command = [server, "public-home", "--directory", fixture / "home", "--manifest", manifest]
    with open(fixture / "server.log", "w") as log:
        subprocess.run([server, "init", "--network", "local", "--auto-federate", "--directory", fixture / "home"], check=True, stdout=log)
        config_path = fixture / "home/config.json"
        config = json.loads(config_path.read_text())
        [(community, token)] = config["communities"].items()
        home = community.split("/")[0]
        config["communities"] = {f"{home}/public": token}
        config_path.write_text(json.dumps(config))
        config_path.rename(fixture / "home/public-home.json")
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
            encoded = base64.b64encode(json.dumps(tunnels.enter_context(emulator_home(data))).encode()).decode()
            check = subprocess.Popen(adb + ["shell", "am", "instrument", "-w", "-e", "fixture", encoded,
                                     "-e", "expectedHome", home, "-e", "check", "delete-account" if options.deletion_only else "full",
                                     "app.yappers.android.test/app.yappers.qa.DisposableHomeCheck"],
                                     stdout=subprocess.PIPE, text=True)
            deadline = time.monotonic() + 200
            replied = False
            while check.poll() is None and time.monotonic() < deadline:
                if not replied and not options.deletion_only:
                    print(json.dumps(dict(kind="echo", expect="Android says hello 🌈", reply="Hello from native 🌈")), file=peer.stdin, flush=True)
                    replied = "Android says hello 🌈" in peer_result(peer)
                time.sleep(1)
            output = check.communicate(timeout=5)[0]
            print(output, end="")
            if "INSTRUMENTATION_CODE: -1" not in output or not (options.deletion_only or replied):
                sys.exit(1)
            if options.deletion_only:
                check_account_deletion(data["join"], fixture, peer)
                sys.exit(0)
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
            check_account_deletion(data["join"], fixture, peer)
        finally:
            if check is not None and check.poll() is None:
                check.kill()
                check.wait()
            if peer is not None:
                peer.stdin.close()
                peer.wait(timeout=15)
            host.send_signal(signal.SIGINT)
            host.wait(timeout=15)
