"""Print the UDID of the first booted iPhone simulator."""
import json
import subprocess
import sys

result = subprocess.run(
    ["xcrun", "simctl", "list", "devices", "booted", "-j"], stdout=subprocess.PIPE
)
if result.returncode:
    sys.exit(result.returncode)

for devices in json.loads(result.stdout)["devices"].values():
    for device in devices:
        model = device.get("deviceTypeIdentifier", device["name"]).split(".")[-1]
        if device["state"] == "Booted" and model.startswith("iPhone"):
            print(device["udid"])
            sys.exit(0)

sys.exit("Boot an iPhone in Simulator or pass a simulator UDID as the first argument.")
