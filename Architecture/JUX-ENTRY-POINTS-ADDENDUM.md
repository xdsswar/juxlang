# Jux Spec Addendum — Entry Points

**Status:** Normative. Targets JUX-LANG-V1.md §7.15 and §16.
**Sigil:** §E

This addendum specifies how a Jux program starts: which function the runtime invokes, what signatures are accepted, how non-standard entry points (DLLs, kernel modules, MCU firmware) are declared, and how freestanding bare-metal mode bypasses the runtime entirely.

---

## Design Philosophy (Non-Normative)

Jux has **three tiers** of entry point:

1. **Implicit `main`** — the default. Top-level statements *or* a `main` function in the entry file. Covers ~95% of programs.
2. **`@entry` annotation** — when the platform demands a specific symbol or calling convention (`DllMain`, `init_module`, `_start`).
3. **Freestanding mode** — no runtime, no CRT, no automatic initialization. The user is the runtime.

The design lets a beginner write `print("Hello")` in `main.jux` and run it with `jux run`. It also lets a kernel author hand-write the reset handler. Same language, same compiler, three sharp options.

---

## §E.1 — Implicit `main` (Default)

### E.1.1. Top-Level Statements

The simplest legal Jux program is a file containing top-level statements:

```java
// main.jux
print("Hello, world!");
```

The compiler wraps top-level statements in a synthetic `main` and emits the program entry. This is the **default** behaviour of the entry file (per §E.5).

### E.1.2. Explicit `main` Function

The entry file may instead declare a `main` function. The compiler accepts any of these signatures, picking the first match in declaration order:

```java
public void main()                            // no args, no exit code
public void main(String[] args)               // args, no exit code
public int main()                             // exit code
public int main(String[] args)                // args and exit code
```

Each signature may optionally declare `throws E` for any checked exception type `E`:

```java
public void main(String[] args) throws IOError {
    var contents = readFile(args[0]);
    print(contents);
}
```

### E.1.3. Rules and Semantics

- **One form per entry file.** If the entry file contains both top-level statements **and** a `main` function, the compiler emits `E0320` (`ambiguous entry point`). Pick one.
- **`args` excludes the program name.** Use `std.process.programName()` to get argv[0].
- **Uncaught exceptions** print to `stderr` and the process exits with a non-zero status.
- **Return from `main`** runs all destructors, flushes I/O streams, and exits with the returned status (or `0` for `void` main).
- **Async `main`** is permitted (`public async void main()`); the runtime spins up the default event loop, awaits completion, then exits. See `JUX-ASYNC-ADDENDUM-v2.md`.

---

## §E.2 — The `@entry` Annotation

When the platform requires a non-default entry symbol or calling convention, use `@entry`:

```java
@entry
public int my_program_start(int argc, CString[] argv) {
    return 0;
}
```

The annotation marks this function as the program's entry regardless of its name. Exactly **one** function per binary may carry `@entry` — duplicate `@entry` declarations are `E0321`.

### E.2.1. Custom Symbol Name

```java
@entry(symbol = "_start")
public void custom_start() {
    halt();
}
```

The linker exposes the function under the given symbol. The Jux name (`custom_start`) remains usable inside the program; the exported symbol (`_start`) is what the loader resolves.

### E.2.2. Calling Convention

For platforms with non-C calling conventions (Windows `stdcall`, system call wrappers):

```java
@entry(symbol = "DllMain", convention = "stdcall")
public int dllMain(void* hinstDLL, uint reason, void* reserved) {
    return 1;
}
```

Recognized conventions: `c` (default), `stdcall`, `fastcall`, `sysv`, `win64`, `aapcs`. Targets that don't support a convention reject it with `E0322`.

### E.2.3. Multiple Entry Points (Module-style binaries)

Some platforms expose more than one entry — Linux kernel modules export `init_module` and `cleanup_module`; Windows DLLs export `DllMain` only. For module-style binaries, declare each entry separately:

```java
@entry(symbol = "init_module")
public int kernelInit() {
    print("module loaded");
    return 0;
}

@entry(symbol = "cleanup_module")
public void kernelExit() {
    print("module unloaded");
}
```

