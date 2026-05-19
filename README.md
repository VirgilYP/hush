# uart-prompt-console

`uart-prompt-console` is a small Rust serial console for noisy UART sessions.

It is useful when a device continuously prints background logs while you need to
type shell commands. The console can briefly stop the log stream at a prompt,
let you type a command, flush the paused logs first, and then send your command
to the device.

## Install

```bash
cargo install --path .
```

Or build a release binary:

```bash
cargo build --release
```

## Usage

```bash
uart-prompt-console /dev/cu.usbmodem01234567895 -b 3000000
```

You can also provide the device through an environment variable:

```bash
export UART_PROMPT_DEVICE=/dev/cu.usbmodem01234567895
uart-prompt-console -b 3000000
```

By default the tool writes a temporary log file under `/tmp`, for example:

```text
/tmp/uart-prompt-console-1779081234.log
```

The log file keeps the original UART bytes. Display-only cleanup does not modify
the log.

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

## Options

```text
-d <device>              Serial device. Positional <device> is also accepted.
-b <baud>                Baud rate. Default: 3000000.
-l <logfile>             Log file path. Default: /tmp/uart-prompt-console-*.log.
--newline cr|lf|crlf     Command line ending. Default: cr.
```

## Notes

- Prompt detection currently uses the `$` character.
- The default command line ending is carriage return (`cr`, `\r`), which is
  common for embedded UART shells.
- If your shell expects LF or CRLF, use `--newline lf` or `--newline crlf`.
