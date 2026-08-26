import { Channel, invoke } from "@tauri-apps/api/core";
import "./style.css";

type PayloadMode = "text" | "hex";
type LineEnding = "cr" | "lf" | "crlf" | "none";
type Transport = "uart" | "hid";
type PauseReason = "manual" | "scroll" | null;

interface PortInfo {
  portName: string;
  kind: string;
  displayName: string;
  vid: number | null;
  pid: number | null;
  serialNumber: string | null;
  manufacturer: string | null;
  product: string | null;
}

interface HidDeviceInfo {
  path: string;
  displayName: string;
  vendorId: number;
  productId: number;
  serialNumber: string | null;
  manufacturer: string | null;
  product: string | null;
  usagePage: number;
  usage: number;
  interfaceNumber: number;
}

interface CommandSlot {
  id: string;
  name: string;
  mode: PayloadMode;
  value: string;
}

interface AppConfig {
  version: number;
  commandBankName: string;
  transport: Transport;
  serial: {
    portName: string;
    baudRate: number;
    lineEnding: LineEnding;
  };
  hid: {
    path: string;
  };
  ui: {
    terminalDark: boolean;
    splitPercent: number;
    dm30GainMid: number;
    dm30GainDb: number;
    dm30AlgorithmMid: number;
  };
  slots: CommandSlot[];
}

interface ConnectionInfo {
  transport: Transport;
  displayName: string;
  detail: string;
  logPath: string;
}

interface CommandBank {
  format: "hush-command-bank";
  version: 1;
  name: string;
  slots: CommandSlot[];
}

interface BatchSendResult {
  commandCount: number;
  byteCount: number;
}

type SerialUiEvent =
  | { kind: "rx"; bytes: number[] }
  | { kind: "error"; message: string }
  | { kind: "disconnected" };

const MAX_TERMINAL_CHARS = 400_000;
const DM30_GAIN_SLOT_NAMES: Record<number, string> = {
  8: "MIC0 gain / level · MID 8",
  18: "MIC1 gain / level · MID 18",
  47: "Accompaniment gain / level · MID 47",
  53: "Reverb wet gain / level · MID 53",
  101: "Digital master gain / level · MID 101",
  121: "Analog monitor gain / level · MID 121",
  102: "UAC record gain · MID 102",
  111: "OTG record gain · MID 111",
  122: "DAC4344 gain · MID 122",
  131: "Internal DAC gain · MID 131",
};
const DM30_ALGORITHM_GROUPS: Array<{
  label: string;
  modules: Array<[number, string]>;
}> = [
  {
    label: "MIC0 chain",
    modules: [
      [9, "Mic amp"], [10, "AFS"], [1, "Denoise"], [2, "Compressor"],
      [3, "EQ"], [4, "Exciter"], [5, "Sibilance EQ"], [6, "Autotune"],
      [7, "Limiter"], [61, "Ducker controller"], [8, "Gain / level"],
    ],
  },
  {
    label: "MIC1 chain",
    modules: [
      [19, "Mic amp"], [20, "AFS"], [11, "Denoise"], [12, "Compressor"],
      [13, "EQ"], [14, "Exciter"], [15, "Sibilance EQ"], [16, "Autotune"],
      [17, "Limiter"], [46, "Ducker actuator"], [18, "Gain / level"],
    ],
  },
  {
    label: "Accompaniment",
    modules: [
      [31, "SFX ducker actuator"], [41, "Input mixer"], [42, "Denoise"],
      [43, "Compressor"], [48, "Vocal removal"], [44, "EQ"], [45, "Pitch"],
      [47, "Gain / level"],
    ],
  },
  {
    label: "Reverb return",
    modules: [[51, "Reverb"], [52, "EQ"], [53, "Gain / level"]],
  },
  {
    label: "Mixers and outputs",
    modules: [
      [71, "MIC0 pan"], [81, "MIC1 pan"], [91, "Main mixer"],
      [101, "Digital master gain"], [111, "OTG record gain"],
      [123, "Monitor mixer"], [121, "Analog monitor gain"], [132, "Mono pan"],
      [131, "Internal DAC gain"], [122, "DAC4344 gain"], [102, "UAC record gain"],
      [161, "BT / AUX mixer"], [162, "BT / AUX ducker"],
      [171, "OTG / UAC mixer"], [172, "OTG / UAC ducker"],
    ],
  },
];

function defaultConfig(): AppConfig {
  return {
    version: 2,
    commandBankName: "DM30",
    transport: "uart",
    serial: {
      portName: "",
      baudRate: 115_200,
      lineEnding: "cr",
    },
    hid: {
      path: "",
    },
    ui: {
      terminalDark: true,
      splitPercent: 64,
      dm30GainMid: 8,
      dm30GainDb: 0,
      dm30AlgorithmMid: 1,
    },
    slots: [
      ["Power on", "set mid=-2,power=on"],
      ["Report all config", "set mid=-1"],
      [DM30_GAIN_SLOT_NAMES[8], "set mid=8,rpsw=off,g=0"],
      [DM30_GAIN_SLOT_NAMES[18], "set mid=18,rpsw=off,g=0"],
      [DM30_GAIN_SLOT_NAMES[47], "set mid=47,rpsw=off,g=0"],
      [DM30_GAIN_SLOT_NAMES[53], "set mid=53,rpsw=off,g=0"],
      [DM30_GAIN_SLOT_NAMES[101], "set mid=101,rpsw=off,g=0"],
      [DM30_GAIN_SLOT_NAMES[121], "set mid=121,rpsw=off,g=0"],
      [DM30_GAIN_SLOT_NAMES[102], "set mid=102,g=0"],
      [DM30_GAIN_SLOT_NAMES[111], "set mid=111,g=0"],
      [DM30_GAIN_SLOT_NAMES[122], "set mid=122,g=0"],
      [DM30_GAIN_SLOT_NAMES[131], "set mid=131,g=0"],
      ["⚠ IN1 32.0 dB setup · MID 9", "set mid=9,sw=on,ch=1,g=0.08,bst=1"],
      ["⚠ IN1 31.9 dB transition · MID 9", "set mid=9,sw=on,ch=1,g=15.87,bst=2"],
    ].map(([name, value], index) => ({
      id: `dm30-slot-${String(index + 1).padStart(2, "0")}`,
      name,
      mode: "text" as const,
      value,
    })),
  };
}

function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

