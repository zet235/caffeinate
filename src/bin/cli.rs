//! `caffeinate`, the command line companion, in the spirit of macOS
//! `caffeinate`.
//!
//! The power request is always held by this process: `SetThreadExecutionState`
//! is bound to the calling thread, and this thread then blocks in
//! `Command::status` or `thread::sleep` for exactly as long as the hold should
//! last. When the process ends, for any reason including being killed, the OS
//! releases the request with it. That is what makes the CLI correct on its own.
//!
//! If `caffeinate-tray` happens to be running, it is told about the hold so its
//! icon and menu can show it. The tray is never load bearing.

use std::io::Write;
use std::process::Command;
use std::time::Duration;

use caffeinate::{ipc, power};

const USAGE: &str = "\
caffeinate - keep Windows awake

USAGE:
    caffeinate [OPTIONS] [--] <command> [args...]
    caffeinate [OPTIONS] -t <duration>
    caffeinate [OPTIONS]

OPTIONS:
    -d, --display          Also keep the screen on. The default holds off
                           system sleep only, so the display can still blank.
    -t, --time <duration>  Hold for a period, then exit. Accepts a bare number
                           of seconds, or a suffix: 90, 30s, 45m, 2h
    -h, --help             Show this help

Given a command, the machine is held awake until that command exits, and
caffeinate returns the command's exit code. Given -t, it holds for that long.
Given neither, it holds until interrupted with Ctrl-C.

-t and a command cannot be combined. Pick one.

Use -- to stop option parsing when the command has flags of its own:
    caffeinate -- cargo build -d

If caffeinate-tray is running it is told about the hold, so the tray icon
reflects it. The power request itself is always held by caffeinate.
";

/// What the process should do while it holds the machine awake.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    /// Run a command; the hold lasts exactly as long as the command does.
    Wrap(Vec<String>),
    /// Hold for a fixed period. The `String` is the duration as typed, kept for
    /// the label shown in the tray.
    Timed(Duration, String),
    /// Hold until interrupted.
    Hold,
    Help,
}

#[derive(Debug, PartialEq, Eq)]
struct Cli {
    display: bool,
    mode: Mode,
}

/// Parse `90`, `30s`, `45m` or `2h` into a duration. A bare number is seconds,
/// matching the way macOS `caffeinate -t` reads its argument.
fn parse_duration(spec: &str) -> Result<Duration, String> {
    if spec.is_empty() {
        return Err("empty duration".to_string());
    }

    let (digits, multiplier) = match spec.as_bytes()[spec.len() - 1] {
        b's' | b'S' => (&spec[..spec.len() - 1], 1),
        b'm' | b'M' => (&spec[..spec.len() - 1], 60),
        b'h' | b'H' => (&spec[..spec.len() - 1], 3600),
        _ => (spec, 1),
    };

    if digits.is_empty() {
        return Err(format!("`{spec}` has a unit but no number"));
    }

    let value: u64 = digits
        .parse()
        .map_err(|_| format!("`{spec}` is not a duration (try 90, 30s, 45m, 2h)"))?;

    value
        .checked_mul(multiplier)
        .map(Duration::from_secs)
        .ok_or_else(|| format!("`{spec}` is too large"))
}

fn parse_args(args: &[String]) -> Result<Cli, String> {
    let mut display = false;
    let mut timed: Option<(Duration, String)> = None;
    let mut command: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-h" | "--help" => {
                return Ok(Cli {
                    display: false,
                    mode: Mode::Help,
                });
            }
            "-d" | "--display" => display = true,
            "-t" | "--time" => {
                let spec = args
                    .get(i + 1)
                    .ok_or_else(|| format!("{arg} needs a duration"))?;
                timed = Some((parse_duration(spec)?, spec.clone()));
                i += 1;
            }
            "--" => {
                if i + 1 >= args.len() {
                    // Silently turning this into an unbounded hold is the same
                    // class of surprise as a dropped -t: a script whose command
                    // expanded to nothing would sit there holding the machine
                    // awake, saying nothing, until something killed it.
                    return Err("`--` with no command after it".to_string());
                }
                command.extend_from_slice(&args[i + 1..]);
                break;
            }
            // The first non-option argument starts the command, and everything
            // after it belongs to the command rather than to us.
            _ if !arg.starts_with('-') => {
                command.extend_from_slice(&args[i..]);
                break;
            }
            _ => return Err(format!("unknown option `{arg}`")),
        }
        i += 1;
    }

    let mode = match (timed, command.is_empty()) {
        // macOS quietly ignores -t when a command is given. Refusing is
        // clearer: a silently dropped time limit is worse than an error.
        (Some(_), false) => {
            return Err("-t and a command cannot be combined. Pick one.".to_string());
        }
        (Some((duration, spec)), true) => Mode::Timed(duration, spec),
        (None, false) => Mode::Wrap(command),
        (None, true) => Mode::Hold,
    };

    Ok(Cli { display, mode })
}

/// Text the tray shows for this hold.
fn label(mode: &Mode) -> String {
    match mode {
        Mode::Wrap(cmd) => cmd.join(" "),
        Mode::Timed(_, spec) => format!("hold {spec}"),
        Mode::Hold => "hold".to_string(),
        Mode::Help => String::new(),
    }
}

fn main() {
    let code = run_cli();
    // process::exit skips the usual cleanup, including the flush that would
    // normally happen when main returns.
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // Not ExitCode: that is a u8, and Windows exit codes are 32 bits wide.
    // Truncating them turns `exit 256` into a success, which would quietly
    // hide a failed build from any script that wrapped it.
    std::process::exit(code);
}

