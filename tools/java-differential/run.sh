#!/usr/bin/env bash
# Differential test: Jux against Java.
#
# Every case is a TWIN -- the same program written twice, once in each
# language. Java runs it, Jux runs it, and the outputs must match line for
# line. Java is the oracle, which is the point: an EXPECT header is only ever
# as good as whoever wrote it, and several of mine were wrong. Nobody writes
# the answer here; the JDK does.
#
# A case may declare a DELIBERATE divergence with a header line in the .jux:
#   // DIVERGES: <reason>
# Those still run, and are reported separately -- a divergence that STOPS
# happening is as interesting as one that appears.
#
# Java 25's single-file source launcher runs `Main.java` with no javac step.
set -u
ROOT="$(cd "$(dirname "$0")" && pwd)"
# Resolve the built compiler from the repo this script lives in, so the
# harness works from any checkout and any working directory.
REPO="$(cd "$ROOT/../.." && pwd)"
JUX="${JUX_BIN:-$REPO/target/release/jux}"
[ -x "$JUX" ] || JUX="$JUX.exe"
export JUX_HOME="${JUX_HOME:-$REPO/target/release}"
# One shared target dir: the dependency tree compiles once for all cases
# instead of once per case.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/shared}"

if ! command -v java >/dev/null 2>&1; then
  echo "java not on PATH -- this harness needs a JDK (17+) as the oracle"
  exit 2
fi
if [ ! -x "$JUX" ]; then
  echo "no jux binary at $JUX -- run: cargo build --release"
  exit 2
fi
ONLY="${1:-}"

pass=0; fail=0; diverge=0
: > "$ROOT/results.txt"

for jux in "$ROOT"/cases/*.jux; do
  name=$(basename "$jux" .jux)
  [ -n "$ONLY" ] && [[ "$name" != *"$ONLY"* ]] && continue
  java_src="$ROOT/cases/$name/Main.java"
  if [ ! -f "$java_src" ]; then
    printf 'NOJAVA %-30s (no twin)\n' "$name" | tee -a "$ROOT/results.txt"
    continue
  fi

  # STDOUT only: a compiler warning is a diagnostic, not program output, and
  # Java has no equivalent of (say) Jux's reference-cycle lint. Diagnostics
  # are kept separately so a failure can still show them.
  jerr="$ROOT/emit/$name.err"
  mkdir -p "$ROOT/emit"
  jout=$("$JUX" run --emit-dir "$ROOT/emit/$name" "$jux" 2>"$jerr")
  jrc=$?
  vout=$(java "$java_src" 2>&1)
  vrc=$?

  # Normalize: drop the compiler's own progress line, trim, drop blanks.
  norm() { grep -v '^jux: ' | sed 's/[[:space:]]*$//' | grep -v '^$'; }
  j=$(printf '%s\n' "$jout" | norm)
  v=$(printf '%s\n' "$vout" | norm)

  divergence=$(grep -m1 '^// DIVERGES:' "$jux" | sed 's|^// DIVERGES: *||')

  if [ $vrc -ne 0 ]; then
    printf 'JAVAERR %-29s %s\n' "$name" "$(printf '%s' "$v" | head -1)" | tee -a "$ROOT/results.txt"
    fail=$((fail+1))
  elif [ $jrc -ne 0 ]; then
    printf 'JUXERR %-30s %s\n' "$name" "$(printf '%s' "$j" | grep -m1 -E '\[E[0-9]+\]|^error' | head -c 150)" | tee -a "$ROOT/results.txt"
    fail=$((fail+1))
  elif [ "$j" = "$v" ]; then
    if [ -n "$divergence" ]; then
      printf 'AGREED %-30s (declared divergence no longer happens: %s)\n' "$name" "$divergence" | tee -a "$ROOT/results.txt"
      diverge=$((diverge+1))
    else
      pass=$((pass+1))
    fi
  elif [ -n "$divergence" ]; then
    printf 'DIVERGE %-29s %s\n' "$name" "$divergence" | tee -a "$ROOT/results.txt"
    diverge=$((diverge+1))
  else
    printf 'MISMATCH %-28s\n' "$name" | tee -a "$ROOT/results.txt"
    diff <(printf '%s\n' "$v") <(printf '%s\n' "$j") | sed 's/^/    /' | head -20 | tee -a "$ROOT/results.txt"
    fail=$((fail+1))
  fi
done
echo "---- match=$pass mismatch=$fail declared-divergence=$diverge ----" | tee -a "$ROOT/results.txt"
