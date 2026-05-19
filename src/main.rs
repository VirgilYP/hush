use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::env;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::mem;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_BAUD: u32 = 3_000_000;
const INPUT_MAX: usize = 4096;

#[derive(Clone, Copy)]
enum NewlineMode {
    Cr,
    Lf,
    Crlf,
}

impl NewlineMode {
    fn bytes(self) -> &'static [u8] {
        match self {
            NewlineMode::Cr => b"\r",
            NewlineMode::Lf => b"\n",
            NewlineMode::Crlf => b"\r\n",
        }
    }
}

struct Config {
    device: String,
    baud: u32,
    log_path: String,
    newline: NewlineMode,
}

#[derive(Default)]
struct OutputState {
    paused: bool,
    wait_for_prompt: bool,
    hidden: Vec<u8>,
}

struct TtyGuard {
    saved: libc::termios,
}

impl TtyGuard {
    fn enter_raw() -> io::Result<Self> {
        unsafe {
            let mut saved: libc::termios = mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut saved) != 0 {
                return Err(io::Error::last_os_error());
            }

            let mut raw = saved;
            raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
            raw.c_iflag &= !(libc::IXON | libc::IXOFF | libc::ICRNL);
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;

            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }

            Ok(Self { saved })
        }
    }
}

impl Drop for TtyGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.saved);
        }
    }
}

fn default_log_path() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("/tmp/uart-prompt-console-{secs}.log")
}

fn usage(program: &str) {
    eprintln!(
        "Usage: {program} [options] <device>\n\
         \n\
         Options:\n\
           -d <device>              Serial device. Also accepts positional <device>.\n\
           -b <baud>                Baud rate.\n\
           -l <logfile>             Log file path.\n\
           --newline cr|lf|crlf     Command line ending.\n\
         \n\
         Defaults:\n\
           baud:    {DEFAULT_BAUD}\n\
           newline: cr\n\
           logfile: /tmp/uart-prompt-console-<timestamp>.log\n\
         \n\
         Environment:\n\
           UART_PROMPT_DEVICE       Default device if -d/<device> is omitted.\n\
         \n\
         Keys:\n\
           Empty Enter    send newline, wait for '$', then pause at prompt\n\
           Enter          flush paused output, then send typed line\n\
           Ctrl-U         clear current input line\n\
           Backspace      edit current input line\n\
           Ctrl-C         send Ctrl-C to device\n\
           Ctrl-T r       resume realtime output\n\
           Ctrl-T q       quit\n\
           Ctrl-T l       clear screen\n\
           Ctrl-T ?       show help"
    );
}

fn parse_args() -> Result<Config, i32> {
    let mut device = env::var("UART_PROMPT_DEVICE").ok();
    let mut baud = DEFAULT_BAUD;
    let mut log_path = default_log_path();
    let mut newline = NewlineMode::Cr;

    let args: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-d" if i + 1 < args.len() => {
                i += 1;
                device = Some(args[i].clone());
            }
            "-b" if i + 1 < args.len() => {
                i += 1;
                baud = args[i].parse().map_err(|_| 2)?;
            }
            "-l" if i + 1 < args.len() => {
                i += 1;
                log_path = args[i].clone();
            }
            "--newline" if i + 1 < args.len() => {
                i += 1;
                newline = match args[i].as_str() {
                    "cr" => NewlineMode::Cr,
                    "lf" => NewlineMode::Lf,
                    "crlf" => NewlineMode::Crlf,
                    _ => return Err(2),
                };
            }
            "-h" | "--help" => {
                usage(&args[0]);
                return Err(0);
            }
            arg if !arg.starts_with('-') && device.is_none() => {
                device = Some(arg.to_string());
            }
            _ => {
                usage(&args[0]);
                return Err(2);
            }
        }
        i += 1;
    }

    let Some(device) = device else {
        usage(&args[0]);
        return Err(2);
    };

    Ok(Config {
        device,
        baud,
        log_path,
        newline,
    })
}

fn open_serial(config: &Config) -> Result<Box<dyn SerialPort>, serialport::Error> {
    serialport::new(&config.device, config.baud)
        .data_bits(DataBits::Eight)
        .flow_control(FlowControl::None)
        .parity(Parity::None)
        .stop_bits(StopBits::One)
        .timeout(Duration::from_millis(50))
        .open()
}

