# VoidLatch

A small, standalone tool for learning how dll injection works. It:

1. Scans for running **Minecraft (Java Edition)** processes.
2. Lets you pick a `.dll` / `.so` from disk with a native file dialog, or from
   your home `dll` folder.
3. Injects it into the Minecraft process you select.

---

## The window

Two tabs:

| Tab | What it does |
|---|---|
| **Targets** | Scan for Minecraft processes, pick one, and choose a dll with the native **Browse…** dialog. |
| **Dll** | Lists every dll in your home `dll` folder, ready to inject. |

### The `dlls` folder

VoidLatch watches a folder called `dlls` in your home directory
(`C:\Users\<you>\dlls`, `~/dlls`). It is created on first launch if missing.

Drop any `.dll` in there and it appears in the **Dll** tab with its size.
From there you can:

- click a row to highlight it, then press the **Inject** button — no file
  dialog, no second confirmation, the injection starts immediately;
- or just **double-click** a row to inject it on the spot.

The folder name is matched case-insensitively, so an existing `DLLS` or `Dlls`
folder is used as-is. A folder called `dll` from an older release is still
recognised when no `dlls` folder exists.

Press **Change…** to watch a different folder entirely — the choice is saved
to `~/.voidlatch-dll-folder` and reused next launch. **Rescan** re-reads the
current folder without changing it.

The **Browse…** button on the Targets tab is unchanged — it is still there for
dlls that live anywhere else on disk.
---

## Build

```bash
cargo build --release
```

Requires Rust 1.80+ (2021 edition). Binary lands at `target/release/minecraft-injector.exe`
on Windows, or `target/release/minecraft-injector` on Linux.

## Run

**GUI (the main path):**

```bash
target\release\minecraft-injector.exe
```

Right-click → **Run as administrator**, then:

1. Click **Scan**.
2. Click the Minecraft process in the list.
3. Choose a dll — either **Browse…** for one anywhere on disk, or drop it
   into your home `dll` folder and use the **Dll** tab.
4. Click **Inject**.

**Headless:**

```bash
minecraft-injector --list                        # list Minecraft PIDs (no elevation needed)
minecraft-injector --inject <pid> <path\to\lib.dll>
```

`--list` only reads process info, so it does not require Administrator. Everything
else does, because injection attaches to another process.

---

## Verifying what actually happened

`LoadLibraryW` returning non-zero only tells you the target *accepted* the load.
It says nothing about what your dll did next — a payload can map perfectly
and still kill the process a moment later.

So after every injection the tool re-reads the target's module list from the
outside (ToolHelp `CreateToolhelp32Snapshot`) and reports one of three states:

| Status | Meaning |
|---|---|
| **Injected — verified in module list** (green) | The dll is mapped and the target is alive. The injector did its job. |
| **Injected, but NOT in the module list** (red) | The dll loaded and the target then died. The problem is your payload, not the injector. |
| **Error…** (red) | The injection itself failed — see the message. |

Headless mode does the same and uses the exit code to say the same thing:
`0` verified, `2` loaded but not present (target likely crashed), `1` injection failed.

This distinction is the fastest way to tell *"my injector is broken"* apart from
*"my DLL crashes the game"*.

### Minecraft version does not matter to this tool

The injector never parses classes, mappings, or game versions — it only writes a
path into the target and calls `LoadLibraryW`. It works identically against
Minecraft 1.8.9, 1.21, or any other Java application.

Version compatibility is entirely your **payload's** responsibility. If your DLL
targets 1.8.9 but the JVM is Java 21, or the DLL was built for 1.21 mappings and
you are running 1.8.9, the target will crash — and the status line above will
tell you so.

---

## Why elevation is required

Injection means writing into *another* process's address space, so you must be
able to open that process for write access:

| OS | Mechanism | Needs |
|---|---|---|
| **Windows** | Remote `LoadLibraryW` thread — `OpenProcess`, `VirtualAllocEx`, `WriteProcessMemory`, `CreateRemoteThread`. Implemented directly against `windows-sys` in `src/platform/windows.rs`, so every step is readable | Administrator |
| **Linux** | `ptrace-inject` — attaches, `mmap`s the `.so` into the target, and forces the dynamic linker to run | root / `CAP_SYS_PTRACE` |
| **macOS** | Not implemented (`src/platform/macos.rs` is a stub) | — |

