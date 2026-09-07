//! The `stress_*` examples: the big multi-feature programs, which are the ones
//! most likely to break on a codegen change and were the least covered.
//!
//! Each expectation was read against the example own header comment before
//! being pinned, not blessed from whatever the compiler happened to say.

mod common;

#[test]
fn stress_anon_abstract() {
    common::expect_output(
        "stress_anon_abstract",
        "stress-anon-abstract",
        &[
            "circle",
            "<drawable label=circle>",
            "(o)",
            "square",
            "[]",
        ],
    );
}

#[test]
fn stress_anon_init() {
    common::expect_output(
        "stress_anon_init",
        "stress-anon-init",
        &[
            "init block ran, x=10",
            "second init block",
            "t() called",
        ],
    );
}

#[test]
fn stress_anon_init_abstract() {
    common::expect_output(
        "stress_anon_init_abstract",
        "stress-anon-init-abstract",
        &[
            "widget anonymous init: first block",
            "widget anonymous init: second block, n=7",
            "<widget>",
            "[rendered]",
        ],
    );
}

#[test]
fn stress_anonymous() {
    common::expect_output(
        "stress_anonymous",
        "stress-anonymous",
        &[
            "initialize called with location=42",
            "<initializable>",
            "==== HEADER ====",
            "render(width=80)",
            "[paint]",
        ],
    );
}

#[test]
fn stress_anonymous_minimal() {
    common::expect_output(
        "stress_anonymous_minimal",
        "stress-anonymous-minimal",
        &[
            "loc=42",
        ],
    );
}

#[test]
fn stress_async() {
    common::expect_output(
        "stress_async",
        "stress-async",
        &[
            "users      = 42",
            "parallel ints:",
            "42",
            "117",
            "9",
            "parallel strings:",
            "hello, alice",
            "hello, bob",
            "hello, carol",
            "doubled (seq)  = 84",
            "loader         = loader[jux-runtime]",
            "done.",
        ],
    );
}

#[test]
fn stress_async_advanced() {
    common::expect_output(
        "stress_async_advanced",
        "stress-async-advanced",
        &[
            "=== 1. mutating async method ===",
            "counter = 1, 2, 3",
            "=== 2. async interface dispatch ===",
            "https://example.com/api",
            "=== 3. await inside try/catch ===",
            "caught: simulated failure",
            "=== 4. await inside for-each loop ===",
            "loop total = 20",
            "=== 5. await inside ternary expression ===",
            "chosen = 20",
            "=== 6. generic async return ===",
            "wrapped = 42",
            "=== 7. nested parallel ===",
            "batch1 = 2, 4",
            "batch2 = 6, 8",
            "top    = 200, 400",
            "=== 8. parallel of async methods on instances ===",
            "http://a",
            "http://b",
            "done.",
        ],
    );
}

#[test]
fn stress_async_concurrency() {
    common::expect_output(
        "stress_async_concurrency",
        "stress-async-concurrency",
        &[
            "--- 3 yielding tasks under one parallel(...) ---",
            "[A] step 0",
            "[B] step 0",
            "[C] step 0",
            "[A] step 1",
            "[B] step 1",
            "[C] step 1",
            "[A] step 2",
            "[B] step 2",
            "[C] step 2",
            "[A] done",
            "[B] done",
            "[C] done",
            "results: A=3, B=3, C=3",
            "--- if it were truly sequential, A would finish before B started ---",
            "--- the interleaved output above is the proof of concurrency ---",
        ],
    );
}

#[test]
fn stress_async_parallel() {
    common::expect_output(
        "stress_async_parallel",
        "stress-async-parallel",
        &[
            "--- batch 1: 4 integer tasks in parallel ---",
            "[task 1] starting (amount=10)",
            "[task 2] starting (amount=20)",
            "[task 3] starting (amount=30)",
            "[task 4] starting (amount=40)",
            "results:",
            "10",
            "40",
            "90",
            "160",
            "--- batch 2: 3 fetch tasks in parallel ---",
            "[fetch] GET /users",
            "[fetch] GET /orders",
            "[fetch] GET /sessions",
            "<body of /users>",
            "<body of /orders>",
            "<body of /sessions>",
            "--- batch 3: nested parallels ---",
            "[task 5] starting (amount=50)",
            "[task 6] starting (amount=60)",
            "[fetch] GET /a",
            "[fetch] GET /b",
            "250",
            "360",
            "<body of /a>",
            "<body of /b>",
            "done.",
        ],
    );
}

