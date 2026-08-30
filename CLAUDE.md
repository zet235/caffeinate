# caffeinate

A Windows tray utility that holds `SetThreadExecutionState` so the machine does
not sleep. Rust, `x86_64-pc-windows-gnu`, no Visual Studio.

## Conventions

- **Code comments and doc comments are English.**
- **The tray's user-facing strings are bilingual and live only in
  `src/i18n.rs`.** Never hard-code a visible string anywhere else in the tray,
  and never compose one by concatenation outside `i18n.rs`: word order and
  punctuation differ between the two languages, which is why the composed
  strings are function pointers.
- **The CLI is English only**, like every other command line tool, so its
  `--help` and diagnostics live in `src/bin/cli.rs`. It does not link
  `i18n.rs`, which belongs to the tray binary.
- Every `unsafe` block carries a `// SAFETY:` comment saying why it is sound.
  `unsafe` is confined to the Win32 layer.

## Commits

Every commit message follows [Conventional Commits
v1.0.0-beta.2](https://www.conventionalcommits.org/en/v1.0.0-beta.2/):

```
<type>[optional scope]: <description>

[optional body]

[optional footer]
```

- `type` is `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`,
  `ci`, or `chore`. `feat` and `fix` are the only ones that map to a release.
- The description is lower case, imperative mood, and carries no trailing full
  stop: `fix(ipc): reject a payload whose cbData does not match Wire`.
- `scope` is the area touched, normally a module: `state`, `ipc`, `leases`,
  `tray`, `theme`, `i18n`, `power`, `cli`, `build`, `ci`.
- A breaking change is marked with `BREAKING CHANGE:` in the footer, or a `!`
  after the type/scope.

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
- **The countdown ticks from a `TIMERPROC`, not from a `WM_TIMER` the message
  loop reads.** While a popup menu is open Win32 runs its own modal message
  loop, and that loop drains the queue: a bare `WM_TIMER` is dispatched there
  and never reaches our `GetMessageW`, so the clock stops for as long as the
  menu stays open and the machine sits awake past its deadline. A `TIMERPROC`
  is invoked *by* `DispatchMessageW`, so the modal loop runs it too.
- `SetTimer` with a null `hwnd` ignores the id argument and returns its own.
  That returned value is what `KillTimer` needs.
- Win32 popup menus render light until the process calls uxtheme ordinals 135
  and 136 (`src/theme.rs`), before any menu is created.
- Without the manifest in `assets/`, the process is DPI unaware and Windows
  bitmap-stretches the menu, blurring the text at any scaling but 100%.

## Verification

`cargo test` covers the state machine and the power flag helper. The Win32
layer is checked by hand; `README.md` documents both the administrator
(`powercfg /requests`) and non-administrator (flag read-back) methods.
