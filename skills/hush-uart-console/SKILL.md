---
name: hush-uart-console
description: Use this when Codex needs to connect to, monitor, drive, or capture logs from a local UART/serial console through hush, especially when the user says hush, UART, serial, monitor my terminal, capture logs, inspect serial output, or avoid a noisy terminal. Prefer hush monitor sessions over opening the serial device directly when the user already owns the primary terminal.
---

# Hush UART Console

Use `hush` for noisy UART sessions where a normal serial console gets flooded by
background logs while the user or agent needs prompt-style command interaction.

## Prefer hush

- Use `hush` when command input must coexist with continuous UART logs.
- Use `hush monitor <session>` when the user already opened the primary serial
  terminal. Do not open the serial device a second time.
- Use `hush monitor --read-only <session>` when Codex should observe without
  sending input.
- Fall back to other serial tools only when `hush` is unavailable, cannot open
  the device, or the user explicitly asks for another tool.

## User-owned primary workflow

Preferred collaborative flow:

```bash
# User terminal owns the serial device.
hush /dev/cu.usbmodem01234567895 -b 3000000 --session dut-c

# Codex terminal attaches without opening the serial device.
hush monitor dut-c
```

Before attaching, discover available sessions:

```bash
hush sessions
hush monitor --list
```

Rules:

- If the user says they already opened `hush`, attach with `hush monitor
  <session>` instead of opening `/dev/cu.*`, `/dev/tty*`, or another UART path.
- Tell the user that the primary terminal and monitor share one input stream.
  Avoid typing while the user is typing.
- Interactive monitor input is forwarded to the primary `hush` session and then
  to the device. The user can see Codex's commands and device responses in the
  primary terminal.
- `Ctrl-]` detaches only Codex's monitor. `Ctrl-T q` closes the primary `hush`
  session, so use it only when intentionally ending the primary session.
- If the monitor exits with `monitor ended: primary hush exited`, report that
  the primary terminal closed or exited. Do not reopen the serial device unless
  the user asks Codex to own a new primary session.

## Agent-owned primary workflow

Use this only when the user has not opened a primary UART terminal, or when they
explicitly ask Codex to own the serial session.

```bash
hush /dev/cu.usbmodem01234567895 -b 3000000 -l /tmp/hush.log
```

Then:

- Wait long enough to capture the relevant boot or runtime lines.
- If a prompt is needed, send an empty Enter to the `hush` session.
- Quit the primary session with `Ctrl-T q` when done.
- Inspect the temporary log with `rg`, `tail`, or `less`.

Keep capture logs under `/tmp` unless the user asks for a durable path.

## Convenience wrappers

If local wrapper commands such as `hush-a`, `hush-b`, `hush-c`, or `hush-d` are
installed, prefer stable session names so agents can discover them:

```sh
exec hush /dev/cu.usbmodem01234567895 -b 3000000 --session dut-c "$@"
```

With that wrapper, the user starts:

```bash
hush-c
```

Codex discovers and attaches:

```bash
hush sessions
hush monitor dut-c
```

## Interaction model

```text
normal mode: UART output streams live
empty Enter: send newline, wait for '$', then pause at the prompt
type command
Up/Down: select an older/newer command from persistent history
Enter: flush paused output first, then send the typed command
Ctrl-T r: resume realtime output
Ctrl-T q: quit the primary hush session
monitor input: forwarded to the primary hush session
monitor Ctrl-]: detach only the monitor window
monitor auto-exit: primary hush exited or terminal closed
```

## Command history

Commands sent with `Enter` persist across `hush` restarts. `Up` selects older
commands, `Down` selects newer commands, and pressing `Down` past the newest
command restores the unsent draft from before history navigation.

History is stored at `$XDG_STATE_HOME/hush/history` when `XDG_STATE_HOME` is set,
otherwise at `~/.local/state/hush/history`. Each session loads the most recent
1000 non-empty commands.