On Windows the code is deliberately unabstracted — it's the single most useful
thing to read if you're studying this. It also means the project builds on
**stable Rust**; the `dll-syringe` crate it replaced requires nightly.

The tool **checks** for these privileges and exits with an explanation if they're
missing. It never attempts to acquire or escalate them — there is no UAC bypass,
no token manipulation, no `SeDebugPrivilege` use.

---

## Scope and limits

This is deliberately narrow, and worth being explicit about:

- **Minecraft processes only.** The scan in `src/discovery.rs` keeps a process only
  if its executable is `java`/`javaw` **and** its command line mentions "minecraft".
  There is no way to point this at arbitrary software — a process that isn't in
  the list can't be selected.
- **You pick the dll explicitly.** There's no bundled payload, no default, and
  no download. The tool only ever loads a file you named: either chosen via the
  file dialog, or clicked in your home `dll` folder. It never picks one for you.
- **No network code.** No HTTP client, no sockets, no DNS. It cannot phone home
  because it has no way to talk to anything.
- **No stealth.** Nothing here hides the process, hides the mapped module, spoofs
  anything, or defeats anti-cheat or antivirus. It is a visible, ordinary
  injection — exactly what you'd want when studying the technique.

---

## What your injected dll needs to do

The tool loads your dll and lets the OS run its entry point. It does not call
any function on your behalf, so your dll must be self-starting:

- **Windows** — export `DllMain`, or use the CRT/`DllMain`-equivalent for your
  language (Rust `cdylib` works; see below).
- **Linux** — an ELF `.so` with a constructor (Rust `cdylib` with `#[ctor]`, or a
  `__attribute__((constructor))` in C).

On both platforms the dll runs code on load via the `ctor` crate:

```toml
# mylib/Cargo.toml
[lib]
name = "mylib"
crate-type = ["cdylib"]

[dependencies]
ctor = "0.2"
```

```rust
// mylib/src/lib.rs
#[ctor::ctor]
fn on_load() {
    // runs when the dll is mapped into the target
}
```

`cargo build --release` produces `target/release/mylib.dll` (Windows) or
`.so` (Linux). Nothing else is required — in particular **you do not need to
export `DllMain`**. On Windows the loader calls the PE entry point
(`DllMainCRTStartup`), so a Rust `cdylib` with an empty export table loads
perfectly well. Confirm it is 64-bit with:

```
dumpbin /headers mylib.dll   →  machine (x64), magic (PE32+)
```

A 64-bit DLL will not load into a 32-bit JVM, and vice versa. This is the most
common reason an injection appears to succeed and then does nothing.

Inject it by hand to confirm the mechanism: `target\release\minecraft-injector.exe --inject 12345 mylib.dll`.

---

## Project layout

```
minecraft-injector/
├── Cargo.toml
└── src/
    ├── main.rs              # entry point, CLI arg handling, headless mode
    ├── app.rs               # UI-agnostic state machine (scan / select / inject)
    ├── discovery.rs         # Minecraft process detection (+ unit tests)
    ├── dll.rs               # the `dlls` folder and its contents (+ unit tests)
    ├── gui/
    │   ├── mod.rs           # egui front-end: Targets + Dll tabs, file picker
    │   └── theme.rs         # dark theme, frames, shared text styles
    └── platform/
        ├── mod.rs           # injection trait + error type + shared validation
        ├── windows.rs       # dll-syringe backend
        ├── linux.rs         # ptrace backend
        └── macos.rs         # unsupported stub
```

## Tests

```bash
cargo test
```

Covers the process-classification logic, dll-path validation, argument
parsing, the `dlls` folder listing and filtering, and app-state transitions. No
privileges or running game required.

---

## Safety notes

- **Only inject code you trust.** The injected dll runs with the full
  privileges of the Minecraft process. If you inject into a game you launched as
  Administrator, so does the injected code — anything it does, it can do with
  Administrator rights.
- **This violates Minecraft's Terms of Service.** Do not use it on multiplayer
  servers or any account you care about; you will be banned.
- **Injection crashes the game.** A malformed dll, a wrong CPU architecture
  (32/64-bit mismatch is the usual cause), or a dll that throws during load
  will take the JVM down with it.