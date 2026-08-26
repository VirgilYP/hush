use serde::{Deserialize, Serialize};
use serialport::{DataBits, FlowControl, Parity, SerialPort, SerialPortType, StopBits};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const DEFAULT_BAUD_RATE: u32 = 3_000_000;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnding {
    #[default]
    Cr,
    Lf,
    Crlf,
    None,
}

impl LineEnding {
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Self::Cr => b"\r",
            Self::Lf => b"\n",
            Self::Crlf => b"\r\n",
            Self::None => b"",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PayloadMode {
    #[default]
    Text,
    Hex,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialSettings {
    pub port_name: String,
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    #[serde(default)]
    pub parity: ParitySetting,
    #[serde(default)]
    pub flow_control: FlowControlSetting,
}

fn default_baud_rate() -> u32 {
    DEFAULT_BAUD_RATE
}

fn default_data_bits() -> u8 {
    8
}

fn default_stop_bits() -> u8 {
    1
}

impl Default for SerialSettings {
    fn default() -> Self {
        Self {
            port_name: String::new(),
            baud_rate: DEFAULT_BAUD_RATE,
            data_bits: 8,
            stop_bits: 1,
            parity: ParitySetting::None,
            flow_control: FlowControlSetting::None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParitySetting {
    #[default]
    None,
    Odd,
    Even,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FlowControlSetting {
    #[default]
    None,
    Software,
    Hardware,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInfo {
    pub port_name: String,
    pub kind: String,
    pub display_name: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
}

#[derive(Debug)]
pub enum SessionEvent {
    Received(Vec<u8>),
    ReadError(String),
    Disconnected,
}

#[derive(Debug, Error)]
pub enum HushError {
    #[error("serial port is not selected")]
    MissingPort,
    #[error("unsupported data bits: {0}")]
    UnsupportedDataBits(u8),
    #[error("unsupported stop bits: {0}")]
    UnsupportedStopBits(u8),
    #[error("invalid HEX payload: {0}")]
    InvalidHex(String),
    #[error("serial error: {0}")]
    Serial(#[from] serialport::Error),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

pub struct SerialSession {
    writer: Box<dyn SerialPort>,
    stop: Arc<AtomicBool>,
    reader_handle: Option<JoinHandle<()>>,
    log_path: PathBuf,
}

impl SerialSession {
    pub fn open(
        settings: &SerialSettings,
        log_path: Option<PathBuf>,
        events: Sender<SessionEvent>,
    ) -> Result<Self, HushError> {
        if !matches!(settings.data_bits, 5..=8) {
            return Err(HushError::UnsupportedDataBits(settings.data_bits));
        }
        if !matches!(settings.stop_bits, 1 | 2) {
            return Err(HushError::UnsupportedStopBits(settings.stop_bits));
        }
        let mut writer = open_serial_port(settings)?;
        let mut reader = writer.try_clone()?;
        let log_path = log_path.unwrap_or_else(default_log_path);
        let mut log = open_private_log(&log_path)?;
        let stop = Arc::new(AtomicBool::new(false));
        let reader_stop = stop.clone();

        writer.flush()?;
        let reader_handle = thread::spawn(move || {
            let mut buffer = [0_u8; 16 * 1024];
            while !reader_stop.load(Ordering::Relaxed) {
                match reader.read(&mut buffer) {
                    Ok(size) if size > 0 => {
                        let bytes = buffer[..size].to_vec();
                        if let Err(error) = log.write_all(&bytes).and_then(|_| log.flush()) {
                            let _ = events.send(SessionEvent::ReadError(format!(
                                "raw log write failed: {error}"
                            )));
                        }
                        if events.send(SessionEvent::Received(bytes)).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(ref error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => {
                        let _ = events.send(SessionEvent::ReadError(error.to_string()));
                        break;
                    }
                }
            }
            let _ = events.send(SessionEvent::Disconnected);
        });

        Ok(Self {
            writer,
            stop,
            reader_handle: Some(reader_handle),
            log_path,
        })
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<(), HushError> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    pub fn close(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.reader_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SerialSession {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn open_serial_port(
    settings: &SerialSettings,
) -> Result<Box<dyn SerialPort>, serialport::Error> {
    let port_name = preferred_serial_port_name(&settings.port_name);
    let data_bits = match settings.data_bits {
        5 => DataBits::Five,
        6 => DataBits::Six,
        7 => DataBits::Seven,
        _ => DataBits::Eight,
    };
    let stop_bits = if settings.stop_bits == 2 {
        StopBits::Two
    } else {
        StopBits::One
    };
    let parity = match settings.parity {
        ParitySetting::None => Parity::None,
        ParitySetting::Odd => Parity::Odd,
        ParitySetting::Even => Parity::Even,
    };
    let flow_control = match settings.flow_control {
        FlowControlSetting::None => FlowControl::None,
        FlowControlSetting::Software => FlowControl::Software,
        FlowControlSetting::Hardware => FlowControl::Hardware,
    };

    serialport::new(port_name, settings.baud_rate)
        .data_bits(data_bits)
        .flow_control(flow_control)
        .parity(parity)
        .stop_bits(stop_bits)
        .timeout(Duration::from_millis(50))
        .open()
}

pub fn list_serial_ports() -> Result<Vec<PortInfo>, serialport::Error> {
    let available = serialport::available_ports()?;
    #[cfg(target_os = "macos")]
    let available = available
        .into_iter()
        .filter(|port| !port.port_name.starts_with("/dev/tty."))
        .collect::<Vec<_>>();

    let mut ports = available
        .into_iter()
        .map(|port| match port.port_type {
            SerialPortType::UsbPort(usb) => {
                let product = usb.product.clone();
                let manufacturer = usb.manufacturer.clone();
                let detail = product
                    .clone()
                    .or_else(|| manufacturer.clone())
                    .unwrap_or_else(|| "USB serial".to_string());
                PortInfo {
                    display_name: format!("{detail} — {}", port.port_name),
                    port_name: port.port_name,
                    kind: "usb".to_string(),
                    vid: Some(usb.vid),
                    pid: Some(usb.pid),
                    serial_number: usb.serial_number,
                    manufacturer,
                    product,
                }
            }
            SerialPortType::BluetoothPort => PortInfo {
                display_name: format!("Bluetooth — {}", port.port_name),
                port_name: port.port_name,
                kind: "bluetooth".to_string(),
                vid: None,
                pid: None,
                serial_number: None,
                manufacturer: None,
                product: None,
            },
            SerialPortType::PciPort => PortInfo {
                display_name: format!("PCI serial — {}", port.port_name),
                port_name: port.port_name,
                kind: "pci".to_string(),
                vid: None,
                pid: None,
                serial_number: None,
                manufacturer: None,
                product: None,
            },
            SerialPortType::Unknown => PortInfo {
                display_name: port.port_name.clone(),
                port_name: port.port_name,
                kind: "unknown".to_string(),
                vid: None,
                pid: None,
                serial_number: None,
                manufacturer: None,
                product: None,
            },
        })
        .collect::<Vec<_>>();
    ports.sort_by(|left, right| left.port_name.cmp(&right.port_name));
    Ok(ports)
}

fn preferred_serial_port_name(port_name: &str) -> String {
    #[cfg(target_os = "macos")]
    if let Some(candidate) = macos_callout_candidate(port_name) {
        if Path::new(&candidate).exists() {
            return candidate;
        }
    }
    port_name.to_string()
}

#[cfg(target_os = "macos")]
fn macos_callout_candidate(port_name: &str) -> Option<String> {
    port_name
        .strip_prefix("/dev/tty.")
        .map(|suffix| format!("/dev/cu.{suffix}"))
}

pub fn encode_payload(
    mode: PayloadMode,
    value: &str,
    line_ending: LineEnding,
) -> Result<Vec<u8>, HushError> {
    match mode {
        PayloadMode::Text => {
            let mut bytes = value.as_bytes().to_vec();
            bytes.extend_from_slice(line_ending.bytes());
            Ok(bytes)
        }
        PayloadMode::Hex => parse_hex(value),
    }
}

pub fn parse_hex(value: &str) -> Result<Vec<u8>, HushError> {
    let compact = value
        .chars()
        .filter(|character| !character.is_ascii_whitespace() && !matches!(character, ',' | ':'))
        .collect::<String>();
    if compact.is_empty() {
        return Ok(Vec::new());
    }
    if compact.len() % 2 != 0 {
        return Err(HushError::InvalidHex(
            "expected an even number of hexadecimal digits".to_string(),
        ));
    }

    compact
        .as_bytes()
        .chunks_exact(2)
        .enumerate()
        .map(|(index, pair)| {
            let text = std::str::from_utf8(pair).expect("HEX input is ASCII-sized");
            u8::from_str_radix(text, 16).map_err(|_| {
                HushError::InvalidHex(format!("invalid byte '{text}' at position {index}"))
            })
        })
        .collect()
}

pub fn default_log_path() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    PathBuf::from(format!("/tmp/hush-gui-{timestamp}.log"))
}

fn open_private_log(path: &Path) -> io::Result<fs::File> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_payload_appends_the_selected_line_ending() {
        assert_eq!(
            encode_payload(PayloadMode::Text, "status", LineEnding::Cr).unwrap(),
            b"status\r"
        );
        assert_eq!(
            encode_payload(PayloadMode::Text, "status", LineEnding::Crlf).unwrap(),
            b"status\r\n"
        );
    }

    #[test]
    fn hex_payload_accepts_spaced_and_contiguous_bytes() {
        assert_eq!(parse_hex("13 00 ff 88").unwrap(), [0x13, 0x00, 0xff, 0x88]);
        assert_eq!(parse_hex("1300FF88").unwrap(), [0x13, 0x00, 0xff, 0x88]);
        assert_eq!(parse_hex("13,00:ff,88").unwrap(), [0x13, 0x00, 0xff, 0x88]);
    }

    #[test]
    fn hex_payload_rejects_odd_and_non_hex_input() {
        assert!(matches!(parse_hex("123"), Err(HushError::InvalidHex(_))));
        assert!(matches!(parse_hex("12 GG"), Err(HushError::InvalidHex(_))));
    }

    #[test]
    fn hex_payload_never_appends_a_line_ending() {
        assert_eq!(
            encode_payload(PayloadMode::Hex, "41 42", LineEnding::Crlf).unwrap(),
            b"AB"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn saved_tty_device_prefers_the_macos_callout_peer() {
        let suffix = format!("hush-test-missing-{}", std::process::id());
        assert_eq!(
            preferred_serial_port_name(&format!("/dev/tty.{suffix}")),
            format!("/dev/tty.{suffix}"),
            "a missing callout peer must not be invented"
        );

        assert_eq!(
            macos_callout_candidate("/dev/tty.usbmodem01234567893"),
            Some("/dev/cu.usbmodem01234567893".to_string())
        );
        assert_eq!(macos_callout_candidate("/dev/cu.usbmodem1"), None);
    }
}
