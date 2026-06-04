# hush

[English](README.md) | [中文](README.zh-CN.md)

`hush` is a prompt-aware wrapper for `tio`-style UART sessions with noisy device
logs.

It is useful when a device continuously prints background logs while you need to
type shell commands. The console can briefly stop the log stream at a prompt,
let you type a command, flush the paused logs first, and then send your command
to the device.

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
- The default command line ending is carriage return (`cr`, `\r`), which is
  common for embedded UART shells.
- If your shell expects LF or CRLF, use `--newline lf` or `--newline crlf`.