#[test]
fn stress_employees() {
    common::expect_output(
        "stress_employees",
        "stress-employees",
        &[
            "Alice: Employee earns 10000 per month",
            "Bob: Contractor earns 12800 per month",
            "instances created: 2",
            "Employee TAX_RATE: 0.3",
        ],
    );
}

#[test]
fn stress_enum_machine() {
    common::expect_output(
        "stress_enum_machine",
        "stress-enum-machine",
        &[
            "PUSH 42",
            "PUSH 7",
            "ADD",
            "PUSH 3",
            "MUL",
            "POP",
            "ops run: 6",
        ],
    );
}

#[test]
fn stress_exceptions() {
    common::expect_output(
        "stress_exceptions",
        "stress-exceptions",
        &[
            "risky(7) ok",
            "finally block 1",
            "caught: negative input: -3",
            "finally block 2",
            "risky(42) ok",
            "finally block 3",
            "after all tries",
        ],
    );
}

#[test]
fn stress_generic_pair() {
    common::expect_output(
        "stress_generic_pair",
        "stress-generic-pair",
        &[
            "<1, one>",
            "<2, two>",
            "<3, three>",
            "swapped: <one, 1>",
            "paired count: 3",
        ],
    );
}

#[test]
fn stress_higher_order() {
    common::expect_output(
        "stress_higher_order",
        "stress-higher-order",
        &[
            "viaRef = 15",
            "doubled = 20",
            "Tracker.calls = 5",
        ],
    );
}

#[test]
fn stress_jux_std() {
    // Timings differ every run, so only the lines that carry none are
    // asserted -- enough to prove the program got where it was going.
    common::expect_contains(
        "stress_jux_std",
        "stress-jux-std",
        &[
            "--- section 1: collections ---",
            "xs        = len=4, first=1, last=4",
            "ages.size = 3",
            "seen.size = 2",
            "--- section 2: exceptions ---",
            "all stdlib exceptions caught + downcas",
        ],
    );
}

#[test]
fn stress_jux_std_parallel() {
    // The workers report in thread order and the elapsed time differs every run,
    // so the assertion is on the collected results rather than the log.
    common::expect_contains(
        "stress_jux_std_parallel",
        "stress-jux-std-parallel",
        &[
            "results.size = 4",
            "A = 6796",
            "B = 6796",
            "C = 6796",
            "D = 6796",
        ],
    );
}

#[test]
fn stress_kitchen_sink() {
    common::expect_output(
        "stress_kitchen_sink",
        "stress-kitchen-sink",
        &[
            "price=29.99 USD, doubled=59.98 USD",
            "sum=89.97 USD",
            "zero.isZero=true",
            "pair=(7, lucky)",
            "swap=(lucky, 7)",
            "circle area=78.53975",
            // `16.0`, not `16`: these are doubles, and a double keeps its
            // decimal point so it cannot be read as an int (LANG-V1 3.4).
            "square area=16.0",
            "triangle area=9.0",
            "biggest=circle",
            "Shape.allocated=3",
            "<named:hello>",
            "<named:(unnamed)>",
            "Tag.produced=2",
            "sumOfCubes(5)=100.0",
            "sumOf(x+1)(5)=15.0",
            "Stats.callCount=5",
        ],
    );
}

#[test]
fn stress_multi_payload() {
    common::expect_output(
        "stress_multi_payload",
        "stress-multi-payload",
        &[
            "two: hello 42",
            "none",
        ],
    );
}

