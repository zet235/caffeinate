# caffeinate

Keep Windows awake from the system tray. Tick a box, the machine stops sleeping
and the screen stays on. Close the program and everything is exactly as it was.

No power plan is modified and no key presses are simulated. It uses the
documented [`SetThreadExecutionState`][stes] API, which is the same mechanism
video players and installers use to hold a machine awake.

[stes]: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate

## Install

Download `caffeinate-tray.exe` from the [releases page][releases] and run it.
It is a single file with no installer and no runtime to install. Nothing is
written to the registry and no configuration file is created.

[releases]: https://github.com/zet235/caffeinate/releases

## Use

Left click or right click the tray icon:

```
☑ Keep system awake      holds ES_SYSTEM_REQUIRED
☑ Keep screen on         holds ES_DISPLAY_REQUIRED
─────────────
Duration  ▸   ☑ Indefinitely
              ☐ 5 / 10 / 15 / 30 minutes
              ☐ 1 / 2 / 5 hours
─────────────
00:29:31 remaining
─────────────
Exit
```

- The two switches are independent. While downloading something large you can
  keep only the system awake and let the screen turn off as usual.
- When the countdown expires both switches turn off together and the duration
  resets to Indefinitely.
- Choosing a duration while both switches are off only records the choice. The
  clock starts when a switch is turned on.
- Turning on a second switch mid countdown does not restart it. Choosing a
  different duration does.
- Every launch starts in the off state. Nothing is remembered between runs and
  nothing is added to startup.
- Launching a second copy does nothing, so there is never a duplicate icon.

The interface follows the system display language: Chinese on a Chinese
Windows, English everywhere else. This is not only a preference. A Win32 menu
takes its font from the system-wide `lfMenuFont`, which on an English Windows
is Segoe UI, and Segoe UI has no CJK glyphs. Chinese text then falls through
GDI font linking into a Japanese face whose bitmap glyphs look poor at menu
sizes.

## Verify that it works

With an **administrator** terminal:

```
powercfg /requests
```

While a switch is on, the `SYSTEM:` and `DISPLAY:` sections list
`caffeinate-tray.exe`. After turning it off or exiting they return to `None`.

Without administrator rights there is a second way.
`SetThreadExecutionState` returns the state as it was before the call, so the
flags can be read back: with both switches on you get `0x80000003`
(`ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED`), and `0x80000000`
with everything off.

## Build

A Rust toolchain and mingw are required. Visual Studio is not.

```
scoop install main/rustup-gnu
scoop install main/mingw
cargo build --release
```

`windows-sys` needs mingw's `dlltool.exe` on the gnu target and `winresource`
needs `windres.exe`, so mingw has to be on `PATH`.

The result is `target/release/caffeinate-tray.exe`, one self-contained file of
roughly 360 KB.

### Icons

The tray icons are drawn in code by `tools/gen_icons.py` (pure Python, no
dependencies), so there is no third-party artwork involved:

```
python tools/gen_icons.py
```

Change the two RGB values in `main()` for different colours, or drop your own
`.ico` files into `assets/`.

## Notes for anyone changing this

Three things in here are easy to get wrong and hard to notice afterwards, and
each one fails silently. Read them before touching the Win32 layer.

- `SetThreadExecutionState` must be called with `ES_CONTINUOUS` every time.
  Without it the call resets the idle timer once instead of holding the state,
  so the program looks fine and the machine sleeps anyway a few minutes later.
- `SetTimer` with a null `hwnd` **ignores the id you pass** and assigns its
  own, returning it. `WM_TIMER` carries that id in `wParam`, so the comparison
  must use the returned value. Getting this wrong makes the countdown silently
  do nothing, and no unit test catches it because the bug is in the wiring.
- `SetThreadExecutionState` is bound to the calling thread, which is why this
  program starts no background threads at all. The countdown runs on `WM_TIMER`
  inside the main message loop.

## Roadmap

- `caffeinate.exe`, a command line companion in the style of macOS
  `caffeinate`, so a long build can be wrapped: `caffeinate cargo build`.
- A Scoop manifest.

## Known limitations

Some Modern Standby (S0ix) machines, and corporate group policy, can still
force the display off or the machine to sleep. That is system level behaviour
no user mode program can override.

## License

MIT. See [LICENSE](LICENSE).