fn print_locked(screen: &Arc<Mutex<()>>, bytes: &[u8]) {
    let _guard = screen.lock().unwrap();
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(bytes);
    let _ = stdout.flush();
}

fn is_background_log_prefix(bytes: &[u8], index: usize) -> bool {
    let tail = &bytes[index..];
    tail.starts_with(b"[sr]")
        || tail.starts_with(b"[general]")
        || tail.starts_with(b"[audio]")
        || tail.starts_with(b"[dsp]")
}

fn prompt_display_end(bytes: &[u8]) -> Option<usize> {
    let mut index = bytes.iter().position(|byte| *byte == b'$')? + 1;

    loop {
        while matches!(bytes.get(index), Some(b' ' | b'\t')) {
            index += 1;
        }

        if bytes.get(index) == Some(&0x1b) && bytes.get(index + 1) == Some(&b'[') {
            index += 2;
            while let Some(byte) = bytes.get(index) {
                index += 1;
                if byte.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }

        break;
    }

    Some(index)
}

fn print_serial_locked(screen: &Arc<Mutex<()>>, bytes: &[u8], line_has_prompt: &mut bool) {
    let _guard = screen.lock().unwrap();
    let mut stdout = io::stdout().lock();

    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *line_has_prompt && is_background_log_prefix(bytes, index) {
            let _ = stdout.write_all(&bytes[start..index]);
            let _ = stdout.write_all(b"\r\n");
            start = index;
            *line_has_prompt = false;
        }

        match *byte {
            b'\r' | b'\n' => *line_has_prompt = false,
            b'$' => *line_has_prompt = true,
            _ => {}
        }
    }

    let _ = stdout.write_all(&bytes[start..]);
    let _ = stdout.flush();
}

fn redraw_input(screen: &Arc<Mutex<()>>, input: &[u8]) {
    let _guard = screen.lock().unwrap();
    let mut stdout = io::stdout().lock();
    let _ = stdout.write_all(b"\r\x1b[K> ");
    let _ = stdout.write_all(input);
    let _ = stdout.flush();
}

fn show_help(screen: &Arc<Mutex<()>>, input: &[u8], paused: bool) {
    print_locked(
        screen,
        b"\r\nuart-prompt-console keys:\r\n\
          Empty Enter send newline, wait for '$', then pause at prompt\r\n\
          Enter       flush paused output, then send typed line\r\n\
          Ctrl-U      clear input\r\n\
          Backspace   delete input char\r\n\
          Ctrl-C      send Ctrl-C to device\r\n\
          Ctrl-T r    resume realtime output\r\n\
          Ctrl-T q    quit\r\n\
          Ctrl-T l    clear screen\r\n\
          Ctrl-T ?    show this help\r\n\r\n",
    );
    if paused {
        redraw_input(screen, input);
    }
}