class HushApp {
  private readonly root: HTMLElement;
  private config: AppConfig = defaultConfig();
  private ports: PortInfo[] = [];
  private hidDevices: HidDeviceInfo[] = [];
  private connected = false;
  private connectionInfo: ConnectionInfo | null = null;
  private transportChannel: Channel<SerialUiEvent> | null = null;
  private receivePaused = false;
  private pauseReason: PauseReason = null;
  private programmaticTerminalScroll = false;
  private pausedText = "";
  private terminalText = "";
  private terminalPendingText = "";
  private terminalNeedsFullRender = false;
  private terminalTextNode: Text | null = null;
  private rxBytes = 0;
  private txBytes = 0;
  private saveTimer: number | undefined;
  private terminalRenderTimer: number | undefined;
  private draggedSlotId: string | null = null;
  private readonly decoder = new TextDecoder("utf-8", { fatal: false });

  constructor(root: HTMLElement) {
    this.root = root;
    this.renderShell();
    this.populateDm30AlgorithmOptions();
    this.bindShellEvents();
    this.renderSlots();
    this.applyConfigToControls();
    void this.initialize();
  }

  private element<T extends HTMLElement>(selector: string): T {
    const element = this.root.querySelector<T>(selector);
    if (!element) {
      throw new Error(`Missing UI element: ${selector}`);
    }
    return element;
  }

  private renderShell(): void {
    this.root.innerHTML = `
      <main class="app-shell">
        <header class="connection-toolbar" aria-label="Device connection">
          <div class="brand-lockup" aria-label="Hush">
            <span class="brand-mark" aria-hidden="true"><i></i><i></i><i></i></span>
            <strong>Hush</strong>
          </div>

          <div class="transport-switch" role="group" aria-label="Command transport">
            <button id="transport-uart" class="transport-option active" type="button" data-transport="uart">UART</button>
            <button id="transport-hid" class="transport-option" type="button" data-transport="hid" title="DM30 USB HID · 64-byte reports">USB HID</button>
          </div>

          <label class="compact-field port-field">
            <span id="device-field-label">Port</span>
            <select id="port-select" aria-label="Device"></select>
          </label>
          <button id="refresh-ports" class="icon-button" type="button" title="Refresh devices" aria-label="Refresh devices">↻</button>

          <label class="compact-field baud-field">
            <span>Baud</span>
            <input id="baud-rate" type="number" min="1" step="1" inputmode="numeric" aria-label="Baud rate" />
          </label>

          <span class="framing-badge" title="8 data bits, no parity, 1 stop bit">8-N-1</span>
          <button id="connect-button" class="primary-button" type="button">Connect</button>

          <div class="toolbar-spacer"></div>
          <div id="connection-status" class="connection-status disconnected" role="status">
            <span class="status-dot" aria-hidden="true"></span>
            <span class="status-label">Disconnected</span>
          </div>
        </header>

        <section class="workspace" aria-label="Command workspace">
          <section class="terminal-pane" aria-label="Receive terminal">
            <header class="pane-header">
              <div>
                <h1>Receive</h1>
                <span id="port-detail" class="pane-detail">No device session</span>
              </div>
              <div class="header-actions">
                <button id="pause-receive" class="quiet-button" type="button">Pause display</button>
                <button id="clear-terminal" class="quiet-button" type="button">Clear</button>
                <button id="terminal-theme" class="icon-button" type="button" title="Toggle terminal theme" aria-label="Toggle terminal theme">◐</button>
              </div>
            </header>

            <div id="pause-banner" class="pause-banner" hidden>
              <span id="pause-reason">Display paused</span> · device reads and raw logging continue
              <span id="paused-size"></span>
            </div>

            <pre id="terminal-output" class="terminal-output dark" tabindex="0" aria-label="Device receive output"><span class="terminal-placeholder">Connect a device to begin receiving data.</span></pre>

            <footer class="manual-send-bar">
              <label class="manual-input-wrap">
                <span class="sr-only">Manual command</span>
                <input id="manual-input" type="text" autocomplete="off" spellcheck="false" placeholder="Type a command…" />
              </label>
              <label class="line-ending-field">
                <span class="sr-only">Line ending</span>
                <select id="line-ending" title="Text line ending" aria-label="Text line ending">
                  <option value="cr">CR</option>
                  <option value="lf">LF</option>
                  <option value="crlf">CRLF</option>
                  <option value="none">None</option>
                </select>
              </label>
              <button id="manual-send" class="send-button" type="button" disabled>Send</button>
            </footer>
          </section>

          <aside class="command-pane" aria-label="Reusable commands">
            <header class="pane-header command-header">
              <div class="command-title-block">
                <h1>Commands</h1>
                <div class="bank-meta">
                  <input id="bank-name" class="bank-name" value="DM30" aria-label="Command bank name" />
                  <span aria-hidden="true">·</span>
                  <span id="slot-count" class="pane-detail">12 slots</span>
                </div>
              </div>
              <div class="command-bank-actions">
                <button id="load-bank" class="quiet-button" type="button">Load</button>
                <button id="export-bank" class="quiet-button" type="button">Export</button>
                <button id="add-slot" class="quiet-button add-slot" type="button"><span aria-hidden="true">＋</span> Add</button>
              </div>
            </header>
            <div id="dm30-quick-controls" class="quick-control-bar">
              <button id="silence-uart-reports" class="quick-action-button" type="button" disabled>
                Silence reports
              </button>
              <span class="quick-divider" aria-hidden="true"></span>
              <label class="gain-target">
                <span>Gain</span>
                <select id="gain-mid" aria-label="DM30 gain module">
                  <option value="8">MIC0 · MID 8</option>
                  <option value="18">MIC1 · MID 18</option>
                  <option value="47">Accompaniment · MID 47</option>
                  <option value="53">Reverb wet · MID 53</option>
                  <option value="101">Digital master · MID 101</option>
                  <option value="121">Analog monitor · MID 121</option>
                  <option value="102">UAC record · MID 102</option>
                  <option value="111">OTG record · MID 111</option>
                  <option value="122">DAC4344 · MID 122</option>
                  <option value="131">Internal DAC · MID 131</option>
                </select>
              </label>
              <input id="gain-slider" class="gain-slider" type="range" min="-200" max="0" step="1" value="0" aria-label="DM30 gain in dB" />
              <output id="gain-value" class="gain-value" for="gain-slider">0 dB</output>
              <button id="apply-gain" class="quick-apply-button" type="button" disabled>Apply</button>
              <div class="algorithm-control-row">
                <label class="algorithm-target">
                  <span>Algorithm</span>
                  <select id="algorithm-mid" aria-label="DM30 algorithm module"></select>
                </label>
                <button id="algorithm-off" class="algorithm-toggle off" type="button" disabled>OFF</button>
                <button id="algorithm-on" class="algorithm-toggle on" type="button" disabled>ON</button>
              </div>
            </div>
            <div class="slot-column-labels" aria-hidden="true">
              <span></span><span>Mode</span><span>Name & command</span><span></span><span></span>
            </div>
            <div id="command-list" class="command-list"></div>
          </aside>
        </section>

        <footer class="status-bar">
          <div class="traffic-stats">
            <span>RX <strong id="rx-count">0 B</strong></span>
            <span>TX <strong id="tx-count">0 B</strong></span>
          </div>
          <div id="activity-message" class="activity-message" role="status">Ready</div>
          <button id="log-path" class="log-path" type="button" title="Raw receive log path">Raw log starts when connected</button>
        </footer>
      </main>
    `;
  }

