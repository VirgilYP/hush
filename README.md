# hush

[English](README.md) | [中文](README.zh-CN.md)

`hush` is a prompt-aware wrapper for `tio`-style UART sessions with noisy device
logs.

It is useful when a device continuously prints background logs while you need to
type shell commands. The console can briefly stop the log stream at a prompt,
let you type a command, flush the paused logs first, and then send your command
to the device.

The repository also contains a macOS-first Tauri GUI with a UART / USB HID receive terminal,
manual send bar, and an SSCOM-style editable command bank.

## macOS GUI

The GUI keeps UART / HID ownership and byte handling in Rust. The webview is only
responsible for presentation and invokes the shared `hush` library instead of
launching or scraping the CLI as a subprocess.

```bash
cd apps/hush-gui
npm install
npm run tauri dev
```

Build a local macOS application bundle:

```bash
npm run tauri build -- --bundles app
open src-tauri/target/release/bundle/macos/Hush.app
```

The development bundle is ad-hoc signed. External distribution still requires
an Apple Developer signature and notarization.

### DM30 defaults

The first-run command bank is `DM30`:

- UART1: 115200 baud, 8 data bits, no parity, 1 stop bit
- a top-level `UART` / `USB HID` selector; HID lists legacy DM30 `1ACC:1A61`
  and current DM30 `1ACC:1AEC`, uses
  report ID 1, and accepts up to 63 command bytes including the line ending
- text line ending: CR
- 14 editable commands: power on, full configuration report, documented
  gain/report-off commands, and two explicitly warned IN1 32.0→31.9 dB
  transition-reproduction commands
- one-click `Silence UART reports` sends the six module-specific `rpsw=off`
  commands without changing gain
- a safe `-200..0 dB` gain test slider selects one of the ten documented gain
  module IDs and updates the matching editable command; `Apply` performs the send
- an Algorithm selector groups every active `sw=on/off` module by audio path and
  sends only source-verified MID/switch combinations
- `/dev/cu.*` callout devices are shown on macOS; saved `/dev/tty.*` peers are
  migrated when a matching callout device exists

All slots remain editable. Text and HEX slots can be added, removed, or
reordered. `Load` and `Export` exchange a versioned JSON command-bank file that
contains the bank name and slots, but deliberately excludes machine-specific
device paths.

The current DM30 firmware accepts `set mid=-2,power=on` from UART / shell only,
and USB HID does not enumerate until the unit is powered. Hush disables that
slot in HID mode; the remaining DSP commands share the same editable slots.

The receive stream is forwarded every few milliseconds and appended on the
next display frame, matching the continuous feel of the CLI without rebuilding
the entire terminal DOM. Raw UART bytes are written to `/tmp/hush-gui-*.log`;
raw HID input reports are written to `/tmp/hush-gui-hid-*.log`.
Pausing or clearing the display does not modify either log.

Only one program can own a serial device. Close any CLI terminal that already
has the port open before connecting the GUI.

Scrolling the receive view away from the bottom activates Smart Pause before
new data can steal the scroll position. Returning to the bottom resumes only a
scroll-triggered pause; a manual pause remains manual.

## Relationship to tio

This tool is intended as a small wrapper around the `tio` workflow, not as a
replacement for `tio`'s full feature set.

Use `tio` directly for ordinary serial sessions. Use `hush` when the device keeps
printing background logs and you need a prompt-aware interaction layer on top of
that familiar serial-console flow. The key bindings intentionally keep the
`Ctrl-T` prefix style used by `tio`.

## Install

```bash
cargo install --path .
```

Or build a release binary:

```bash
cargo build --release
```

To install the bundled Codex skill for UART monitor workflows:

```bash
scripts/install-codex-skill.sh
```

By default this installs to `$CODEX_HOME/skills/hush-uart-console`, or
`~/.codex/skills/hush-uart-console` when `CODEX_HOME` is not set. To install the
skill somewhere else:

```bash
scripts/install-codex-skill.sh --dest /path/to/skills
scripts/install-codex-skill.sh --target /path/to/hush-uart-console --force
```

