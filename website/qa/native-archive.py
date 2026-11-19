"""Round-trip a disposable PWA backup through the same ABI used by the Apple apps."""
import ctypes
import json
from pathlib import Path
import sys

suffix = "dylib" if sys.platform == "darwin" else "so"
libraries = [Path("build/rust") / variant / f"libyappers_netkit.{suffix}" for variant in ("release", "debug")]
lib = ctypes.CDLL(str(next(path for path in libraries if path.exists()).resolve()))
lib.yappers_netkit_portability.argtypes = [ctypes.c_char_p]
lib.yappers_netkit_portability.restype = ctypes.c_void_p
lib.yappers_netkit_string_free.argtypes = [ctypes.c_void_p]

def invoke(command):
    pointer = lib.yappers_netkit_portability(json.dumps(command).encode())
    try:
        result = json.loads(ctypes.string_at(pointer))
        assert result["ok"], "Native backup operation failed"
        return result["data"]
    finally:
        lib.yappers_netkit_string_free(pointer)

passphrase = "test backup for disposable home"
bundle = invoke({"type": "decrypt", "ciphertext": sys.stdin.buffer.read().hex(), "passphrase": passphrase})["bundle"]
archive = bundle["archive"]["payload"]
assert archive["localData"]["client"]["profile"]["bio"] == "Book club regular"
assert len(archive["events"]) > 0
encrypted = invoke({"type": "encrypt", "wallet": bundle["wallet"], "events": archive["events"],
                    "localData": archive["localData"], "passphrase": passphrase})
sys.stdout.buffer.write(bytes.fromhex(encrypted["ciphertext"]))