The constraint is "one entry per emitted binary symbol," not "one `@entry` per program." For a kernel-module crate type, the compiler permits the documented set of paired entries; for a normal executable, only one is permitted.

---

## §E.3 — Freestanding Mode

Freestanding mode targets bare-metal: OS kernels, bootloaders, MCU firmware where no runtime exists. Enabled per-package in `jux.toml`:

```toml
[build]
profile = "core"
target = "thumbv7em-none-eabihf"
freestanding = true
linker_script = "stm32f4.ld"
```

### E.3.1. What Freestanding Mode Disables

- **No CRT** is linked.
- **No runtime initialization** is performed automatically. No static initializers run.
- **No automatic destructors** at program exit — there is no "exit."
- **No `std`** beyond what's available under the `core` profile (see §2.4 in the main spec). No heap, no threads, no exceptions.

### E.3.2. What Freestanding Mode Requires

- The user provides the entry symbol named by the linker script (commonly `_start` or `Reset_Handler`).
- The user is responsible for stack setup, BSS clearing, hardware initialization, and any pre-main setup.

### E.3.3. Worked Example — ARM Cortex-M Reset Handler

```java
@entry(symbol = "Reset_Handler")
public void reset_handler() {
    setupClocks();
    enableMMU();
    main_loop();
}

public void main_loop() {
    while (true) {
        readSensors();
        actuate();
        delay(100);
    }
}
```

The `Reset_Handler` symbol is wired to the vector table by the linker script (`stm32f4.ld`). Static data is initialized only if the user explicitly copies it from flash to RAM; the runtime does nothing on the user's behalf.

See §16.9 of the main spec for a complete STM32 LED-blinker example.

---

## §E.4 — Mode Summary

| Mode                 | Entry declaration                     | Runtime init | Typical use                       |
|----------------------|---------------------------------------|--------------|-----------------------------------|
| Implicit (default)   | top-level stmts OR `public ... main(...)` | Automatic    | Apps, CLI tools, services         |
| Named entry          | `@entry public ...`                   | Automatic    | Kernel modules, plugins           |
| Custom symbol        | `@entry(symbol = "...")`              | Manual       | Replacing CRT, exotic loaders     |
| Freestanding         | User-provided `_start` (or named)     | None         | OS kernels, bootloaders, MCU FW   |

---

## §E.5 — Module Entry Configuration

For multi-file projects, the entry file is configurable in `jux.toml`. Defaults to `src/main.jux` for executable crates:

```toml
[module]
name = "com.example.app"
entry = "src/cli/Main.jux"          # optional; defaults to src/main.jux
```

For library crates (no executable produced), omit `entry` or set it to the empty string. For packages with more than one binary, declare each via `[[bin]]` (see `JUX-BUILD-SYSTEM-ADDENDUM.md` §B.2.2 and §B.15.2).

Multiple binaries in one package use `[[bin]]` tables:

```toml
[[bin]]
name = "myapp"
path = "src/main.jux"

[[bin]]
name = "myapp-server"
path = "src/bin/server.jux"
```

Each `path` must contain exactly one entry (per §E.1.3 / §E.2).

---

## §E.6 — Diagnostics

| Code  | Condition                                                                    |
|-------|------------------------------------------------------------------------------|
| `E0320` | Entry file contains both top-level statements and a `main` function        |
| `E0321` | Multiple functions carry `@entry` in the same binary                       |
| `E0322` | `@entry(convention = "...")` is unsupported on the current target          |
| `E0323` | `main`'s signature does not match any accepted form                        |
| `E0324` | `@entry` function's signature is incompatible with its `symbol`'s ABI      |
| `E0325` | `freestanding = true` but no `@entry` function is declared                 |

---

## §E.7 — Integration Notes

- The `@entry` annotation joins the built-in annotation table in JUX-LANG-V1.md §3.6.
- Freestanding mode interacts with profiles defined in JUX-LANG-V1.md §2.4 — only `jux-core` may set `freestanding = true`.
- The accepted `main` signatures interact with async semantics in `JUX-ASYNC-ADDENDUM-v2.md` (top-level await initialization).
- Build-system configuration (`entry`, `[[bin]]`, `freestanding`, `linker_script`) lives in `JUX-BUILD-SYSTEM-ADDENDUM.md`.

*End of Entry Points addendum.*
