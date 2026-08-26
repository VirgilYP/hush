# Hush GUI Product Design

## 1. Product Context

- Product: a macOS-first Tauri serial console built on a reusable `hush-core`.
- Target user: embedded engineers who watch noisy UART output and repeatedly send a small bank of known commands.
- Target surface: one desktop window, optimized for 1280x800 and larger, with useful behavior down to 960x640.
- Primary job-to-be-done: connect to one serial device, keep trustworthy raw receive logs, edit reusable command slots, and send any slot immediately.
- Success criteria: a new user can select a port, connect, edit a default slot, and send it without opening a secondary dialog.
- Content/data that must appear: connection status, serial port, baud rate, framing summary, receive terminal, manual send input, 14 default command slots, Text/HEX mode, transmitted/received byte counts, and log state.
- Interaction requirements: one active serial connection per window; multiple windows are allowed; command slots send immediately; receive display may pause without stopping UART reads or raw logging; the manual terminal may opt into hush prompt-aware behavior.
- Technical constraints: Tauri frontend with a Rust backend; existing CLI and GUI share `hush-core`; macOS is the first packaged and verified target.

## 2. Existing UI Read

- Current visual vocabulary: `hush` is terminal-only and has no GUI component system.
- Strongest existing cue to preserve: prompt-aware control of noisy UART output without losing the raw log.
- Patterns to preserve: explicit session ownership, raw-byte logging, clear connection/log paths, command history, and monitor-compatible session behavior.
- Patterns to evolve: terminal-only controls become visible connection, display, and send controls.
- Patterns to remove or avoid: hidden keyboard-only state, subprocess/PTY screen scraping, SSCOM's unrelated ISP/network panels, tiny ambiguous checkboxes, and modal editing of common commands.
- Accessibility/state conventions: every status must use text and icon/shape in addition to color; all actions require keyboard focus states and labels.

## 3. Taste Direction

- Product identity sentence: a compact macOS engineering workbench that makes UART state and repeated actions obvious.
- Recommended taste direction: light precision workspace with a separately themed terminal surface.
- Direction to avoid: generic dark developer dashboard, oversized cards, neon accents, gradients, and decorative telemetry.
- Why this makes the UI more useful: dense rows remain scannable, command editing stays comfortable, and connection state remains visible without competing with UART content.
- What should feel distinctive: the direct left-terminal/right-command-bank relationship and trustworthy hush session behavior.
- What should stay quiet: app branding, decoration, shadows, and secondary configuration.

## 4. Selected References

### SSCOM 5.13.1 multi-string panel

- Why it fits: it proves the core workflow of a receive terminal beside dense, editable, independently sendable command rows.
- Transferable traits: split workspace, high-density rows, per-row Text/HEX meaning, editable command and label, immediate send.
- Non-transferable details: Windows-era menus, 99 rows by default, ISP/network features, tiny controls, checksum/delay/order columns, and INI-specific behavior.
- Implementation substitution: 12 core rows plus two explicitly warned reproduction rows, dynamic add/remove/reorder, macOS toolbar, persistent profiles, and explicit state labels.
- Risk: literal copying would preserve clutter and poor hierarchy.
- Rejection rule: remove any SSCOM-derived control that is not needed to connect, observe, edit, or send in the agreed first release.

### Airtable-like structured data clarity

- Why it fits: command slots behave more like editable structured rows than dashboard cards.
- Transferable traits: white canvas, deep neutral text, restrained blue action color, clear row boundaries, and consistent editable-field alignment.
- Non-transferable details: large rounded marketing cards, brand blue, large typography, and colorful database decoration.
- Implementation substitution: compact 30-36px slot rows, 6px radii, semantic tokens, and monospace command content.
- Risk: too much SaaS softness would reduce tool density.

### HashiCorp-like infrastructure restraint

