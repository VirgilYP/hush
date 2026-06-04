use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::mem;
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
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
    monitor_name: String,
    monitor_socket_path: String,
}

struct MonitorConfig {
    socket_path: String,
    read_only: bool,
}

enum Command {
    Session(Config),
    Monitor(MonitorConfig),
    Sessions,
}

enum InteractiveMonitorExit {
    Detached,
    PrimaryClosed,
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
    format!("/tmp/hush-{secs}.log")
}

fn sanitize_session_name(name: &str) -> String {
    let mut sanitized = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
        }
    }

    let sanitized = sanitized.trim_matches('_').to_string();
    if sanitized.is_empty() {
        "session".to_string()
    } else {
        sanitized
    }
}

fn default_session_name(device: &str) -> String {
    let name = Path::new(device)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(device);
    sanitize_session_name(name)
}

fn session_name_from_arg(arg: &str) -> String {
    if arg.contains('/') {
        default_session_name(arg)
    } else {
        sanitize_session_name(arg)
    }
}

fn socket_path_for_session(session_name: &str) -> String {
    format!("/tmp/hush-{session_name}.sock")
}

fn usage(program: &str) {
    eprintln!(
        "Usage:\n\
           {program} [options] <device>\n\
           {program} monitor [--read-only] <session>\n\
           {program} monitor [--read-only] --socket <path>\n\
           {program} monitor --list\n\
           {program} sessions\n\
         \n\
         Options:\n\
           -d <device>              Serial device. Also accepts positional <device>.\n\
           -b <baud>                Baud rate.\n\
           -l <logfile>             Log file path.\n\
           --session <name>         Monitor session name.\n\
           --newline cr|lf|crlf     Command line ending.\n\
         \n\
         Defaults:\n\
           baud:    {DEFAULT_BAUD}\n\
           newline: cr\n\
           logfile: /tmp/hush-<timestamp>.log\n\
           session: serial device basename\n\
         \n\
         Environment:\n\
           HUSH_DEVICE              Default device if -d/<device> is omitted.\n\
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
           Ctrl-T ?       show help\n\
           Ctrl-]         detach an interactive monitor"
    );
}

fn parse_args() -> Result<Command, i32> {
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(|arg| arg.as_str()) == Some("monitor") {
        return parse_monitor_args(&args);
    }
    if matches!(args.get(1).map(|arg| arg.as_str()), Some("sessions" | "ls")) {
        return Ok(Command::Sessions);
    }

    parse_session_args(&args)
}

fn parse_monitor_args(args: &[String]) -> Result<Command, i32> {
    let mut session_name = None;
    let mut socket_path = None;
    let mut read_only = false;
    let mut list = false;
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--read-only" => read_only = true,
            "--list" | "-l" => list = true,
            "--socket" if i + 1 < args.len() => {
                i += 1;
                socket_path = Some(args[i].clone());
            }
            "-h" | "--help" => {
                usage(&args[0]);
                return Err(0);
            }
            arg if !arg.starts_with('-') && session_name.is_none() => {
                session_name = Some(session_name_from_arg(arg));
            }
            _ => {
                usage(&args[0]);
                return Err(2);
            }
        }
        i += 1;
    }

    if list {
        return Ok(Command::Sessions);
    }

    let socket_path = match socket_path {
        Some(path) => path,
        None => {
            let Some(session_name) = session_name else {
                return Ok(Command::Sessions);
            };
            socket_path_for_session(&session_name)
        }
    };

    Ok(Command::Monitor(MonitorConfig {
        socket_path,
        read_only,
    }))
}

