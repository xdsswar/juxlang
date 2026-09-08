"""The compiler must never panic, however broken the input.

Rust's rule: an internal compiler error is ALWAYS a bug, whatever the source
said. A user who typed something malformed should get a diagnostic pointing at
it, never a stack trace and never a hang. Jux has no equivalent gate, so this
one takes every example in the corpus, breaks it in mechanical ways, and
asserts the front end still answers with diagnostics.

Mutations are deliberately dumb -- truncation, deleting a line, duplicating a
line, dropping a character, inserting a stray delimiter. Cleverness is not the
point; a parser that survives an adversary usually dies on a typo.

    python tools/no-ice-fuzz/fuzz.py                 # whole corpus
    python tools/no-ice-fuzz/fuzz.py --only stress   # names containing "stress"
    python tools/no-ice-fuzz/fuzz.py --seed 7        # reproduce a run

Every finding is written to `crashes/` with the exact input that caused it.
"""

import argparse
import io
import os
import random
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(ROOT, '..', '..'))
EXAMPLES = os.path.join(REPO, 'examples')
CRASHES = os.path.join(ROOT, 'crashes')
WORK = os.path.join(ROOT, 'work')

JUXC = os.path.join(REPO, 'target', 'release', 'juxc.exe')
if not os.path.exists(JUXC):
    JUXC = os.path.join(REPO, 'target', 'release', 'juxc')

# A crash is any of these. Note that a NON-ZERO exit is expected and fine --
# broken input SHOULD be rejected. What must never happen is a panic, an
# assertion, an overflow, or a hang.
CRASH_MARKERS = (
    'panicked at',
    'internal error',
    'RUST_BACKTRACE',
    'stack overflow',
    'attempt to subtract with overflow',
    'index out of bounds',
    'unwrap()` on a `None`',
    'unwrap()` on an `Err`',
)


def mutations(src, rng, per_file):
    """Yield (label, mutated_source) pairs."""
    lines = src.split('\n')
    out = []

    # Truncation: the classic parser crasher. An unterminated everything.
    for _ in range(max(2, per_file // 3)):
        cut = rng.randrange(1, max(2, len(src)))
        out.append(('truncate@%d' % cut, src[:cut]))

    if len(lines) > 2:
        for _ in range(max(1, per_file // 4)):
            i = rng.randrange(len(lines))
            out.append(('drop-line@%d' % i, '\n'.join(lines[:i] + lines[i + 1:])))
        for _ in range(max(1, per_file // 4)):
            i = rng.randrange(len(lines))
            out.append(('dup-line@%d' % i,
                        '\n'.join(lines[:i] + [lines[i]] + lines[i:])))

    if len(src) > 4:
        for _ in range(max(1, per_file // 4)):
            i = rng.randrange(len(src))
            out.append(('drop-char@%d' % i, src[:i] + src[i + 1:]))
        for _ in range(max(1, per_file // 4)):
            i = rng.randrange(len(src))
            c = rng.choice('}{)(><"\';,.@$?!:')
            out.append(('insert-%s@%d' % (c, i), src[:i] + c + src[i:]))

    return out


def check(path):
    """Run the front end. Returns (crashed, detail)."""
    try:
        p = subprocess.run(
            [JUXC, '--check', path],
            capture_output=True, text=True, timeout=25,
            encoding='utf-8', errors='replace',
        )
    except subprocess.TimeoutExpired:
        return True, 'HANG (front end did not finish in 25s)'
    blob = (p.stdout or '') + (p.stderr or '')
    for m in CRASH_MARKERS:
        if m in blob:
            line = next((l for l in blob.split('\n') if m in l), m)
            return True, line.strip()[:200]
    # A panic can also show only as the abort exit code.
    if p.returncode in (101, -1073741819, 134, 139):
        return True, 'exit %d with no diagnostic' % p.returncode
    return False, ''


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--only', default='', help='substring filter on example name')
    ap.add_argument('--per-file', type=int, default=12)
    ap.add_argument('--seed', type=int, default=1)
    args = ap.parse_args()

    if not os.path.exists(JUXC):
        print('no juxc at %s -- run: cargo build --release' % JUXC)
        return 2

    rng = random.Random(args.seed)
    shutil.rmtree(WORK, ignore_errors=True)
    os.makedirs(WORK, exist_ok=True)
    os.makedirs(CRASHES, exist_ok=True)

    sources = sorted(
        os.path.join(EXAMPLES, f)
        for f in os.listdir(EXAMPLES)
        if f.endswith('.jux') and args.only in f
    )
    if not sources:
        print('no examples matched')
        return 2

    total = 0
    found = []
    for src_path in sources:
        name = os.path.basename(src_path)[:-4]
        try:
            src = io.open(src_path, encoding='utf-8', newline='').read()
        except OSError:
            continue
        for label, mutant in mutations(src, rng, args.per_file):
            total += 1
            work = os.path.join(WORK, 'm.jux')
            io.open(work, 'w', encoding='utf-8', newline='').write(mutant)
            crashed, detail = check(work)
            if crashed:
                key = '%s__%s' % (name, label.replace('@', '_at_').replace('/', '_'))
                keep = os.path.join(CRASHES, key + '.jux')
                io.open(keep, 'w', encoding='utf-8', newline='').write(mutant)
                found.append((key, detail))
                print('CRASH %-46s %s' % (key[:46], detail))
        print('  %-40s %d mutations' % (name, total), end='\r')

    print(' ' * 70, end='\r')
    print('---- %d mutations, %d crashes ----' % (total, len(found)))
    if found:
        print('inputs kept under %s' % CRASHES)
    return 1 if found else 0


if __name__ == '__main__':
    sys.exit(main())