- Why it fits: it supplies precise focus states, subtle borders, small radii, semantic status colors, and minimal elevation.
- Transferable traits: system font for functional UI, 4-6px radii, whisper-level shadows, tokenized colors, and strong focus rings.
- Non-transferable details: marketing typography, product-brand color system, hero layouts, and dark promotional sections.
- Implementation substitution: native system typography, one restrained blue accent, and flat operational panels.
- Risk: enterprise styling can become sterile; slot labels and direct manipulation must keep it approachable.

## 5. Visual Theme And Atmosphere

- Design thesis: preserve SSCOM's speed and density while making every state legible and every frequent action directly editable.
- Emotional tone: calm, precise, trustworthy, and fast.
- Product personality: an instrument, not a dashboard and not a terminal-themed toy.
- First viewport message: which device is connected, what it is receiving, and which commands are ready to send.
- Visual weight priorities: receive data first; connection status second; command bank third; secondary settings last.

## 6. Color Palette And Roles

- Page/background: `#F4F5F7`.
- Primary surface: `#FFFFFF`.
- Secondary surface: `#F8F9FB`.
- Terminal light surface: `#FBFBFC`; optional terminal dark surface: `#17191D`.
- Primary text: `#1E2329`.
- Secondary text: `#59636E`.
- Muted text: `#7A8490`.
- Accent/action: `#2563A6`, with white text when filled.
- Border/divider: `#D9DEE5` and `#E8EBEF` for row separators.
- Focus ring: `0 0 0 3px rgba(37, 99, 166, 0.24)`.
- Success/warning/error: `#238636`, `#A96600`, `#C43636`; always paired with a label or icon.
- Color constraints: no gradients, no decorative glow, no saturated status background covering a whole panel.

## 7. Typography Rules

- UI family: `-apple-system, BlinkMacSystemFont, "SF Pro Text", system-ui, sans-serif`.
- Terminal and command content: `ui-monospace, "SFMono-Regular", Menlo, monospace`.
- Toolbar and body: 13px-14px, weight 400-500, line height 1.35.
- Section labels: 12px, weight 600, restrained letter spacing, sentence case.
- Slot name: 13px system font; slot content: 13px monospace.
- Status/microcopy: 12px, never below 11px.
- Weight rule: reserve 600 for section labels, active tabs, and primary status; avoid 700+.

## 8. Component Styling

### Window and toolbar

- A single 44-48px toolbar contains profile/port, baud, Connect/Disconnect, connection status, display pause, clear, and log controls.
- Advanced framing settings live in a compact popover, not in the main workspace.
- Controls use 5-6px radii, 30-32px height, visible focus rings, and no heavy shadows.

### Receive terminal

- Occupies roughly 65% of the resizable split view by default.
- Flat surface with monospace text, selectable output, search, autoscroll, and a clear paused-display banner.
- Pausing display never pauses serial reads or raw logging.
- Scrolling away from the bottom triggers Smart Pause before new output can steal the scroll position; returning to the bottom resumes scroll-triggered pauses only.
- TX and RX presentation is distinguishable without rewriting the stored raw bytes.

### Command bank

- Occupies roughly 35% of the resizable split view, with a minimum useful width of 360px.
- Header contains an editable command-bank name, slot count, Load, Export, and Add.
- Default state has 14 rows: 12 general DM30 controls and two warned IN1 transition reproductions. Rows can be added, deleted, and reordered.
- Command banks use a versioned JSON format and can be loaded/exported independently of machine-specific serial port paths.
- The built-in DM30 profile exposes a compact quick-control strip: one-click UART report suppression, a safe `-200..0 dB` single-module gain slider with explicit Apply, and source-verified algorithm ON/OFF controls grouped by audio path.
- Each row contains: drag handle, Text/HEX segmented control, editable name, editable content, and a Send button.
- Content is always visible; editing never requires a modal.
- Send is disabled with a clear reason when disconnected or when HEX input is invalid.
- Successful send gets a brief non-blocking confirmation; failure remains visible until dismissed or superseded.

