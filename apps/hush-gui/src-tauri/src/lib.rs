use hidapi::{HidApi, HidDevice};
use hush::{
    encode_payload, list_serial_ports, LineEnding, PayloadMode, PortInfo, SerialSession,
    SerialSettings, SessionEvent,
};
use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

struct AppState {
    serial_session: Arc<Mutex<Option<SerialSession>>>,
    hid_session: Arc<Mutex<Option<HidSession>>>,
    config_path: PathBuf,
}

const DM30_HID_VENDOR_ID: u16 = 0x1acc;
const DM30_HID_PRODUCT_IDS: [u16; 2] = [0x1a61, 0x1aec];
const DM30_HID_REPORT_ID: u8 = 1;
const DM30_HID_REPORT_SIZE: usize = 64;
const DM30_HID_PAYLOAD_SIZE: usize = DM30_HID_REPORT_SIZE - 1;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Transport {
    #[default]
    Uart,
    Hid,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum SerialUiEvent {
    Rx { bytes: Vec<u8> },
    Error { message: String },
    Disconnected,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionInfo {
    transport: Transport,
    display_name: String,
    detail: String,
    log_path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HidDeviceInfo {
    path: String,
    display_name: String,
    vendor_id: u16,
    product_id: u16,
    serial_number: Option<String>,
    manufacturer: Option<String>,
    product: Option<String>,
    usage_page: u16,
    usage: u16,
    interface_number: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendRequest {
    transport: Transport,
    mode: PayloadMode,
    value: String,
    line_ending: LineEnding,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchSendRequest {
    transport: Transport,
    commands: Vec<String>,
    line_ending: LineEnding,
    #[serde(default = "default_batch_delay_ms")]
    delay_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchSendResult {
    command_count: usize,
    byte_count: usize,
}

fn default_batch_delay_ms() -> u64 {
    20
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandSlot {
    id: String,
    name: String,
    mode: PayloadMode,
    value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedSerialSettings {
    port_name: String,
    baud_rate: u32,
    line_ending: LineEnding,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedHidSettings {
    path: String,
}

impl Default for PersistedSerialSettings {
    fn default() -> Self {
        Self {
            port_name: String::new(),
            baud_rate: 115_200,
            line_ending: LineEnding::Cr,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
struct UiPreferences {
    terminal_dark: bool,
    split_percent: u8,
    dm30_gain_mid: i32,
    dm30_gain_db: i32,
    dm30_algorithm_mid: i32,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            terminal_dark: true,
            split_percent: 64,
            dm30_gain_mid: 8,
            dm30_gain_db: 0,
            dm30_algorithm_mid: 1,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppConfig {
    version: u8,
    #[serde(default = "default_command_bank_name")]
    command_bank_name: String,
    #[serde(default)]
    transport: Transport,
    serial: PersistedSerialSettings,
    #[serde(default)]
    hid: PersistedHidSettings,
    ui: UiPreferences,
    slots: Vec<CommandSlot>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 2,
            command_bank_name: default_command_bank_name(),
            transport: Transport::Uart,
            serial: PersistedSerialSettings::default(),
            hid: PersistedHidSettings::default(),
            ui: UiPreferences::default(),
            slots: dm30_command_slots(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandBank {
    format: String,
    version: u8,
    name: String,
    slots: Vec<CommandSlot>,
}

fn default_command_bank_name() -> String {
    "DM30".to_string()
}

fn dm30_command_slots() -> Vec<CommandSlot> {
    let commands = [
        ("Power on", "set mid=-2,power=on"),
        ("Report all config", "set mid=-1"),
        ("MIC0 gain / level · MID 8", "set mid=8,rpsw=off,g=0"),
        ("MIC1 gain / level · MID 18", "set mid=18,rpsw=off,g=0"),
        (
            "Accompaniment gain / level · MID 47",
            "set mid=47,rpsw=off,g=0",
        ),
        (
            "Reverb wet gain / level · MID 53",
            "set mid=53,rpsw=off,g=0",
        ),
        (
            "Digital master gain / level · MID 101",
            "set mid=101,rpsw=off,g=0",
        ),
        (
            "Analog monitor gain / level · MID 121",
            "set mid=121,rpsw=off,g=0",
        ),
        ("UAC record gain · MID 102", "set mid=102,g=0"),
        ("OTG record gain · MID 111", "set mid=111,g=0"),
        ("DAC4344 gain · MID 122", "set mid=122,g=0"),
        ("Internal DAC gain · MID 131", "set mid=131,g=0"),
        (
            "⚠ IN1 32.0 dB setup · MID 9",
            "set mid=9,sw=on,ch=1,g=0.08,bst=1",
        ),
        (
            "⚠ IN1 31.9 dB transition · MID 9",
            "set mid=9,sw=on,ch=1,g=15.87,bst=2",
        ),
    ];
    commands
        .into_iter()
        .enumerate()
        .map(|(index, (name, value))| CommandSlot {
            id: format!("dm30-slot-{:02}", index + 1),
            name: name.to_string(),
            mode: PayloadMode::Text,
            value: value.to_string(),
        })
        .collect()
}

enum HidWorkerCommand {
    Write {
        report: [u8; DM30_HID_REPORT_SIZE],
        reply: mpsc::Sender<Result<usize, String>>,
    },
    Stop,
}

struct HidSession {
    commands: mpsc::Sender<HidWorkerCommand>,
    worker: Option<JoinHandle<()>>,
    log_path: PathBuf,
    alive: Arc<AtomicBool>,
}

impl HidSession {
    fn open(path: &str, on_event: Channel<SerialUiEvent>) -> Result<Self, String> {
        let c_path = CString::new(path.as_bytes()).map_err(|_| {
            "The selected HID device path contains an invalid NUL byte.".to_string()
        })?;
        let api = HidApi::new().map_err(|error| format!("Could not initialize HID: {error}"))?;
        let device = api
            .open_path(&c_path)
            .map_err(|error| format!("Could not open the selected HID interface: {error}"))?;
        let log_path = default_hid_log_path();
        let log = open_private_append_log(&log_path)
            .map_err(|error| format!("Could not open {}: {error}", log_path.display()))?;
        let (command_tx, command_rx) = mpsc::channel();
        let alive = Arc::new(AtomicBool::new(true));
        let worker_alive = alive.clone();
        let worker = std::thread::spawn(move || {
            run_hid_worker(device, log, command_rx, on_event, &worker_alive);
            worker_alive.store(false, Ordering::Release);
        });
        Ok(Self {
            commands: command_tx,
            worker: Some(worker),
            log_path,
            alive,
        })
    }

    fn send(&self, payload: &[u8]) -> Result<usize, String> {
        if !self.alive.load(Ordering::Acquire) {
            return Err("The HID session has ended. Reconnect the device.".to_string());
        }
        let report = build_dm30_hid_report(payload)?;
        let (reply_tx, reply_rx) = mpsc::channel();
        self.commands
            .send(HidWorkerCommand::Write {
                report,
                reply: reply_tx,
            })
            .map_err(|_| "The HID worker is no longer running.".to_string())?;
        reply_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "Timed out waiting for the HID write to complete.".to_string())??;
        Ok(payload.len())
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    fn log_path(&self) -> &Path {
        &self.log_path
    }

    fn close(&mut self) {
        let _ = self.commands.send(HidWorkerCommand::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.alive.store(false, Ordering::Release);
    }
}

impl Drop for HidSession {
    fn drop(&mut self) {
        self.close();
    }
}

fn run_hid_worker(
    device: HidDevice,
    mut log: fs::File,
    commands: mpsc::Receiver<HidWorkerCommand>,
    on_event: Channel<SerialUiEvent>,
    alive: &AtomicBool,
) {
    let mut read_buffer = [0u8; DM30_HID_REPORT_SIZE];
    let mut stopped = false;
    while !stopped {
        while let Ok(command) = commands.try_recv() {
            match command {
                HidWorkerCommand::Write { report, reply } => {
                    let result = write_hid_report(&device, &report);
                    let failed = result.is_err();
                    let _ = reply.send(result);
                    if failed {
                        stopped = true;
                        break;
                    }
                }
                HidWorkerCommand::Stop => {
                    stopped = true;
                    break;
                }
            }
        }
        if stopped {
            break;
        }

        match device.read_timeout(&mut read_buffer, 8) {
            Ok(0) => {}
            Ok(count) => {
                let raw = &read_buffer[..count];
                if let Err(error) = log.write_all(raw).and_then(|_| log.flush()) {
                    let _ = on_event.send(SerialUiEvent::Error {
                        message: format!("HID raw log write failed: {error}"),
                    });
                }
                let bytes = decode_dm30_hid_report(raw);
                if !bytes.is_empty() && on_event.send(SerialUiEvent::Rx { bytes }).is_err() {
                    stopped = true;
                }
            }
            Err(error) => {
                let _ = on_event.send(SerialUiEvent::Error {
                    message: format!("HID read failed: {error}"),
                });
                let _ = on_event.send(SerialUiEvent::Disconnected);
                stopped = true;
            }
        }
    }
    alive.store(false, Ordering::Release);
}

fn write_hid_report(
    device: &HidDevice,
    report: &[u8; DM30_HID_REPORT_SIZE],
) -> Result<usize, String> {
    let written = device
        .write(report)
        .map_err(|error| format!("HID write failed: {error}"))?;
    if written != report.len() {
        return Err(format!(
            "HID write was incomplete: wrote {written} of {} bytes.",
            report.len()
        ));
    }
    Ok(written)
}

fn build_dm30_hid_report(payload: &[u8]) -> Result<[u8; DM30_HID_REPORT_SIZE], String> {
    if payload.is_empty() {
        return Err("Command is empty.".to_string());
    }
    if payload.len() > DM30_HID_PAYLOAD_SIZE {
        return Err(format!(
            "DM30 HID commands are limited to {DM30_HID_PAYLOAD_SIZE} bytes including the line ending; this command is {} bytes.",
            payload.len()
        ));
    }
    let mut report = [0u8; DM30_HID_REPORT_SIZE];
    report[0] = DM30_HID_REPORT_ID;
    report[1..1 + payload.len()].copy_from_slice(payload);
    Ok(report)
}

fn decode_dm30_hid_report(report: &[u8]) -> Vec<u8> {
    if report.first().copied() != Some(DM30_HID_REPORT_ID) {
        return Vec::new();
    }
    let end = report
        .iter()
        .rposition(|byte| *byte != 0)
        .map(|index| index + 1)
        .unwrap_or(1);
    report[1..end].to_vec()
}

fn is_hid_power_command(payload: &[u8]) -> bool {
    let value = String::from_utf8_lossy(payload).to_ascii_lowercase();
    value.contains("mid=-2") && value.contains("power=")
}

fn default_hid_log_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    PathBuf::from(format!("/tmp/hush-gui-hid-{timestamp}.log"))
}

fn open_private_append_log(path: &Path) -> std::io::Result<fs::File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

#[tauri::command]
fn serial_ports() -> Result<Vec<PortInfo>, String> {
    list_serial_ports().map_err(|error| error.to_string())
}

#[tauri::command]
fn hid_devices() -> Result<Vec<HidDeviceInfo>, String> {
    let api = HidApi::new().map_err(|error| format!("Could not scan HID devices: {error}"))?;
    let mut devices = api
        .device_list()
        .filter(|device| is_dm30_hid_identity(device.vendor_id(), device.product_id()))
        .map(|device| {
            let product = device.product_string().map(str::to_string);
            let serial_number = device.serial_number().map(str::to_string);
            let identity = product
                .clone()
                .unwrap_or_else(|| "DM30 USB HID".to_string());
            let serial_suffix = serial_number
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(|value| format!(" · {value}"))
                .unwrap_or_default();
            HidDeviceInfo {
                path: device.path().to_string_lossy().into_owned(),
                display_name: format!(
                    "{identity} · {:04X}:{:04X} · IF {}{serial_suffix}",
                    device.vendor_id(),
                    device.product_id(),
                    device.interface_number()
                ),
                vendor_id: device.vendor_id(),
                product_id: device.product_id(),
                serial_number,
                manufacturer: device.manufacturer_string().map(str::to_string),
                product,
                usage_page: device.usage_page(),
                usage: device.usage(),
                interface_number: device.interface_number(),
            }
        })
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(devices)
}

fn is_dm30_hid_identity(vendor_id: u16, product_id: u16) -> bool {
    vendor_id == DM30_HID_VENDOR_ID && DM30_HID_PRODUCT_IDS.contains(&product_id)
}

#[tauri::command]
async fn connect_serial(
    settings: SerialSettings,
    on_event: Channel<SerialUiEvent>,
    state: State<'_, AppState>,
) -> Result<ConnectionInfo, String> {
    if settings.port_name.trim().is_empty() {
        return Err("Select a serial port before connecting.".to_string());
    }

    {
        let guard = state
            .serial_session
            .lock()
            .map_err(|_| "serial session lock was poisoned".to_string())?;
        if guard.is_some() {
            return Err("A serial port is already connected in this window.".to_string());
        }
    }
    {
        let guard = state
            .hid_session
            .lock()
            .map_err(|_| "HID session lock was poisoned".to_string())?;
        if guard.as_ref().is_some_and(HidSession::is_alive) {
            return Err("Disconnect USB HID before opening a UART session.".to_string());
        }
    }

    let connected_port_name = settings.port_name.clone();
    let connected_baud_rate = settings.baud_rate;
    let (event_tx, event_rx) = mpsc::channel();
    let session = tauri::async_runtime::spawn_blocking(move || {
        SerialSession::open(&settings, None, event_tx)
    })
    .await
    .map_err(|error| format!("serial open task failed: {error}"))?
    .map_err(|error| error.to_string())?;
    let info = ConnectionInfo {
        transport: Transport::Uart,
        display_name: connected_port_name.clone(),
        detail: format!("{connected_port_name} · {connected_baud_rate} baud · 8-N-1"),
        log_path: session.log_path().to_string_lossy().to_string(),
    };
    let mut guard = state
        .serial_session
        .lock()
        .map_err(|_| "serial session lock was poisoned".to_string())?;
    *guard = Some(session);
    drop(guard);

    let session_store = state.serial_session.clone();
    std::thread::spawn(move || {
        let mut receive_batch = Vec::with_capacity(16 * 1024);
        loop {
            match event_rx.recv_timeout(Duration::from_millis(8)) {
                Ok(SessionEvent::Received(bytes)) => {
                    receive_batch.extend_from_slice(&bytes);
                    if receive_batch.len() >= 16 * 1024
                        && !flush_receive_batch(&on_event, &mut receive_batch)
                    {
                        break;
                    }
                }
                Ok(SessionEvent::ReadError(message)) => {
                    if !flush_receive_batch(&on_event, &mut receive_batch)
                        || on_event.send(SerialUiEvent::Error { message }).is_err()
                    {
                        break;
                    }
                }
                Ok(SessionEvent::Disconnected) => {
                    let _ = flush_receive_batch(&on_event, &mut receive_batch);
                    let _ = on_event.send(SerialUiEvent::Disconnected);
                    if let Ok(mut session) = session_store.lock() {
                        let _ = session.take();
                    }
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {
                    if !flush_receive_batch(&on_event, &mut receive_batch) {
                        break;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = flush_receive_batch(&on_event, &mut receive_batch);
                    break;
                }
            }
        }
    });

    Ok(info)
}

#[tauri::command]
async fn connect_hid(
    path: String,
    on_event: Channel<SerialUiEvent>,
    state: State<'_, AppState>,
) -> Result<ConnectionInfo, String> {
    if path.trim().is_empty() {
        return Err("Select the DM30 HID interface before connecting.".to_string());
    }
    {
        let guard = state
            .serial_session
            .lock()
            .map_err(|_| "serial session lock was poisoned".to_string())?;
        if guard.is_some() {
            return Err("Disconnect UART before opening USB HID.".to_string());
        }
    }
    {
        let mut guard = state
            .hid_session
            .lock()
            .map_err(|_| "HID session lock was poisoned".to_string())?;
        if let Some(session) = guard.as_mut() {
            if session.is_alive() {
                return Err("A USB HID device is already connected in this window.".to_string());
            }
            session.close();
            let _ = guard.take();
        }
    }

    let selected = hid_devices()?
        .into_iter()
        .find(|device| device.path == path)
        .ok_or_else(|| {
            "The selected DM30 HID interface is no longer present. Refresh devices and try again."
                .to_string()
        })?;
    let open_path = selected.path.clone();
    let session =
        tauri::async_runtime::spawn_blocking(move || HidSession::open(&open_path, on_event))
            .await
            .map_err(|error| format!("HID open task failed: {error}"))??;
    let info = ConnectionInfo {
        transport: Transport::Hid,
        display_name: selected.display_name.clone(),
        detail: format!(
            "{} · report ID {} · {}-byte commands",
            selected.display_name, DM30_HID_REPORT_ID, DM30_HID_PAYLOAD_SIZE
        ),
        log_path: session.log_path().to_string_lossy().to_string(),
    };
    let mut guard = state
        .hid_session
        .lock()
        .map_err(|_| "HID session lock was poisoned".to_string())?;
    *guard = Some(session);
    Ok(info)
}

fn flush_receive_batch(channel: &Channel<SerialUiEvent>, receive_batch: &mut Vec<u8>) -> bool {
    if receive_batch.is_empty() {
        return true;
    }
    let bytes = std::mem::take(receive_batch);
    channel.send(SerialUiEvent::Rx { bytes }).is_ok()
}

#[tauri::command]
fn disconnect_serial(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state
        .serial_session
        .lock()
        .map_err(|_| "serial session lock was poisoned".to_string())?;
    if let Some(mut session) = guard.take() {
        session.close();
    }
    Ok(())
}

#[tauri::command]
fn disconnect_hid(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state
        .hid_session
        .lock()
        .map_err(|_| "HID session lock was poisoned".to_string())?;
    if let Some(mut session) = guard.take() {
        session.close();
    }
    Ok(())
}

#[tauri::command]
fn send_payload(request: SendRequest, state: State<'_, AppState>) -> Result<usize, String> {
    let bytes = encode_payload(request.mode, &request.value, request.line_ending)
        .map_err(|error| error.to_string())?;
    if bytes.is_empty() {
        return Err("Command is empty.".to_string());
    }
    send_encoded_payload(request.transport, &bytes, &state)
}

fn send_encoded_payload(
    transport: Transport,
    bytes: &[u8],
    state: &State<'_, AppState>,
) -> Result<usize, String> {
    match transport {
        Transport::Uart => {
            let mut guard = state
                .serial_session
                .lock()
                .map_err(|_| "serial session lock was poisoned".to_string())?;
            let session = guard
                .as_mut()
                .ok_or_else(|| "Connect a serial port before sending.".to_string())?;
            session.send(bytes).map_err(|error| error.to_string())?;
            Ok(bytes.len())
        }
        Transport::Hid => {
            if is_hid_power_command(bytes) {
                return Err(
                    "DM30 power on cannot be sent over HID: the USB HID interface only exists after USB power is already on, and the current firmware accepts this command from UART/shell only."
                        .to_string(),
                );
            }
            let guard = state
                .hid_session
                .lock()
                .map_err(|_| "HID session lock was poisoned".to_string())?;
            let session = guard
                .as_ref()
                .ok_or_else(|| "Connect the DM30 USB HID interface before sending.".to_string())?;
            session.send(bytes)
        }
    }
}

#[tauri::command]
fn send_batch(
    request: BatchSendRequest,
    state: State<'_, AppState>,
) -> Result<BatchSendResult, String> {
    if request.commands.is_empty() || request.commands.len() > 64 {
        return Err("Batch must contain 1 to 64 commands.".to_string());
    }
    if request.delay_ms > 5_000 {
        return Err("Batch delay cannot exceed 5000 ms.".to_string());
    }

    let payloads = request
        .commands
        .iter()
        .enumerate()
        .map(|(index, command)| {
            if command.trim().is_empty() {
                return Err(format!("Batch command {} is empty.", index + 1));
            }
            encode_payload(PayloadMode::Text, command, request.line_ending)
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut byte_count = 0;
    for (index, payload) in payloads.iter().enumerate() {
        byte_count += send_encoded_payload(request.transport, payload, &state)
            .map_err(|error| format!("Batch command {} failed: {error}", index + 1))?;
        if index + 1 < payloads.len() && request.delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(request.delay_ms));
        }
    }

    Ok(BatchSendResult {
        command_count: payloads.len(),
        byte_count,
    })
}

#[tauri::command]
fn load_app_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    match fs::read(&state.config_path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| format!("Could not parse {}: {error}", state.config_path.display())),
        Err(ref error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AppConfig::default()),
        Err(error) => Err(format!(
            "Could not read {}: {error}",
            state.config_path.display()
        )),
    }
}

#[tauri::command]
fn save_app_config(config: AppConfig, state: State<'_, AppState>) -> Result<(), String> {
    write_config_atomically(&state.config_path, &config)
        .map_err(|error| format!("Could not save {}: {error}", state.config_path.display()))
}

#[tauri::command]
fn import_command_bank(app: AppHandle) -> Result<Option<CommandBank>, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("Load Hush command bank")
        .add_filter("Hush command bank", &["json"])
        .blocking_pick_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|error| format!("Selected file is not a local path: {error}"))?;
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    if metadata.len() > 1_048_576 {
        return Err("Command bank is larger than 1 MiB.".to_string());
    }
    let bytes =
        fs::read(&path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let bank: CommandBank = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Could not parse {}: {error}", path.display()))?;
    validate_command_bank(&bank)?;
    Ok(Some(bank))
}

#[tauri::command]
fn export_command_bank(app: AppHandle, bank: CommandBank) -> Result<Option<String>, String> {
    validate_command_bank(&bank)?;
    let file_name = format!("{}.hush-commands.json", safe_file_stem(&bank.name));
    let selected = app
        .dialog()
        .file()
        .set_title("Export Hush command bank")
        .set_file_name(file_name)
        .add_filter("Hush command bank", &["json"])
        .blocking_save_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|error| format!("Selected file is not a local path: {error}"))?;
    let encoded = serde_json::to_vec_pretty(&bank)
        .map_err(|error| format!("Could not encode command bank: {error}"))?;
    fs::write(&path, encoded)
        .map_err(|error| format!("Could not write {}: {error}", path.display()))?;
    Ok(Some(path.to_string_lossy().to_string()))
}

fn validate_command_bank(bank: &CommandBank) -> Result<(), String> {
    if bank.format != "hush-command-bank" {
        return Err("This is not a Hush command bank file.".to_string());
    }
    if bank.version != 1 {
        return Err(format!(
            "Unsupported command bank version {}. This build supports version 1.",
            bank.version
        ));
    }
    if bank.name.trim().is_empty() || bank.name.chars().count() > 128 {
        return Err("Command bank name must contain 1 to 128 characters.".to_string());
    }
    if bank.slots.is_empty() || bank.slots.len() > 256 {
        return Err("Command bank must contain 1 to 256 slots.".to_string());
    }
    for (index, slot) in bank.slots.iter().enumerate() {
        if slot.name.chars().count() > 128 {
            return Err(format!(
                "Slot {} name is longer than 128 characters.",
                index + 1
            ));
        }
        if slot.value.len() > 4096 {
            return Err(format!(
                "Slot {} command is larger than 4096 bytes.",
                index + 1
            ));
        }
    }
    Ok(())
}

fn safe_file_stem(name: &str) -> String {
    let value = name
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let value = value.trim_matches('-');
    if value.is_empty() {
        "commands".to_string()
    } else {
        value.to_string()
    }
}

fn write_config_atomically(path: &Path, config: &AppConfig) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let encoded = serde_json::to_vec_pretty(config).map_err(std::io::Error::other)?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    fs::rename(temporary, path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                window.clear_all_browsing_data()?;
            }
            let config_path = app.path().app_config_dir()?.join("config.json");
            app.manage(AppState {
                serial_session: Arc::new(Mutex::new(None)),
                hid_session: Arc::new(Mutex::new(None)),
                config_path,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            serial_ports,
            hid_devices,
            connect_serial,
            connect_hid,
            disconnect_serial,
            disconnect_hid,
            send_payload,
            send_batch,
            load_app_config,
            save_app_config,
            import_command_bank,
            export_command_bank
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hush");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_fourteen_editable_slots() {
        let config = AppConfig::default();
        assert_eq!(config.slots.len(), 14);
        assert_eq!(config.command_bank_name, "DM30");
        assert_eq!(config.serial.baud_rate, 115_200);
        assert_eq!(config.slots[0].value, "set mid=-2,power=on");
        assert_eq!(config.slots[13].value, "set mid=9,sw=on,ch=1,g=15.87,bst=2");
    }

    #[test]
    fn config_round_trips_as_camel_case_json() {
        let config = AppConfig::default();
        let encoded = serde_json::to_string(&config).unwrap();
        assert!(encoded.contains("\"transport\":\"uart\""));
        assert!(encoded.contains("\"hid\""));
        assert!(encoded.contains("baudRate"));
        assert!(encoded.contains("terminalDark"));
        assert!(encoded.contains("commandBankName"));
        let decoded: AppConfig = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.slots.len(), 14);
    }

    #[test]
    fn command_bank_validation_rejects_unknown_formats() {
        let config = AppConfig::default();
        let bank = CommandBank {
            format: "other".to_string(),
            version: 1,
            name: config.command_bank_name,
            slots: config.slots,
        };
        assert!(validate_command_bank(&bank).is_err());
    }

    #[test]
    fn ui_defaults_select_a_safe_dm30_gain_range_endpoint() {
        let preferences = UiPreferences::default();
        assert_eq!(preferences.dm30_gain_mid, 8);
        assert_eq!(preferences.dm30_gain_db, 0);
        assert_eq!(preferences.dm30_algorithm_mid, 1);
    }

    #[test]
    fn dm30_hid_report_uses_id_one_and_zero_padding() {
        let payload = b"set mid=8,g=-6\r";
        let report = build_dm30_hid_report(payload).unwrap();
        assert_eq!(report.len(), 64);
        assert_eq!(report[0], 1);
        assert_eq!(&report[1..1 + payload.len()], payload);
        assert!(report[1 + payload.len()..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn dm30_hid_discovery_accepts_legacy_and_current_product_ids() {
        assert!(is_dm30_hid_identity(0x1acc, 0x1a61));
        assert!(is_dm30_hid_identity(0x1acc, 0x1aec));
        assert!(!is_dm30_hid_identity(0x1acc, 0x1a62));
        assert!(!is_dm30_hid_identity(0x1234, 0x1aec));
    }

    #[test]
    fn dm30_hid_report_rejects_payloads_over_sixty_three_bytes() {
        assert!(build_dm30_hid_report(&[b'x'; 63]).is_ok());
        let error = build_dm30_hid_report(&[b'x'; 64]).unwrap_err();
        assert!(error.contains("limited to 63 bytes"));
    }

    #[test]
    fn dm30_hid_input_strips_report_id_and_zero_padding() {
        let mut report = [0u8; 64];
        report[0] = 1;
        report[1..5].copy_from_slice(b"ok\r\n");
        assert_eq!(decode_dm30_hid_report(&report), b"ok\r\n");
        report[0] = 2;
        assert!(decode_dm30_hid_report(&report).is_empty());
    }

    #[test]
    fn dm30_hid_power_command_is_rejected_before_device_io() {
        assert!(is_hid_power_command(b"set mid=-2,power=on\r"));
        assert!(!is_hid_power_command(b"set mid=8,g=0\r"));
    }
}