fn parse_session_args(args: &[String]) -> Result<Command, i32> {
    let mut device = env::var("HUSH_DEVICE").ok();
    let mut baud = DEFAULT_BAUD;
    let mut log_path = default_log_path();
    let mut newline = NewlineMode::Cr;
    let mut session_name = None;

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
            "--session" if i + 1 < args.len() => {
                i += 1;
                session_name = Some(sanitize_session_name(&args[i]));
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

    let monitor_name = session_name.unwrap_or_else(|| default_session_name(&device));
    let monitor_socket_path = socket_path_for_session(&monitor_name);

    Ok(Command::Session(Config {
        device,
        baud,
        log_path,
        newline,
        monitor_name,
        monitor_socket_path,
    }))
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

#[derive(Default)]
struct MonitorHub {
    clients: Mutex<Vec<UnixStream>>,
}

impl MonitorHub {
    fn add_client(&self, stream: UnixStream) {
        self.clients.lock().unwrap().push(stream);
    }

    fn broadcast(&self, bytes: &[u8]) {
        let mut clients = self.clients.lock().unwrap();
        let mut index = 0;
        while index < clients.len() {
            match clients[index].write_all(bytes) {
                Ok(()) => index += 1,
                Err(ref err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::BrokenPipe
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::NotConnected
                    ) =>
                {
                    let _ = clients.remove(index);
                }
                Err(_) => {
                    let _ = clients.remove(index);
                }
            }
        }
    }
}

#[derive(Clone)]
struct Screen {
    lock: Arc<Mutex<()>>,
    monitor: Arc<MonitorHub>,
}

impl Screen {
    fn new(monitor: Arc<MonitorHub>) -> Self {
        Self {
            lock: Arc::new(Mutex::new(())),
            monitor,
        }
    }
}

fn write_screen_bytes(stdout: &mut io::StdoutLock<'_>, monitor: &MonitorHub, bytes: &[u8]) {
    let _ = stdout.write_all(bytes);
    monitor.broadcast(bytes);
}

fn print_locked(screen: &Screen, bytes: &[u8]) {
    let _guard = screen.lock.lock().unwrap();
    let mut stdout = io::stdout().lock();
    write_screen_bytes(&mut stdout, &screen.monitor, bytes);
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

fn print_serial_locked(screen: &Screen, bytes: &[u8], line_has_prompt: &mut bool) {
    let _guard = screen.lock.lock().unwrap();
    let mut stdout = io::stdout().lock();

    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *line_has_prompt && is_background_log_prefix(bytes, index) {
            write_screen_bytes(&mut stdout, &screen.monitor, &bytes[start..index]);
            write_screen_bytes(&mut stdout, &screen.monitor, b"\r\n");
            start = index;
            *line_has_prompt = false;
        }

        match *byte {
            b'\r' | b'\n' => *line_has_prompt = false,
            b'$' => *line_has_prompt = true,
            _ => {}
        }
    }

    write_screen_bytes(&mut stdout, &screen.monitor, &bytes[start..]);
    let _ = stdout.flush();
}

fn redraw_input(screen: &Screen, input: &[u8]) {
    let _guard = screen.lock.lock().unwrap();
    let mut stdout = io::stdout().lock();
    write_screen_bytes(&mut stdout, &screen.monitor, b"\r\x1b[K> ");
    write_screen_bytes(&mut stdout, &screen.monitor, input);
    let _ = stdout.flush();
}

fn show_help(screen: &Screen, input: &[u8], paused: bool) {
    print_locked(
        screen,
        b"\r\nhush keys:\r\n\
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
    screen: Screen,
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

fn bind_monitor_listener(socket_path: &str) -> io::Result<UnixListener> {
    if Path::new(socket_path).exists() {
        match UnixStream::connect(socket_path) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("monitor socket already in use: {socket_path}"),
                ));
            }
            Err(_) => fs::remove_file(socket_path)?,
        }
    }

    let listener = UnixListener::bind(socket_path)?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

fn spawn_monitor_acceptor(
    socket_path: &str,
    stop: Arc<AtomicBool>,
    monitor: Arc<MonitorHub>,
    input_tx: mpsc::Sender<u8>,
) -> io::Result<thread::JoinHandle<()>> {
    let listener = bind_monitor_listener(socket_path)?;

    Ok(thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    if stream.set_nonblocking(true).is_ok() {
                        match stream.try_clone() {
                            Ok(input_stream) => {
                                monitor.add_client(stream);
                                spawn_monitor_input_reader(
                                    input_stream,
                                    stop.clone(),
                                    input_tx.clone(),
                                );
                            }
                            Err(_) => {
                                let _ = stream.shutdown(std::net::Shutdown::Both);
                            }
                        }
                    }
                }
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(ref err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    }))
}