### Manual send bar

- Anchored below the receive terminal.
- Contains command input, history access, line-ending selector, prompt-aware toggle, and Send.
- Enter sends; Shift+Enter may insert a newline only if multiline input is enabled later.

### Empty/loading/error states

- No port: show `No serial devices found` plus Refresh; do not show a decorative illustration.
- Disconnected: command editing remains enabled, sending is disabled.
- Port lost: raw log remains visible, connection status becomes `Disconnected`, and reconnect is explicit in the first release.
- Errors: actionable inline messages name the port and failing operation.

## 9. Layout Principles

- Spacing scale: 4, 6, 8, 12, 16, 24px.
- Grid: toolbar; resizable two-pane workspace; manual send/status footer.
- Density model: compact operational density, not touch-first card density.
- Whitespace philosophy: use alignment and dividers rather than empty space to establish hierarchy.
- Desktop breakpoint: below 1040px, command bank may collapse into a right drawer; it must not stack below the terminal in a desktop window.
- Minimum target window: 960x640; recommended: 1280x800.

## 10. Depth, Motion, And Interaction

- Elevation: flat panels, 1px dividers, and only menu/popover shadows.
- Motion: 120-180ms opacity/background transitions; no sliding cards or animated counters.
- Splitter and row reordering provide immediate direct-manipulation feedback.
- Click targets are at least 28px high for dense desktop controls; primary Connect and Send targets are at least 32px.
- Destructive slot reset/delete actions require undo or confirmation appropriate to scope.

## 11. Do's And Don'ts

### Do

- Keep connection state visible at all times.
- Keep all 14 default slots editable without opening another screen.
- Validate HEX before writing any bytes.
- Preserve raw receive bytes independently from display formatting.
- Make Text/HEX and line-ending behavior explicit.
- Support keyboard navigation across command rows and `Cmd+Enter` to send the focused slot.

### Don't

- Do not copy SSCOM's menu clutter or unrelated protocol/ISP controls.
- Do not turn every region into a rounded card.
- Do not default the whole app to a dark dashboard.
- Do not hide serial ownership or connection errors behind generic toasts.
- Do not let display pause imply that logging or UART reads stopped.
- Do not execute a command simply because its text was edited.

## 12. Implementation Mapping

- Restructure the Rust crate into a reusable library plus binaries rather than launching the CLI as a subprocess.
- Add a Tauri app whose Rust commands own one `hush-core` session and emit typed receive/status events.
- Frontend components: `ConnectionToolbar`, `ReceiveTerminal`, `CommandBank`, `CommandSlotRow`, `ManualSendBar`, `SessionStatusBar`, and `AdvancedSerialPopover`.
- Persist device profiles, UI split position, line ending, terminal preferences, and command slots in the app configuration directory.
- No custom imagery is needed. Use a small consistent icon set and system symbols where licensing/packaging allows.
- The first release ships English UI strings with architecture ready for Chinese localization; final language scope remains an implementation decision.

## 13. Evaluation Plan

- Rust core: unit tests for Text/HEX encoding, line endings, port ownership, display pause versus raw capture, and slot persistence.
- Tauri/frontend: typecheck, component tests for slot editing/sending/disabled/error states, and production build.
- Visual verification: screenshots at 960x640, 1280x800, and a wide desktop size; inspect both light terminal and optional dark terminal.
- Interaction verification: connect/disconnect, unplug while connected, edit/add/delete/reorder slots, invalid HEX, manual send, prompt-aware manual mode, pause display, clear display, and log continuity.
- Hardware boundary: a loopback or pseudo-terminal verifies bytes mechanically; a real UART run separately verifies the actual USB-UART device and firmware response.
- Product-fit test: the receive terminal and 12 general slots fit in the first viewport at 1280x800; the two warned reproduction slots remain directly accessible by scrolling, without hidden editing dialogs.
