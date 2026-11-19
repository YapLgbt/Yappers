#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
export JAVA_HOME=$(/usr/libexec/java_home -v 21)
if [ "$#" -gt 0 ]; then
  if [ "$#" -ne 2 ] || [ "$1" != --data-dir ]; then echo 'Usage: scripts/run-desktop.sh [--data-dir PATH]' >&2; exit 1; fi
  # Gradle's --args parser accepts quoted paths; reject quote characters to avoid ambiguity.
  case "$2" in *\"*|*\'*) echo 'Use a data path without quote characters' >&2; exit 1;; esac
  exec ./gradlew :composeApp:run --args="--data-dir \"$2\""
fi
exec ./gradlew :composeApp:run
