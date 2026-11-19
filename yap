#!/bin/bash
# Public brand entry point; persistent service/data namespaces remain compatible.
set -euo pipefail
exec python3 "$(dirname "$0")/scripts/local-federation.py" "$@"