  private bindShellEvents(): void {
    for (const button of this.root.querySelectorAll<HTMLButtonElement>(".transport-option")) {
      button.addEventListener("click", () => {
        const transport = button.dataset.transport as Transport;
        if (this.connected || transport === this.config.transport) return;
        this.config.transport = transport;
        this.applyTransportControls();
        this.renderPortOptions();
        this.updateConnectionControls();
        this.scheduleSave();
        void this.refreshPorts();
        if (transport === "hid") {
          this.showActivity("USB HID mode · DM30 power on remains UART/shell only.");
        }
      });
    }
    this.element<HTMLButtonElement>("#refresh-ports").addEventListener("click", () => {
      void this.refreshPorts();
    });
    this.element<HTMLButtonElement>("#connect-button").addEventListener("click", () => {
      void (this.connected ? this.disconnect() : this.connect());
    });
    this.element<HTMLInputElement>("#baud-rate").addEventListener("change", (event) => {
      const value = Number((event.currentTarget as HTMLInputElement).value);
      if (Number.isFinite(value) && value > 0) {
        this.config.serial.baudRate = Math.floor(value);
        this.scheduleSave();
      }
    });
    this.element<HTMLSelectElement>("#port-select").addEventListener("change", (event) => {
      const value = (event.currentTarget as HTMLSelectElement).value;
      if (this.config.transport === "uart") {
        this.config.serial.portName = value;
      } else {
        this.config.hid.path = value;
      }
      this.scheduleSave();
      this.updateConnectionControls();
    });
    this.element<HTMLSelectElement>("#line-ending").addEventListener("change", (event) => {
      this.config.serial.lineEnding = (event.currentTarget as HTMLSelectElement).value as LineEnding;
      this.scheduleSave();
    });
    this.element<HTMLButtonElement>("#pause-receive").addEventListener("click", () => {
      this.toggleReceivePause();
    });
    const terminalOutput = this.element<HTMLElement>("#terminal-output");
    terminalOutput.addEventListener(
      "wheel",
      (event) => {
        if (event.deltaY < 0 && !this.receivePaused) {
          this.setReceivePaused(true, "scroll");
        }
      },
      { passive: true },
    );
    terminalOutput.addEventListener("scroll", () => {
      if (this.programmaticTerminalScroll) return;
      const distanceFromBottom =
        terminalOutput.scrollHeight - terminalOutput.scrollTop - terminalOutput.clientHeight;
      if (distanceFromBottom > 24 && !this.receivePaused) {
        this.setReceivePaused(true, "scroll");
      } else if (distanceFromBottom <= 24 && this.pauseReason === "scroll") {
        this.setReceivePaused(false, null);
      }
    });
    this.element<HTMLButtonElement>("#clear-terminal").addEventListener("click", () => {
      this.terminalText = "";
      this.pausedText = "";
      this.terminalPendingText = "";
      this.terminalNeedsFullRender = true;
      this.terminalTextNode = null;
      if (this.terminalRenderTimer !== undefined) {
        window.cancelAnimationFrame(this.terminalRenderTimer);
        this.terminalRenderTimer = undefined;
      }
      this.updateTerminal();
      this.updatePauseBanner();
      this.showActivity("Display cleared. Raw log was not changed.");
    });
    this.element<HTMLButtonElement>("#terminal-theme").addEventListener("click", () => {
      this.config.ui.terminalDark = !this.config.ui.terminalDark;
      this.applyTerminalTheme();
      this.scheduleSave();
    });
    this.element<HTMLButtonElement>("#manual-send").addEventListener("click", () => {
      void this.sendManual();
    });
    this.element<HTMLInputElement>("#manual-input").addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        void this.sendManual();
      }
    });
    this.element<HTMLInputElement>("#manual-input").addEventListener("input", () => {
      this.updateConnectionControls();
    });
    this.element<HTMLButtonElement>("#add-slot").addEventListener("click", () => {
      this.addSlot();
    });
    this.element<HTMLInputElement>("#bank-name").addEventListener("input", (event) => {
      this.config.commandBankName = (event.currentTarget as HTMLInputElement).value;
      this.updateDm30QuickControls();
      this.scheduleSave();
    });
    this.element<HTMLButtonElement>("#load-bank").addEventListener("click", () => {
      void this.loadCommandBank();
    });
    this.element<HTMLButtonElement>("#export-bank").addEventListener("click", () => {
      void this.exportCommandBank();
    });
    this.element<HTMLButtonElement>("#silence-uart-reports").addEventListener("click", () => {
      void this.silenceDm30UartReports();
    });
    this.element<HTMLSelectElement>("#gain-mid").addEventListener("change", (event) => {
      this.config.ui.dm30GainMid = Number((event.currentTarget as HTMLSelectElement).value);
      this.syncGainSliderFromSlot();
      this.scheduleSave();
    });
    this.element<HTMLInputElement>("#gain-slider").addEventListener("input", (event) => {
      this.config.ui.dm30GainDb = Number((event.currentTarget as HTMLInputElement).value);
      this.updateGainControl();
      this.updateSelectedGainSlot();
      this.scheduleSave();
    });
    this.element<HTMLButtonElement>("#apply-gain").addEventListener("click", () => {
      void this.applyDm30Gain();
    });
    this.element<HTMLSelectElement>("#algorithm-mid").addEventListener("change", (event) => {
      this.config.ui.dm30AlgorithmMid = Number((event.currentTarget as HTMLSelectElement).value);
      this.scheduleSave();
    });
    this.element<HTMLButtonElement>("#algorithm-off").addEventListener("click", () => {
      void this.setDm30Algorithm(false);
    });
    this.element<HTMLButtonElement>("#algorithm-on").addEventListener("click", () => {
      void this.setDm30Algorithm(true);
    });
    this.element<HTMLButtonElement>("#log-path").addEventListener("click", () => {
      if (this.connectionInfo?.logPath) {
        void navigator.clipboard.writeText(this.connectionInfo.logPath);
        this.showActivity("Raw log path copied.");
      }
    });
  }

  private async initialize(): Promise<void> {
    if (!isTauriRuntime()) {
      this.showActivity("Browser preview · device backend unavailable");
      this.renderPortOptions();
      return;
    }

    try {
      this.config = await invoke<AppConfig>("load_app_config");
      this.config.transport ??= "uart";
      this.config.hid ??= { path: "" };
      if (!Array.isArray(this.config.slots) || this.config.slots.length === 0) {
        this.config.slots = defaultConfig().slots;
      }
      const migratedNames = this.migrateDm30FriendlyNames();
      const addedReproCommands = this.ensureDm30ReproCommands();
      this.renderSlots();
      this.applyConfigToControls();
      if (migratedNames || addedReproCommands) this.scheduleSave();
    } catch (error) {
      this.showActivity(String(error), true);
    }
    await this.refreshPorts();
  }

  private applyConfigToControls(): void {
    this.element<HTMLInputElement>("#baud-rate").value = String(this.config.serial.baudRate);
    this.element<HTMLSelectElement>("#line-ending").value = this.config.serial.lineEnding;
    this.element<HTMLInputElement>("#bank-name").value = this.config.commandBankName;
    this.element<HTMLSelectElement>("#gain-mid").value = String(this.config.ui.dm30GainMid);
    this.element<HTMLInputElement>("#gain-slider").value = String(this.config.ui.dm30GainDb);
    this.element<HTMLSelectElement>("#algorithm-mid").value = String(
      this.config.ui.dm30AlgorithmMid,
    );
    this.updateGainControl();
    this.updateDm30QuickControls();
    this.applyTerminalTheme();
    this.applyTransportControls();
    this.renderPortOptions();
    this.updateConnectionControls();
  }

  private applyTransportControls(): void {
    for (const button of this.root.querySelectorAll<HTMLButtonElement>(".transport-option")) {
      const active = button.dataset.transport === this.config.transport;
      button.classList.toggle("active", active);
      button.setAttribute("aria-pressed", String(active));
      button.disabled = this.connected;
    }
    const isUart = this.config.transport === "uart";
    this.element<HTMLElement>("#device-field-label").textContent = isUart ? "Port" : "HID";
    this.element<HTMLElement>(".baud-field").hidden = !isUart;
    this.element<HTMLElement>(".framing-badge").hidden = !isUart;
  }

  private migrateDm30FriendlyNames(): boolean {
    if (this.config.commandBankName.trim().toUpperCase() !== "DM30") return false;
    let changed = false;
    for (const slot of this.config.slots) {
      if (!slot.id.startsWith("dm30-slot-") || !/^MID \d+ · gain/i.test(slot.name)) continue;
      const match = slot.value.match(/^set\s+mid=(\d+)(?:,|$)/i);
      const friendlyName = match ? DM30_GAIN_SLOT_NAMES[Number(match[1])] : undefined;
      if (friendlyName && friendlyName !== slot.name) {
        slot.name = friendlyName;
        changed = true;
      }
    }
    return changed;
  }

  private ensureDm30ReproCommands(): boolean {
    if (this.config.commandBankName.trim().toUpperCase() !== "DM30") return false;
    const required: Array<[string, string, string]> = [
      [
        "dm30-repro-in1-32",
        "⚠ IN1 32.0 dB setup · MID 9",
        "set mid=9,sw=on,ch=1,g=0.08,bst=1",
      ],
      [
        "dm30-repro-in1-31-9",
        "⚠ IN1 31.9 dB transition · MID 9",
        "set mid=9,sw=on,ch=1,g=15.87,bst=2",
      ],
    ];
    let changed = false;
    for (const [id, name, value] of required) {
      if (this.config.slots.some((slot) => slot.value.trim().toLowerCase() === value.toLowerCase())) {
        continue;
      }
      this.config.slots.push({ id, name, mode: "text", value });
      changed = true;
    }
    return changed;
  }

  private applyTerminalTheme(): void {
    this.element<HTMLElement>("#terminal-output").classList.toggle(
      "dark",
      this.config.ui.terminalDark,
    );
  }

  private async refreshPorts(): Promise<void> {
    if (!isTauriRuntime()) {
      this.renderPortOptions();
      return;
    }
    const refresh = this.element<HTMLButtonElement>("#refresh-ports");
    refresh.classList.add("spinning");
    try {
      if (this.config.transport === "uart") {
        this.ports = await invoke<PortInfo[]>("serial_ports");
        const saved = this.config.serial.portName;
        if (saved.startsWith("/dev/tty.")) {
          const callout = saved.replace("/dev/tty.", "/dev/cu.");
          if (this.ports.some((port) => port.portName === callout)) {
            this.config.serial.portName = callout;
            this.scheduleSave();
          }
        }
        if (saved && !this.ports.some((port) => port.portName === saved)) {
          if (!this.config.serial.portName.startsWith("/dev/cu.")) {
            this.config.serial.portName = "";
          }
        }
        if (!this.config.serial.portName && this.ports.length === 1) {
          this.config.serial.portName = this.ports[0].portName;
        }
        this.showActivity(
          this.ports.length === 0
            ? "No serial devices found."
            : `${this.ports.length} serial ${this.ports.length === 1 ? "device" : "devices"} found.`,
        );
      } else {
        this.hidDevices = await invoke<HidDeviceInfo[]>("hid_devices");
        if (
          this.config.hid.path &&
          !this.hidDevices.some((device) => device.path === this.config.hid.path)
        ) {
          this.config.hid.path = "";
        }
        if (!this.config.hid.path && this.hidDevices.length === 1) {
          this.config.hid.path = this.hidDevices[0].path;
          this.scheduleSave();
        }
        this.showActivity(
          this.hidDevices.length === 0
            ? "No DM30 USB HID found. The unit must already be powered and enumerated."
            : `${this.hidDevices.length} DM30 USB HID ${this.hidDevices.length === 1 ? "interface" : "interfaces"} found.`,
        );
      }
      this.renderPortOptions();
      this.updateConnectionControls();
    } catch (error) {
      this.showActivity(`Device scan failed: ${String(error)}`, true);
    } finally {
      refresh.classList.remove("spinning");
    }
  }

  private renderPortOptions(): void {
    const select = this.element<HTMLSelectElement>("#port-select");
    select.replaceChildren();
    const placeholder = document.createElement("option");
    placeholder.value = "";
    const isUart = this.config.transport === "uart";
    const devices = isUart ? this.ports : this.hidDevices;
    placeholder.textContent = isTauriRuntime()
      ? devices.length === 0
        ? isUart ? "No serial devices" : "No DM30 USB HID"
        : isUart ? "Select a serial port" : "Select DM30 USB HID"
      : "Tauri runtime required";
    select.append(placeholder);

    for (const device of devices) {
      const option = document.createElement("option");
      option.value = isUart
        ? (device as PortInfo).portName
        : (device as HidDeviceInfo).path;
      option.textContent = device.displayName;
      select.append(option);
    }
    select.value = isUart ? this.config.serial.portName : this.config.hid.path;
    select.disabled = this.connected;
  }

  private async connect(): Promise<void> {
    if (!isTauriRuntime()) {
      this.showActivity("Run the Tauri app to connect a device.", true);
      return;
    }
    const selectedPath = this.config.transport === "uart"
      ? this.config.serial.portName
      : this.config.hid.path;
    if (!selectedPath) {
      this.showActivity(
        this.config.transport === "uart"
          ? "Select a serial port before connecting."
          : "Select the DM30 USB HID interface before connecting.",
        true,
      );
      return;
    }

    const channel = new Channel<SerialUiEvent>();
    channel.onmessage = (event) => this.handleSerialEvent(event);
    this.transportChannel = channel;
    this.setConnectingState(true);
    try {
      this.connectionInfo = this.config.transport === "uart"
        ? await invoke<ConnectionInfo>("connect_serial", {
            settings: {
              portName: this.config.serial.portName,
              baudRate: this.config.serial.baudRate,
              dataBits: 8,
              stopBits: 1,
              parity: "none",
              flowControl: "none",
            },
            onEvent: channel,
          })
        : await invoke<ConnectionInfo>("connect_hid", {
            path: this.config.hid.path,
            onEvent: channel,
          });
      this.connected = true;
      this.updateConnectionControls();
      this.updateConnectionStatus();
      this.element<HTMLElement>("#port-detail").textContent = this.connectionInfo.detail;
      this.element<HTMLButtonElement>("#log-path").textContent = this.connectionInfo.logPath;
      this.appendTerminal(
        `[Hush] Connected via ${this.connectionInfo.transport.toUpperCase()} · ${this.connectionInfo.displayName}\n`,
      );
      this.showActivity(
        this.connectionInfo.transport === "uart"
          ? "Serial port connected."
          : "DM30 USB HID connected · commands use report ID 1.",
      );
      this.scheduleSave();
    } catch (error) {
      this.transportChannel = null;
      this.showActivity(`Connect failed: ${String(error)}`, true);
    } finally {
      this.setConnectingState(false);
    }
  }

  private async disconnect(): Promise<void> {
    if (!isTauriRuntime()) return;
    this.setConnectingState(true);
    try {
      await invoke(this.config.transport === "uart" ? "disconnect_serial" : "disconnect_hid");
    } catch (error) {
      this.showActivity(`Disconnect failed: ${String(error)}`, true);
    } finally {
      this.markDisconnected(
        this.config.transport === "uart" ? "Serial port disconnected." : "USB HID disconnected.",
      );
      this.setConnectingState(false);
    }
  }

  private handleSerialEvent(event: SerialUiEvent): void {
    switch (event.kind) {
      case "rx": {
        const bytes = new Uint8Array(event.bytes);
        this.rxBytes += bytes.byteLength;
        const text = this.decoder.decode(bytes, { stream: true });
        this.appendTerminal(text);
        this.updateTrafficStats();
        break;
      }
      case "error":
        this.showActivity(`${this.transportLabel()} read error: ${event.message}`, true);
        break;
      case "disconnected":
        if (this.connected) {
          this.markDisconnected(`The ${this.transportLabel()} session ended.`);
        }
        break;
    }
  }

  private markDisconnected(message: string): void {
    this.connected = false;
    if (this.transportChannel) {
      this.transportChannel.onmessage = () => undefined;
    }
    this.transportChannel = null;
    this.connectionInfo = null;
    this.updateConnectionControls();
    this.updateConnectionStatus();
    this.element<HTMLElement>("#port-detail").textContent = "No device session";
    this.appendTerminal(`\n[Hush] ${message}\n`);
    this.showActivity(message);
  }

  private transportLabel(): string {
    return this.config.transport === "uart" ? "UART" : "USB HID";
  }

  private setConnectingState(connecting: boolean): void {
    const button = this.element<HTMLButtonElement>("#connect-button");
    button.disabled = connecting;
    button.textContent = connecting ? "Working…" : this.connected ? "Disconnect" : "Connect";
  }

  private updateConnectionStatus(): void {
    const status = this.element<HTMLElement>("#connection-status");
    status.classList.toggle("connected", this.connected);
    status.classList.toggle("disconnected", !this.connected);
    status.querySelector<HTMLElement>(".status-label")!.textContent = this.connected
      ? "Connected"
      : "Disconnected";
  }

  private updateConnectionControls(): void {
    this.applyTransportControls();
    this.renderPortOptions();
    const connect = this.element<HTMLButtonElement>("#connect-button");
    connect.textContent = this.connected ? "Disconnect" : "Connect";
    connect.classList.toggle("disconnect-button", this.connected);
    const selectedDevice = this.config.transport === "uart"
      ? this.config.serial.portName
      : this.config.hid.path;
    connect.disabled = !this.connected && !selectedDevice;
    this.element<HTMLInputElement>("#baud-rate").disabled = this.connected;
    const manualInput = this.element<HTMLInputElement>("#manual-input");
    this.element<HTMLButtonElement>("#manual-send").disabled =
      !this.connected || manualInput.value.trim().length === 0;
    for (const button of this.root.querySelectorAll<HTMLButtonElement>(".slot-send")) {
      const row = button.closest<HTMLElement>(".command-row");
      const value = row?.querySelector<HTMLInputElement>(".slot-value")?.value ?? "";
      const hidPowerUnsupported =
        this.config.transport === "hid" && this.isDm30PowerCommand(value);
      button.disabled = !this.connected || value.trim().length === 0 || hidPowerUnsupported;
      button.title = hidPowerUnsupported
        ? "Power on is UART/shell only; USB HID exists after the unit is powered"
        : this.connected
          ? `Send via ${this.transportLabel()}`
          : `Connect ${this.transportLabel()} to send`;
      row?.classList.toggle("transport-unsupported", hidPowerUnsupported);
    }
    this.element<HTMLButtonElement>("#silence-uart-reports").disabled = !this.connected;
    this.element<HTMLButtonElement>("#apply-gain").disabled = !this.connected;
    this.element<HTMLButtonElement>("#algorithm-off").disabled = !this.connected;
    this.element<HTMLButtonElement>("#algorithm-on").disabled = !this.connected;
  }

  private isDm30PowerCommand(value: string): boolean {
    const normalized = value.toLowerCase();
    return normalized.includes("mid=-2") && normalized.includes("power=");
  }

  private renderSlots(): void {
    const list = this.element<HTMLElement>("#command-list");
    list.replaceChildren();

    for (const slot of this.config.slots) {
      const row = document.createElement("div");
      row.className = "command-row";
      row.dataset.slotId = slot.id;
      row.draggable = true;

      const handle = document.createElement("button");
      handle.type = "button";
      handle.className = "drag-handle";
      handle.textContent = "⠿";
      handle.title = "Drag to reorder";
      handle.setAttribute("aria-label", `Reorder ${slot.name}`);

      const mode = document.createElement("button");
      mode.type = "button";
      mode.className = `mode-toggle ${slot.mode}`;
      mode.textContent = slot.mode === "hex" ? "HEX" : "TXT";
      mode.title = slot.mode === "hex" ? "Send exact hexadecimal bytes" : "Send text with line ending";
      mode.addEventListener("click", () => {
        slot.mode = slot.mode === "text" ? "hex" : "text";
        mode.className = `mode-toggle ${slot.mode}`;
        mode.textContent = slot.mode === "hex" ? "HEX" : "TXT";
        mode.title = slot.mode === "hex" ? "Send exact hexadecimal bytes" : "Send text with line ending";
        value.placeholder = slot.mode === "hex" ? "13 00 FF 88" : "app info version";
        this.scheduleSave();
      });

      const fields = document.createElement("div");
      fields.className = "slot-fields";
      const name = document.createElement("input");
      name.className = "slot-name";
      name.value = slot.name;
      name.placeholder = "Command name";
      name.setAttribute("aria-label", "Command name");
      name.addEventListener("input", () => {
        slot.name = name.value;
        handle.setAttribute("aria-label", `Reorder ${slot.name || "command"}`);
        this.scheduleSave();
      });
      const value = document.createElement("input");
      value.className = "slot-value";
      value.value = slot.value;
      value.placeholder = slot.mode === "hex" ? "13 00 FF 88" : "app info version";
      value.spellcheck = false;
      value.autocomplete = "off";
      value.setAttribute("aria-label", `${slot.name} content`);
      value.addEventListener("input", () => {
        slot.value = value.value;
        row.classList.remove("invalid");
        this.updateConnectionControls();
        this.scheduleSave();
      });
      value.addEventListener("keydown", (event) => {
        if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
          event.preventDefault();
          void this.sendSlot(slot, row);
        }
      });
      fields.append(name, value);

      const send = document.createElement("button");
      send.type = "button";
      send.className = "slot-send";
      send.textContent = "Send";
      send.addEventListener("click", () => void this.sendSlot(slot, row));

      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "remove-slot";
      remove.textContent = "×";
      remove.title = "Delete slot";
      remove.setAttribute("aria-label", `Delete ${slot.name}`);
      remove.addEventListener("click", () => this.removeSlot(slot.id));

      row.addEventListener("dragstart", (event) => {
        this.draggedSlotId = slot.id;
        row.classList.add("dragging");
        event.dataTransfer?.setData("text/plain", slot.id);
        if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
      });
      row.addEventListener("dragend", () => {
        this.draggedSlotId = null;
        row.classList.remove("dragging");
      });
      row.addEventListener("dragover", (event) => {
        event.preventDefault();
        row.classList.add("drop-target");
      });
      row.addEventListener("dragleave", () => row.classList.remove("drop-target"));
      row.addEventListener("drop", (event) => {
        event.preventDefault();
        row.classList.remove("drop-target");
        const sourceId = this.draggedSlotId ?? event.dataTransfer?.getData("text/plain") ?? "";
        this.moveSlot(sourceId, slot.id);
      });

      row.append(handle, mode, fields, send, remove);
      list.append(row);
    }

    const count = this.config.slots.length;
    this.element<HTMLElement>("#slot-count").textContent = `${count} ${count === 1 ? "slot" : "slots"}`;
    this.updateConnectionControls();
  }

  private addSlot(): void {
    const index = this.config.slots.length + 1;
    this.config.slots.push({
      id: crypto.randomUUID(),
      name: `Command ${String(index).padStart(2, "0")}`,
      mode: "text",
      value: "",
    });
    this.renderSlots();
    this.scheduleSave();
    const rows = this.root.querySelectorAll<HTMLElement>(".command-row");
    rows[rows.length - 1]?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  private removeSlot(id: string): void {
    const slot = this.config.slots.find((candidate) => candidate.id === id);
    if (!slot) return;
    if (slot.value.trim() && !window.confirm(`Delete “${slot.name}”?`)) return;
    this.config.slots = this.config.slots.filter((candidate) => candidate.id !== id);
    this.renderSlots();
    this.scheduleSave();
  }

  private moveSlot(sourceId: string, targetId: string): void {
    if (!sourceId || sourceId === targetId) return;
    const sourceIndex = this.config.slots.findIndex((slot) => slot.id === sourceId);
    const targetIndex = this.config.slots.findIndex((slot) => slot.id === targetId);
    if (sourceIndex < 0 || targetIndex < 0) return;
    const [slot] = this.config.slots.splice(sourceIndex, 1);
    this.config.slots.splice(targetIndex, 0, slot);
    this.renderSlots();
    this.scheduleSave();
  }

  private async sendSlot(slot: CommandSlot, row: HTMLElement): Promise<void> {
    if (!slot.value.trim()) return;
    row.classList.remove("invalid");
    try {
      const count = await this.send(slot.mode, slot.value);
      this.appendTerminal(`\n[TX · ${slot.name || "Command"}] ${this.txPreview(slot.mode, slot.value)}\n`);
      row.classList.add("sent");
      window.setTimeout(() => row.classList.remove("sent"), 500);
      this.showActivity(`${slot.name || "Command"} sent · ${count} B`);
    } catch (error) {
      row.classList.add("invalid");
      this.showActivity(String(error), true);
    }
  }

  private async sendManual(): Promise<void> {
    const input = this.element<HTMLInputElement>("#manual-input");
    if (!input.value.trim()) return;
    try {
      const count = await this.send("text", input.value);
      this.appendTerminal(`\n[TX] ${input.value}\n`);
      input.value = "";
      this.updateConnectionControls();
      this.showActivity(`Manual command sent · ${count} B`);
    } catch (error) {
      this.showActivity(String(error), true);
    }
  }

  private async loadCommandBank(): Promise<void> {
    if (!isTauriRuntime()) {
      this.showActivity("Command bank dialogs require the Tauri app.", true);
      return;
    }
    try {
      const bank = await invoke<CommandBank | null>("import_command_bank");
      if (!bank) return;
      if (
        this.config.slots.some((slot) => slot.value.trim()) &&
        !window.confirm(
          `Replace the current “${this.config.commandBankName}” command bank with “${bank.name}”?`,
        )
      ) {
        return;
      }
      this.config.commandBankName = bank.name;
      this.config.slots = bank.slots;
      this.element<HTMLInputElement>("#bank-name").value = bank.name;
      this.renderSlots();
      this.updateDm30QuickControls();
      this.scheduleSave();
      this.showActivity(`Loaded “${bank.name}” · ${bank.slots.length} slots`);
    } catch (error) {
      this.showActivity(`Load failed: ${String(error)}`, true);
    }
  }

  private async exportCommandBank(): Promise<void> {
    if (!isTauriRuntime()) {
      this.showActivity("Command bank dialogs require the Tauri app.", true);
      return;
    }
    const name = this.config.commandBankName.trim();
    if (!name) {
      this.showActivity("Name the command bank before exporting it.", true);
      this.element<HTMLInputElement>("#bank-name").focus();
      return;
    }
    const bank: CommandBank = {
      format: "hush-command-bank",
      version: 1,
      name,
      slots: this.config.slots,
    };
    try {
      const path = await invoke<string | null>("export_command_bank", { bank });
      if (path) this.showActivity(`Exported “${name}” to ${path}`);
    } catch (error) {
      this.showActivity(`Export failed: ${String(error)}`, true);
    }
  }

  private updateDm30QuickControls(): void {
    const isDm30 = this.config.commandBankName.trim().toUpperCase() === "DM30";
    this.element<HTMLElement>("#dm30-quick-controls").hidden = !isDm30;
  }

  private populateDm30AlgorithmOptions(): void {
    const select = this.element<HTMLSelectElement>("#algorithm-mid");
    select.replaceChildren();
    for (const group of DM30_ALGORITHM_GROUPS) {
      const optionGroup = document.createElement("optgroup");
      optionGroup.label = group.label;
      for (const [mid, name] of group.modules) {
        const option = document.createElement("option");
        option.value = String(mid);
        option.textContent = `${name} · MID ${mid}`;
        optionGroup.append(option);
      }
      select.append(optionGroup);
    }
  }

  private dm30AlgorithmName(mid: number): string {
    for (const group of DM30_ALGORITHM_GROUPS) {
      const module = group.modules.find(([candidate]) => candidate === mid);
      if (module) return module[1];
    }
    return `MID ${mid}`;
  }

  private updateGainControl(): void {
    const gain = Math.max(-200, Math.min(0, this.config.ui.dm30GainDb));
    this.config.ui.dm30GainDb = gain;
    this.element<HTMLInputElement>("#gain-slider").value = String(gain);
    this.element<HTMLOutputElement>("#gain-value").value = `${gain} dB`;
  }

  private syncGainSliderFromSlot(): void {
    const mid = this.config.ui.dm30GainMid;
    const slot = this.config.slots.find((candidate) =>
      new RegExp(`^set\\s+mid=${mid}(?:,|$)`, "i").test(candidate.value.trim()),
    );
    const match = slot?.value.match(/(?:^|,)g=(-?\d+)(?:,|$)/i);
    if (match) this.config.ui.dm30GainDb = Number(match[1]);
    this.updateGainControl();
  }

  private updateSelectedGainSlot(): void {
    const mid = this.config.ui.dm30GainMid;
    const gain = this.config.ui.dm30GainDb;
    const slot = this.config.slots.find((candidate) =>
      new RegExp(`^set\\s+mid=${mid}(?:,|$)`, "i").test(candidate.value.trim()),
    );
    if (!slot || !/(?:^|,)g=-?\d+(?:,|$)/i.test(slot.value)) return;
    slot.value = slot.value.replace(/((?:^|,)g=)-?\d+(?=,|$)/i, `$1${gain}`);
    const row = this.root.querySelector<HTMLElement>(`[data-slot-id="${CSS.escape(slot.id)}"]`);
    const input = row?.querySelector<HTMLInputElement>(".slot-value");
    if (input) input.value = slot.value;
  }

  private async silenceDm30UartReports(): Promise<void> {
    const commands = [8, 18, 47, 53, 101, 121].map((mid) => `set mid=${mid},rpsw=off`);
    const button = this.element<HTMLButtonElement>("#silence-uart-reports");
    button.disabled = true;
    try {
      const result = await invoke<BatchSendResult>("send_batch", {
        request: {
          transport: this.config.transport,
          commands,
          lineEnding: this.config.serial.lineEnding,
          delayMs: 20,
        },
      });
      this.txBytes += result.byteCount;
      this.updateTrafficStats();
      this.appendTerminal(
        `\n[TX · Silence UART reports] ${result.commandCount} commands · ${result.byteCount} B\n`,
      );
      this.showActivity(`UART reports disabled · ${result.commandCount} commands sent`);
    } catch (error) {
      this.showActivity(`Silence reports failed: ${String(error)}`, true);
    } finally {
      button.disabled = !this.connected;
    }
  }

  private async applyDm30Gain(): Promise<void> {
    const mid = this.config.ui.dm30GainMid;
    const gain = this.config.ui.dm30GainDb;
    const button = this.element<HTMLButtonElement>("#apply-gain");
    button.disabled = true;
    try {
      const command = `set mid=${mid},g=${gain}`;
      const count = await this.send("text", command);
      this.appendTerminal(`\n[TX · Gain] ${command}\n`);
      this.showActivity(`MID ${mid} gain set to ${gain} dB · ${count} B`);
    } catch (error) {
      this.showActivity(`Gain send failed: ${String(error)}`, true);
    } finally {
      button.disabled = !this.connected;
    }
  }

  private async setDm30Algorithm(enabled: boolean): Promise<void> {
    const mid = this.config.ui.dm30AlgorithmMid;
    const state = enabled ? "on" : "off";
    const name = this.dm30AlgorithmName(mid);
    const offButton = this.element<HTMLButtonElement>("#algorithm-off");
    const onButton = this.element<HTMLButtonElement>("#algorithm-on");
    offButton.disabled = true;
    onButton.disabled = true;
    try {
      const command = `set mid=${mid},sw=${state}`;
      const count = await this.send("text", command);
      this.appendTerminal(`\n[TX · Algorithm] ${command}\n`);
      this.showActivity(`${name} · MID ${mid} switched ${state.toUpperCase()} · ${count} B`);
    } catch (error) {
      this.showActivity(`Algorithm switch failed: ${String(error)}`, true);
    } finally {
      offButton.disabled = !this.connected;
      onButton.disabled = !this.connected;
    }
  }

  private async send(mode: PayloadMode, value: string): Promise<number> {
    if (!isTauriRuntime()) throw new Error("Device backend is unavailable in browser preview.");
    const count = await invoke<number>("send_payload", {
      request: {
        transport: this.config.transport,
        mode,
        value,
        lineEnding: this.config.serial.lineEnding,
      },
    });
    this.txBytes += count;
    this.updateTrafficStats();
    return count;
  }

  private txPreview(mode: PayloadMode, value: string): string {
    return mode === "hex" ? value.trim().toUpperCase() : value;
  }

  private toggleReceivePause(): void {
    if (this.receivePaused) {
      this.setReceivePaused(false, null);
    } else {
      this.setReceivePaused(true, "manual");
    }
  }

  private setReceivePaused(paused: boolean, reason: PauseReason): void {
    if (paused === this.receivePaused && (!paused || reason === this.pauseReason)) return;

    if (paused) {
      if (this.terminalRenderTimer !== undefined) {
        window.cancelAnimationFrame(this.terminalRenderTimer);
        this.terminalRenderTimer = undefined;
        this.updateTerminal(false);
      }
      this.receivePaused = true;
      this.pauseReason = reason;
    } else {
      this.receivePaused = false;
      this.pauseReason = null;
      if (this.pausedText) {
        const pending = this.pausedText;
        this.pausedText = "";
        this.appendTerminal(pending);
      } else {
        this.scheduleTerminalRender();
      }
    }

    const button = this.element<HTMLButtonElement>("#pause-receive");
    button.textContent = this.receivePaused ? "Resume display" : "Pause display";
    button.classList.toggle("active", this.receivePaused);
    this.updatePauseBanner();
  }

  private updatePauseBanner(): void {
    const banner = this.element<HTMLElement>("#pause-banner");
    banner.hidden = !this.receivePaused;
    this.element<HTMLElement>("#pause-reason").textContent =
      this.pauseReason === "scroll" ? "Scrolled up · display paused" : "Display paused";
    this.element<HTMLElement>("#paused-size").textContent = this.receivePaused
      ? `· ${this.formatBytes(new Blob([this.pausedText]).size)} buffered for display`
      : "";
  }

  private appendTerminal(text: string): void {
    if (!text) return;
    if (this.receivePaused) {
      this.pausedText += text;
      if (this.pausedText.length > MAX_TERMINAL_CHARS) {
        this.pausedText = this.pausedText.slice(-MAX_TERMINAL_CHARS);
      }
      this.updatePauseBanner();
      return;
    }
    this.terminalText += text;
    this.terminalPendingText += text;
    if (this.terminalText.length > MAX_TERMINAL_CHARS) {
      this.terminalText = this.terminalText.slice(-MAX_TERMINAL_CHARS);
      this.terminalPendingText = "";
      this.terminalNeedsFullRender = true;
    }
    this.scheduleTerminalRender();
  }

  private scheduleTerminalRender(): void {
    if (this.terminalRenderTimer !== undefined) return;
    this.terminalRenderTimer = window.requestAnimationFrame(() => {
      this.terminalRenderTimer = undefined;
      this.updateTerminal(true);
    });
  }

  private updateTerminal(scrollToBottom = false): void {
    const output = this.element<HTMLElement>("#terminal-output");
    if (!this.terminalText) {
      output.replaceChildren();
      const placeholder = document.createElement("span");
      placeholder.className = "terminal-placeholder";
      placeholder.textContent = this.connected
        ? `Connected. Waiting for ${this.transportLabel()} data…`
        : "Connect a device to begin receiving data.";
      output.append(placeholder);
      this.terminalPendingText = "";
      this.terminalNeedsFullRender = false;
      this.terminalTextNode = null;
    } else if (this.terminalNeedsFullRender) {
      output.textContent = this.terminalText;
      this.terminalPendingText = "";
      this.terminalNeedsFullRender = false;
      this.terminalTextNode = output.firstChild as Text | null;
    } else if (this.terminalPendingText) {
      if (
        output.querySelector(".terminal-placeholder") ||
        !this.terminalTextNode ||
        this.terminalTextNode.parentNode !== output
      ) {
        output.textContent = this.terminalText;
        this.terminalTextNode = output.firstChild as Text | null;
      } else {
        this.terminalTextNode.appendData(this.terminalPendingText);
      }
      this.terminalPendingText = "";
    }
    if (scrollToBottom) {
      this.programmaticTerminalScroll = true;
      output.scrollTop = output.scrollHeight;
      window.requestAnimationFrame(() => {
        this.programmaticTerminalScroll = false;
      });
    }
  }

  private updateTrafficStats(): void {
    this.element<HTMLElement>("#rx-count").textContent = this.formatBytes(this.rxBytes);
    this.element<HTMLElement>("#tx-count").textContent = this.formatBytes(this.txBytes);
  }

  private formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  private showActivity(message: string, isError = false): void {
    const activity = this.element<HTMLElement>("#activity-message");
    activity.textContent = message;
    activity.classList.toggle("error", isError);
  }

  private scheduleSave(): void {
    if (!isTauriRuntime()) return;
    if (this.saveTimer !== undefined) window.clearTimeout(this.saveTimer);
    this.saveTimer = window.setTimeout(() => void this.saveConfig(), 350);
  }

  private async saveConfig(): Promise<void> {
    try {
      await invoke("save_app_config", { config: this.config });
    } catch (error) {
      this.showActivity(`Settings were not saved: ${String(error)}`, true);
    }
  }
}

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("Missing #app root");
new HushApp(root);