## Usage

```bash
hush /dev/cu.usbmodem01234567895 -b 3000000
```

You can also provide the device through an environment variable:

```bash
export HUSH_DEVICE=/dev/cu.usbmodem01234567895
hush -b 3000000
```

By default the tool writes a temporary log file under `/tmp`, for example:

```text
/tmp/hush-1779081234.log
```

The log file keeps the original UART bytes. Display-only cleanup does not modify
the log.

The primary session also opens a local monitor socket based on the serial device
basename. For the command above, another terminal can attach with:

```bash
hush monitor usbmodem01234567895
```

The monitor window mirrors the primary `hush` screen and forwards its keyboard
input into the primary session. It does not open the serial device itself.

## Interaction Model

Normal mode:

```text
device logs print in real time
```

Press an empty `Enter`:

```text
send one line ending to the device
wait until a '$' prompt is seen
display the prompt
pause further device output
```

Type a command and press `Enter`:

```text
clear the local input line
flush the paused device output to the screen
send your command to the device
resume real-time output
```

This keeps old background logs from appearing after the command response.

## Keys

```text
Empty Enter    Send a newline, wait for '$', then pause at the prompt
Enter          Flush paused output, then send the typed line
Up / Down      Select an older / newer command from persistent history
Ctrl-U         Clear current input
Backspace      Delete one input character
Ctrl-C         Send Ctrl-C to the device
Ctrl-T r       Resume real-time output
Ctrl-T q       Quit
Ctrl-T l       Clear screen
Ctrl-T ?       Show help
```

## Monitor Windows

Start `hush` normally in the terminal that owns the serial device:

```bash
hush /dev/cu.usbmodem01234567895 -b 3000000
```

Then attach from another terminal:

```bash
hush monitor usbmodem01234567895
```

Input typed in the monitor is forwarded to the primary `hush` session and uses
the same prompt-aware state machine as the primary terminal. `Ctrl-T` sequences
are forwarded too, so `Ctrl-T r`, `Ctrl-T l`, `Ctrl-T ?`, and `Ctrl-T q` control
the primary `hush` session.

To detach only the monitor window, press `Ctrl-]`.

If the primary `hush` session exits or its terminal is closed, attached monitor
windows exit automatically after the monitor socket closes.

For a view-only monitor:

```bash
hush monitor --read-only usbmodem01234567895
```

To list active monitorable sessions:

```bash
hush sessions
hush monitor --list
```

You can choose a stable session name when starting the primary session:

```bash
hush /dev/cu.usbmodem01234567895 -b 3000000 --session dut-a
hush monitor dut-a
```

## Shared Agent Workflow

For AI-assisted debugging, keep the primary serial terminal under the user's
control and let the agent attach through a monitor:

```bash
# User terminal
hush /dev/cu.usbmodem01234567895 -b 3000000 --session dut-c

# Agent terminal
hush sessions
hush monitor dut-c
```

The agent can type commands through the monitor and the user can watch every
command and device response in the primary terminal. Because both terminals feed
the same input stream, avoid typing from both windows at the same time.

Use `hush monitor --read-only dut-c` when the agent should observe without
driving the device.

## Options

```text
-d <device>              Serial device. Positional <device> is also accepted.
-b <baud>                Baud rate. Default: 3000000.
-l <logfile>             Log file path. Default: /tmp/hush-*.log.
--session <name>         Monitor session name. Default: serial device basename.
--newline cr|lf|crlf     Command line ending. Default: cr.
```

## Notes

- Prompt detection currently uses the `$` character.
- Command history persists across `hush` sessions in
  `$XDG_STATE_HOME/hush/history`, or `~/.local/state/hush/history` when
  `XDG_STATE_HOME` is unset. The most recent 1000 commands are loaded into each
  session. Pressing Down past the newest command restores the input draft from
  before navigation.
- The default command line ending is carriage return (`cr`, `\r`), which is
  common for embedded UART shells.
- If your shell expects LF or CRLF, use `--newline lf` or `--newline crlf`.