fn spawn_reader(
    mut serial: Box<dyn SerialPort>,
    stop: Arc<AtomicBool>,
    output_state: Arc<Mutex<OutputState>>,
    screen: Arc<Mutex<()>>,
    log_path: String,
) -> io::Result<thread::JoinHandle<()>> {
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    Ok(thread::spawn(move || {
        let mut buf = [0_u8; 8192];
        let mut line_has_prompt = false;
        while !stop.load(Ordering::Relaxed) {
            match serial.read(&mut buf) {
                Ok(n) if n > 0 => {
                    let data = &buf[..n];
                    let _ = log.write_all(data);
                    let _ = log.flush();

                    let mut state = output_state.lock().unwrap();
                    if state.paused {
                        state.hidden.extend_from_slice(data);
                    } else if state.wait_for_prompt {
                        if let Some(prompt_end) = prompt_display_end(data) {
                            state.wait_for_prompt = false;
                            state.paused = true;
                            state.hidden.extend_from_slice(&data[prompt_end..]);
                            drop(state);
                            print_serial_locked(&screen, &data[..prompt_end], &mut line_has_prompt);
                        } else {
                            drop(state);
                            print_serial_locked(&screen, data, &mut line_has_prompt);
                        }
                    } else {
                        drop(state);
                        print_serial_locked(&screen, data, &mut line_has_prompt);
                    }
                }
                Ok(_) => {}
                Err(ref err) if err.kind() == io::ErrorKind::TimedOut => {}
                Err(ref err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => {
                    let msg = format!("\r\nserial read error: {err}\r\n");
                    print_locked(&screen, msg.as_bytes());
                    break;
                }
            }
        }
    }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = match parse_args() {
        Ok(config) => config,
        Err(code) => std::process::exit(code),
    };

    let mut writer = open_serial(&config)?;
    let reader = writer.try_clone()?;

    eprintln!("connected: {} @ {} baud", config.device, config.baud);
    eprintln!("log: {}", config.log_path);
    eprintln!("empty Enter waits for '$' and pauses; Ctrl-T q quits");

    let _tty = TtyGuard::enter_raw()?;

    let stop = Arc::new(AtomicBool::new(false));
    let output_state = Arc::new(Mutex::new(OutputState::default()));
    let screen = Arc::new(Mutex::new(()));

    let reader_handle = spawn_reader(
        reader,
        stop.clone(),
        output_state.clone(),
        screen.clone(),
        config.log_path.clone(),
    )?;

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut input = Vec::with_capacity(INPUT_MAX);
    let mut prefix = false;
    let mut one = [0_u8; 1];

    loop {
        stdin.read_exact(&mut one)?;
        let ch = one[0];

        if prefix {
            prefix = false;
            match ch {
                b'q' => break,
                b'?' => {
                    let is_paused = output_state.lock().unwrap().paused;
                    show_help(&screen, &input, is_paused);
                }
                b'l' => {
                    print_locked(&screen, b"\x1b[2J\x1b[H");
                    if output_state.lock().unwrap().paused {
                        redraw_input(&screen, &input);
                    }
                }
                b'r' => {
                    let hidden = {
                        let mut state = output_state.lock().unwrap();
                        state.paused = false;
                        state.wait_for_prompt = false;
                        mem::take(&mut state.hidden)
                    };
                    if !hidden.is_empty() {
                        print_locked(&screen, b"\r\n");
                        print_locked(&screen, &hidden);
                    }
                }
                0x14 => writer.write_all(&[0x14])?,
                _ => {}
            }
            continue;
        }

        match ch {
            0x14 => prefix = true,
            b'\r' | b'\n' => {
                let was_paused = output_state.lock().unwrap().paused;
                if input.is_empty() && !was_paused {
                    {
                        let mut state = output_state.lock().unwrap();
                        state.paused = false;
                        state.wait_for_prompt = true;
                        state.hidden.clear();
                    }
                    writer.write_all(config.newline.bytes())?;
                    writer.flush()?;
                    continue;
                }

                if was_paused {
                    print_locked(&screen, b"\r\x1b[K");
                    let hidden = {
                        let mut state = output_state.lock().unwrap();
                        state.paused = false;
                        mem::take(&mut state.hidden)
                    };
                    if !hidden.is_empty() {
                        print_locked(&screen, &hidden);
                        if !matches!(hidden.last(), Some(b'\n' | b'\r')) {
                            print_locked(&screen, b"\r\n");
                        }
                    }
                }

                output_state.lock().unwrap().wait_for_prompt = false;
                writer.write_all(&input)?;
                writer.write_all(config.newline.bytes())?;
                writer.flush()?;

                input.clear();
            }
            0x15 => {
                if !input.is_empty() {
                    input.clear();
                    redraw_input(&screen, &input);
                }
            }
            0x7f | 0x08 => {
                if !input.is_empty() {
                    input.pop();
                    redraw_input(&screen, &input);
                }
            }
            0x03 => {
                writer.write_all(&[0x03])?;
                writer.flush()?;
            }
            0x20..=0x7e => {
                if input.len() + 1 < INPUT_MAX {
                    input.push(ch);
                    print_locked(&screen, &[ch]);
                }
            }
            _ => {}
        }
    }

    stop.store(true, Ordering::Relaxed);
    let _ = reader_handle.join();
    eprintln!("\nbye");
    eprintln!("log: {}", config.log_path);
    Ok(())
}