#[test]
fn stress_nested() {
    common::expect_output(
        "stress_nested",
        "stress-nested",
        &[
            "1",
            "2",
            "Inner.n=7, squared=49",
            "Pair=3,4, sum=7",
            "Outer.count=2",
        ],
    );
}

#[test]
fn stress_null_chain() {
    common::expect_output(
        "stress_null_chain",
        "stress-null-chain",
        &[
            "host=example.com, port=443",
            "host=localhost, port=9999",
        ],
    );
}

#[test]
fn stress_pair_minimal() {
    common::expect_output(
        "stress_pair_minimal",
        "stress-pair-minimal",
        &[
            "1",
            "hi",
        ],
    );
}

#[test]
fn stress_record_ops() {
    common::expect_output(
        "stress_record_ops",
        "stress-record-ops",
        &[
            "a = 100 USD",
            "a.isZero = false",
            "zero.isZero = true",
            "total = 400 USD",
        ],
    );
}

#[test]
fn stress_sealed_pat() {
    common::expect_output(
        "stress_sealed_pat",
        "stress-sealed-pat",
        &[
            "red for 30s",
            "yellow for 5s",
            "green 25s + left arrow",
            "green 25s",
        ],
    );
}

#[test]
fn stress_stdlib() {
    common::expect_output(
        "stress_stdlib",
        "stress-stdlib",
        &[
            "=== List<T> methods ===",
            "size       = 3",
            "length     = 3",
            "isEmpty    = false",
            "contains 20 = true",
            "contains 99 = false",
            "indexOf 30 = 2",
            "indexOf 99 = -1",
            "first      = 10",
            "last       = 30",
            "get(1)     = 20",
            "after add(6) -> size=6, last=6",
            "after remove(0) -> first=2, size=5",
            "after insert(0, 99) -> first=99, size=6",
            "after reverse -> first=6, last=99",
            "after sort -> first=2, last=99",
            "after clear -> isEmpty=true",
            "=== String methods ===",
            "length     = 21",
            "trimmed    = 'Hello, Jux World!'",
            "upper      =   HELLO, JUX WORLD!",
            "lower      =   hello, jux world!",
            "contains J = true",
            "startsWith = true",
            "endsWith   = true",
            "indexOf J  = 9",
            "replace    = Hello, Rust World!",
            "substring  = 'Jux'",
            "charAt(0)  = H",
            "isEmpty    = false",
            "empty.isEmpty = true",
            "split size = 2",
            "split[0]   = 'Hello'",
            "split[1]   = 'Jux World!'",
        ],
    );
}

#[test]
fn stress_traffic() {
    common::expect_output(
        "stress_traffic",
        "stress-traffic",
        &[
            "red for 30s; switching to green",
            "green 25s + left arrow; switching to yellow",
            "green 25s; switching to yellow",
            "yellow for 5s; switching to red",
            "transitions counted: 4",
            "signature: TL-2026",
        ],
    );
}

#[test]
fn stress_upcast() {
    common::expect_output(
        "stress_upcast",
        "stress-upcast",
        &[
            "ok(70)",
            "err(negative: -3)",
            "ok(0)",
            "err(direct)",
            "ok(99)",
        ],
    );
}

#[test]
fn stress_workers() {
    // Four OS threads report in whatever order they finish, and every timing
    // differs run to run. What must hold is that both halves reach the same
    // total, which is the whole point of the example.
    common::expect_contains(
        "stress_workers",
        "stress-workers",
        &[
            "=== sequential baseline ===",
            "sequential sum   = 27184",
            "=== parallel workers ===",
            "parallel sum     = 27184",
        ],
    );
}

#[test]
fn stress_zoo() {
    common::expect_output(
        "stress_zoo",
        "stress-zoo",
        &[
            "Rex: Woof!",
            "Rex: Woof!",
            "Fang: Woof!",
            "<dog>",
            "<bat>",
            "legs (rex)=4, sound=Woof!",
            "Animal.liveCount=3",
            "Dog.barkCount=3",
        ],
    );
}
