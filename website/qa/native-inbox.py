"""Disposable messaging peer using the Netkit ABI called by the Apple apps."""
import ctypes
import json
from pathlib import Path
import secrets
import sys

join = json.loads(Path(sys.argv[1]).read_text())["join"]
assert join["serverAddr"]["id"] != "114f92b50e66dd84a14e0ff9b857b0c254565609b00b0cb9ef85f29e864efe5b"
library = Path("build/rust/debug") / ("libyappers_netkit.dylib" if sys.platform == "darwin" else "libyappers_netkit.so")
lib = ctypes.CDLL(str(library.resolve()))
lib.yappers_netkit_string_free.argtypes = [ctypes.c_void_p]


def invoke(name, *args):
    fn = getattr(lib, "yappers_netkit_" + name)
    fn.restype = ctypes.c_void_p
    pointer = fn(*[ctypes.c_uint64(x) if isinstance(x, int) else json.dumps(x).encode() for x in args])
    try:
        result = json.loads(ctypes.string_at(pointer))
        if not result["ok"]:
            raise RuntimeError(result["error"])
        return result["data"]
    finally:
        lib.yappers_netkit_string_free(pointer)


wallet = invoke("portability", {"type": "create", "rootSecret": secrets.token_hex(32), "home": join["serverAddr"]["id"]})["wallet"]
handle = invoke("open", {"join": join, "authorSecret": wallet["deviceSecret"], "nextSequence": 1, "accountChain": wallet["chain"]})["handle"]


def request(kind, **args):
    return invoke("request", handle, {"type": kind, **args})


state = None


def step(kind, **args):
    global state
    state = request("direct", state=state, action={"kind": kind, **args})["state"]


try:
    request("feed")
    request("social", op={"kind": "claimUsername", "name": "native_reader"})
    post = request("prepare", action={"kind": "postV2", "anonymous": False, "displayName": "Native reader",
                   "profile": {"photo": sys.argv[2], "bio": "", "identity": "", "interests": "", "pronouns": "", "hereFor": []},
                   "topic": "Chatter", "headline": "Native inbox test", "body": "Book swap on Sunday"})
    request("submit", event=post["event"])
    step("initialize")
    step("sync")
    print(json.dumps("ready"), flush=True)
    for line in sys.stdin:
        command = json.loads(line)
        if command["kind"] == "send":
            step("prepare", peer=command["peer"], label="Library neighbor", name="Native reader", body=command["body"])
        step("sync")
        if command["kind"] == "echo":
            received = [m for m in state["messages"] if not m["outgoing"] and m["body"] == command["expect"]]
            if received:
                step("prepare", peer=received[-1]["peer"], label="Android neighbor", name="Native reader", body=command["reply"])
                step("sync")
        print(json.dumps([m["body"] for m in state["messages"] if not m["outgoing"]]), flush=True)
finally:
    invoke("close", handle)