fn spawn_monitor_input_reader(
    mut stream: UnixStream,
    stop: Arc<AtomicBool>,
    input_tx: mpsc::Sender<u8>,
) {
    thread::spawn(move || {
        let mut buf = [0_u8; 256];
        while !stop.load(Ordering::Relaxed) {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    for byte in &buf[..n] {
                        if input_tx.send(*byte).is_err() {
                            return;
                        }
                    }
                }
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(ref err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });
}

fn spawn_stdin_reader(input_tx: mpsc::Sender<u8>) {
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut stdin = stdin.lock();
        let mut one = [0_u8; 1];

        loop {
            match stdin.read_exact(&mut one) {
                Ok(()) => {
                    if input_tx.send(one[0]).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn run_monitor(config: MonitorConfig) -> io::Result<()> {
    let mut stream = UnixStream::connect(&config.socket_path)?;

    eprintln!("monitoring: {}", config.socket_path);
    if config.read_only {
        eprintln!("mode: read-only");
        stream_to_stdout(&mut stream)?;
        eprintln!("\nmonitor ended");
        return Ok(());
    }

    eprintln!("mode: interactive; Ctrl-] detaches; Ctrl-T keys control hush");

    match run_interactive_monitor(stream)? {
        InteractiveMonitorExit::Detached => eprintln!("\nmonitor detached"),
        InteractiveMonitorExit::PrimaryClosed => eprintln!("\nmonitor ended: primary hush exited"),
    }

    Ok(())
}

fn active_sessions() -> io::Result<Vec<(String, String)>> {
    let mut sessions = Vec::new();

    for entry in fs::read_dir("/tmp")? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(name) = file_name
            .strip_prefix("hush-")
            .and_then(|name| name.strip_suffix(".sock"))
        else {
            continue;
        };

        let path = entry.path();
        let socket_path = path.to_string_lossy().to_string();
        if UnixStream::connect(&path).is_ok() {
            sessions.push((name.to_string(), socket_path));
        }
    }

    sessions.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(sessions)
}

fn run_sessions() -> io::Result<()> {
    let sessions = active_sessions()?;

    if sessions.is_empty() {
        println!("no active hush monitor sessions");
        return Ok(());
    }

    println!("active hush monitor sessions:");
    for (name, socket_path) in sessions {
        println!("  {name}\t{socket_path}");
    }

    Ok(())
}

fn run_interactive_monitor(mut stream: UnixStream) -> io::Result<InteractiveMonitorExit> {
    let _tty = TtyGuard::enter_raw()?;
    let stream_fd = stream.as_raw_fd();

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut stdout = io::stdout().lock();
    let mut input = [0_u8; 256];
    let mut output = [0_u8; 8192];

    loop {
        let mut fds = [
            libc::pollfd {
                fd: libc::STDIN_FILENO,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: stream_fd,
                events: libc::POLLIN | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL,
                revents: 0,
            },
        ];

        let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }

        let stream_revents = fds[1].revents;
        if stream_revents & libc::POLLIN != 0 {
            match stream.read(&mut output) {
                Ok(0) => return Ok(InteractiveMonitorExit::PrimaryClosed),
                Ok(n) => {
                    stdout.write_all(&output[..n])?;
                    stdout.flush()?;
                }
                Err(ref err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err),
            }
        }

        if stream_revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            return Ok(InteractiveMonitorExit::PrimaryClosed);
        }

        let stdin_revents = fds[0].revents;
        if stdin_revents & libc::POLLIN != 0 {
            match stdin.read(&mut input) {
                Ok(0) => return Ok(InteractiveMonitorExit::Detached),
                Ok(n) => {
                    let start = 0;
                    for (index, byte) in input[..n].iter().enumerate() {
                        if *byte == 0x1d {
                            if start < index {
                                stream.write_all(&input[start..index])?;
                                stream.flush()?;
                            }
                            return Ok(InteractiveMonitorExit::Detached);
                        }
                    }

                    stream.write_all(&input[..n])?;
                    stream.flush()?;
                }
                Err(ref err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err),
            }
        }
    }
}

fn stream_to_stdout(stream: &mut UnixStream) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let mut buf = [0_u8; 8192];

    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                stdout.write_all(&buf[..n])?;
                stdout.flush()?;
            }
            Err(ref err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }

    Ok(())
}

fn run_session(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = open_serial(&config)?;
    let reader = writer.try_clone()?;

    eprintln!("connected: {} @ {} baud", config.device, config.baud);
    eprintln!("log: {}", config.log_path);
    eprintln!("monitor: hush monitor {}", config.monitor_name);
    eprintln!("monitor socket: {}", config.monitor_socket_path);
    eprintln!("empty Enter waits for '$' and pauses; Ctrl-T q quits");

    let stop = Arc::new(AtomicBool::new(false));
    let monitor = Arc::new(MonitorHub::default());
    let screen = Screen::new(monitor.clone());
    let (input_tx, input_rx) = mpsc::channel();
    let monitor_handle = spawn_monitor_acceptor(
        &config.monitor_socket_path,
        stop.clone(),
        monitor,
        input_tx.clone(),
    )?;

    let _tty = TtyGuard::enter_raw()?;
    spawn_stdin_reader(input_tx);

    let output_state = Arc::new(Mutex::new(OutputState::default()));

    let reader_handle = spawn_reader(
        reader,
        stop.clone(),
        output_state.clone(),
        screen.clone(),
        config.log_path.clone(),
    )?;

    let mut input = Vec::with_capacity(INPUT_MAX);
    let mut prefix = false;

    while let Ok(ch) = input_rx.recv() {
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
    let _ = monitor_handle.join();
    let _ = fs::remove_file(&config.monitor_socket_path);
    eprintln!("\nbye");
    eprintln!("log: {}", config.log_path);
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let command = match parse_args() {
        Ok(command) => command,
        Err(code) => std::process::exit(code),
    };

    match command {
        Command::Session(config) => run_session(config),
        Command::Monitor(config) => Ok(run_monitor(config)?),
        Command::Sessions => Ok(run_sessions()?),
    }
}