fn run_cli() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let cli = match parse_args(&args) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("caffeinate: {message}");
            eprintln!("Try `caffeinate --help`.");
            return 2;
        }
    };

    if cli.mode == Mode::Help {
        print!("{USAGE}");
        return 0;
    }

    // The hold belongs to this process from here until the end of main.
    if !power::apply(true, cli.display) {
        eprintln!("caffeinate: the system refused the power request; continuing anyway");
    }

    let pid = std::process::id();
    ipc::send(&ipc::Wire::new(
        ipc::KIND_ACQUIRE,
        pid,
        cli.display,
        &label(&cli.mode),
    ));

    let code = match &cli.mode {
        Mode::Wrap(command) => run(command),
        Mode::Timed(duration, _) => {
            std::thread::sleep(*duration);
            0
        }
        Mode::Hold => {
            // Nothing to wait on, so park until Ctrl-C kills the process. The
            // OS releases the power request as it goes.
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        }
        Mode::Help => unreachable!("handled above"),
    };

    // Sent unconditionally rather than only when the acquire was accepted: a
    // tray that started after this process did never saw the acquire, but it
    // may well be running now, and a release it does not recognise is a no-op.
    ipc::send(&ipc::Wire::new(ipc::KIND_RELEASE, pid, cli.display, ""));
    power::apply(false, false);

    code
}

/// Run the wrapped command and return its exit code.
fn run(command: &[String]) -> i32 {
    match Command::new(&command[0]).args(&command[1..]).status() {
        // On Windows this is the full 32-bit exit code, including values like
        // 0xC0000005 for a crash.
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("caffeinate: cannot run `{}`: {e}", command[0]);
            // 127 is the shell convention for "command not found".
            127
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn bare_numbers_are_seconds() {
        assert_eq!(parse_duration("90").unwrap(), Duration::from_secs(90));
        assert_eq!(parse_duration("0").unwrap(), Duration::ZERO);
    }

    #[test]
    fn suffixes_scale_the_value() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("45m").unwrap(), Duration::from_secs(2700));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
    }

    #[test]
    fn suffixes_are_case_insensitive() {
        assert_eq!(parse_duration("2H").unwrap(), parse_duration("2h").unwrap());
        assert_eq!(parse_duration("5M").unwrap(), parse_duration("5m").unwrap());
    }

    #[test]
    fn rejects_nonsense_durations() {
        for bad in ["", "h", "abc", "-5", "1d", "1.5h", "10 m"] {
            assert!(parse_duration(bad).is_err(), "`{bad}` should be rejected");
        }
    }

    #[test]
    fn rejects_durations_that_overflow() {
        assert!(parse_duration(&format!("{}h", u64::MAX)).is_err());
    }

    #[test]
    fn no_arguments_means_hold() {
        let cli = parse_args(&args(&[])).unwrap();
        assert_eq!(cli.mode, Mode::Hold);
        assert!(!cli.display);
    }

    #[test]
    fn display_flag_is_recognised_in_both_spellings() {
        assert!(parse_args(&args(&["-d"])).unwrap().display);
        assert!(parse_args(&args(&["--display"])).unwrap().display);
    }

    #[test]
    fn a_command_becomes_wrap_mode() {
        let cli = parse_args(&args(&["cargo", "build"])).unwrap();
        assert_eq!(cli.mode, Mode::Wrap(args(&["cargo", "build"])));
    }

    #[test]
    fn options_after_the_command_belong_to_the_command() {
        // -d here is cargo's, not ours, because it comes after the command.
        let cli = parse_args(&args(&["cargo", "build", "-d"])).unwrap();
        assert!(!cli.display);
        assert_eq!(cli.mode, Mode::Wrap(args(&["cargo", "build", "-d"])));
    }

    #[test]
    fn double_dash_stops_option_parsing() {
        let cli = parse_args(&args(&["-d", "--", "cargo", "-t", "x"])).unwrap();
        assert!(cli.display, "-d before -- is still ours");
        assert_eq!(cli.mode, Mode::Wrap(args(&["cargo", "-t", "x"])));
    }

    #[test]
    fn time_flag_produces_timed_mode() {
        let cli = parse_args(&args(&["-t", "45m"])).unwrap();
        assert_eq!(
            cli.mode,
            Mode::Timed(Duration::from_secs(2700), "45m".to_string())
        );
    }

    #[test]
    fn time_and_a_command_are_refused() {
        let err = parse_args(&args(&["-t", "1h", "cargo", "build"])).unwrap_err();
        assert!(err.contains("cannot be combined"), "got: {err}");
    }

    #[test]
    fn time_without_a_value_is_refused() {
        assert!(parse_args(&args(&["-t"])).is_err());
    }

    #[test]
    fn unknown_options_are_refused() {
        let err = parse_args(&args(&["-z"])).unwrap_err();
        assert!(err.contains("unknown option"), "got: {err}");
    }

    #[test]
    fn help_wins_over_everything_else() {
        assert_eq!(
            parse_args(&args(&["-d", "--help"])).unwrap().mode,
            Mode::Help
        );
        assert_eq!(parse_args(&args(&["-h"])).unwrap().mode, Mode::Help);
    }

    #[test]
    fn a_bare_double_dash_is_refused() {
        let err = parse_args(&args(&["--"])).unwrap_err();
        assert!(err.contains("no command"), "got: {err}");
        assert!(parse_args(&args(&["-d", "--"])).is_err());
    }

    #[test]
    fn labels_describe_the_hold() {
        assert_eq!(label(&Mode::Wrap(args(&["cargo", "build"]))), "cargo build");
        assert_eq!(
            label(&Mode::Timed(Duration::from_secs(60), "1m".to_string())),
            "hold 1m"
        );
        assert_eq!(label(&Mode::Hold), "hold");
    }
}
