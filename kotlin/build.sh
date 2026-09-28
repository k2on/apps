#!/usr/bin/env sh
# The kotlinc fallback: the same sources and the same conformance runner as
# the Gradle build, with no Gradle in it. Run inside the repository's
# kotlin devshell:
#
#     nix develop .#kotlin -c ./kotlin/build.sh            (from /home/user/apps)
#
# It compiles ark-runtime into out/, builds the runner as a runnable jar,
# and runs it over ../spec/vectors (or $1).
set -eu
cd "$(dirname "$0")"
VECTORS="${1:-$(pwd)/../spec/vectors}"
rm -rf out test.jar
kotlinc ark-runtime/src/main/kotlin -d out
kotlinc ark-runtime/src/test/kotlin ../harken/domain/gen/kotlin -cp out -include-runtime -d test.jar
exec java -cp "out:test.jar" dev.arkdb.conformance.Conformance "$VECTORS"
