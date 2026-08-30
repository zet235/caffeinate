# caffeinate

A Windows tray utility that holds `SetThreadExecutionState` so the machine does
not sleep. Rust, `x86_64-pc-windows-gnu`, no Visual Studio.

## Conventions

- **Code comments and doc comments are English.** User-facing strings are
  bilingual and live only in `src/i18n.rs`; never hard-code a visible string
  anywhere else.
- Every `unsafe` block carries a `// SAFETY:` comment saying why it is sound.
  `unsafe` is confined to the Win32 layer.

## Build environment

The target is `x86_64-pc-windows-gnu`, and **mingw has to be on `PATH`**. It is
not optional: on the gnu target `windows-sys` shells out to `dlltool.exe` to
generate import libraries, and `winresource` needs `windres.exe`. Without them
the build stops at

```
error calling dlltool 'dlltool.exe': program not found
```

which reads like a Rust problem but is not. A plain `rustup-gnu` install is not
enough on its own.

If the toolchain came from scoop's `rustup-gnu`, it lives inside scoop's persist
directory and depends on user level `RUSTUP_HOME` / `CARGO_HOME`. A shell that
inherited an older environment will not have them, and `cargo` then fails with
`could not choose a version of cargo to run`. In that case set:

```
RUSTUP_HOME=%USERPROFILE%\scoop\persist\rustup-gnu\.rustup
CARGO_HOME=%USERPROFILE%\scoop\persist\rustup-gnu\.cargo
PATH=%CARGO_HOME%\bin;%USERPROFILE%\scoop\apps\mingw\current\bin;%PATH%
```

## Architecture rules

- **No background threads.** `SetThreadExecutionState` binds its state to the
  calling thread, so power requests, the tray icon and the message loop all sit
  on `main`'s thread. The countdown runs on `WM_TIMER`.
- **All decisions live in `src/state.rs`**, which pulls in no Win32 types and is
  fully covered by `cargo test`. The Win32 layers below it only carry out what
  the state says; they never branch on their own.
- **`Ui::sync` is the only path that updates the UI.** Anything that mutates
  `AppState` must call it; no scattered menu updates.

## Traps already paid for

Do not undo these. Each one fails silently.

- `SetThreadExecutionState` needs `ES_CONTINUOUS` on every call, or it resets
  the idle timer once instead of holding the state.
- `SetTimer` with a null `hwnd` ignores the id argument and returns its own.
  Compare `WM_TIMER`'s `wParam` against the returned value, never a constant.
- Win32 popup menus render light until the process calls uxtheme ordinals 135
  and 136 (`src/theme.rs`), before any menu is created.
- Without the manifest in `assets/`, the process is DPI unaware and Windows
  bitmap-stretches the menu, blurring the text at any scaling but 100%.

## Verification

`cargo test` covers the state machine and the power flag helper. The Win32
layer is checked by hand; `README.md` documents both the administrator
(`powercfg /requests`) and non-administrator (flag read-back) methods.
