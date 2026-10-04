use core::time;
use std::collections::HashMap;
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{self, prelude::*};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use log::{Level, LevelFilter, debug, error, info};
use wake_on_wan_server::{Computer, ThreadPool, read_csv_file};

#[derive(Clone, Copy)]
struct Config {
    max_requests_per_ip_per_second: usize,
    max_request_duration: Duration,
    tcp_listener_port: u16,
}

struct RequestWindow {
    started_at: Instant,
    count: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let config = read_config("config.properties")?;
    fs::create_dir_all("log")?;
    let formatter =
        |out: fern::FormatCallback, message: &std::fmt::Arguments, record: &log::Record| {
            out.finish(format_args!(
                "{} [{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f %:z"),
                record.level(),
                message
            ))
        };
    let console = fern::Dispatch::new()
        .format(formatter)
        .level(LevelFilter::Debug)
        .chain(io::stdout());
    let debug_log = fern::Dispatch::new()
        .format(formatter)
        .filter(|metadata| metadata.level() == Level::Debug)
        .chain(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open("log/debug.log")?,
        );
    let error_log = fern::Dispatch::new()
        .format(formatter)
        .filter(|metadata| metadata.level() == Level::Error)
        .chain(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open("log/error.log")?,
        );

    let info_log = fern::Dispatch::new()
        .format(formatter)
        .filter(|metadata| metadata.level() == Level::Info)
        .chain(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open("log/requests.log")?,
        );

    fern::Dispatch::new()
        .level(LevelFilter::Debug)
        .chain(console)
        .chain(debug_log)
        .chain(error_log)
        .chain(info_log)
        .apply()?;

    let list = read_csv_file("computer_to_wake.csv")?;
    let arc_list = Arc::new(Mutex::new(list));
    let rate_limit = Arc::new(Mutex::new(HashMap::<IpAddr, RequestWindow>::new()));

    let listener_result = TcpListener::bind(("0.0.0.0", config.tcp_listener_port));

    if listener_result.is_err() {
        return Err(listener_result.err().unwrap().into());
    }

    let listener = listener_result?;
    debug!("Server is running on port {}", config.tcp_listener_port);
    let pool = ThreadPool::new(4);

    for stream in listener.incoming() {
        let temp_list = arc_list.clone();
        let temp_rate_limit = Arc::clone(&rate_limit);

        match stream {
            Ok(s) => {
                if let Err(error) = pool.execute(move || {
                    handle_connection(s, temp_list, temp_rate_limit, config);
                }) {
                    error!("Error scheduling job: {error}");
                }
            }
            Err(e) => {
                error!("Error: {e}");
            }
        }

        thread::sleep(time::Duration::from_millis(10));
    }

    debug!("Shutting down.");
    Ok(())
}

fn read_config(file_name: &str) -> Result<Config, Box<dyn Error>> {
    let contents = fs::read_to_string(file_name)?;
    let mut max_requests_per_ip_per_second = None;
    let mut max_request_duration_ms = None;
    let mut tcp_listener_port = None;

    for (line_number, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (key, value) = line.split_once('=').ok_or_else(|| {
            format!(
                "Invalid config entry on line {}: expected key=value",
                line_number + 1
            )
        })?;
        let key = key.trim();
        let value = value.trim();

        match key {
            "max_requests_per_ip_per_second" => {
                max_requests_per_ip_per_second = Some(value.parse::<usize>().map_err(|error| {
                    format!("Invalid {key} value on line {}: {error}", line_number + 1)
                })?);
            }
            "max_request_duration_ms" => {
                max_request_duration_ms = Some(value.parse::<u64>().map_err(|error| {
                    format!("Invalid {key} value on line {}: {error}", line_number + 1)
                })?);
            }
            "tcp_listener_port" => {
                tcp_listener_port = Some(value.parse::<u16>().map_err(|error| {
                    format!("Invalid {key} value on line {}: {error}", line_number + 1)
                })?);
            }
            _ => {
                return Err(
                    format!("Unknown config key on line {}: {key}", line_number + 1).into(),
                );
            }
        }
    }

    let max_requests_per_ip_per_second = max_requests_per_ip_per_second
        .ok_or("Missing max_requests_per_ip_per_second in config.properties")?;
    let max_request_duration_ms =
        max_request_duration_ms.ok_or("Missing max_request_duration_ms in config.properties")?;
    let tcp_listener_port =
        tcp_listener_port.ok_or("Missing tcp_listener_port in config.properties")?;

    if max_requests_per_ip_per_second == 0 || max_request_duration_ms == 0 || tcp_listener_port == 0
    {
        return Err("Config values must be greater than zero".into());
    }

    let max_request_duration = Duration::from_millis(max_request_duration_ms);
    if Instant::now().checked_add(max_request_duration).is_none() {
        return Err("max_request_duration_ms is too large".into());
    }

    Ok(Config {
        max_requests_per_ip_per_second,
        max_request_duration,
        tcp_listener_port,
    })
}

fn handle_connection(
    mut stream: TcpStream,
    computers: Arc<Mutex<Vec<Computer>>>,
    rate_limit: Arc<Mutex<HashMap<IpAddr, RequestWindow>>>,
    config: Config,
) {
    let mut request = Vec::new();
    let mut chunk = [0; 1024];
    let deadline = Instant::now() + config.max_request_duration;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            write_response(&mut stream, "HTTP/1.1 408 Request Timeout", "error.html");
            return;
        }
        if let Err(error) = stream.set_read_timeout(Some(remaining)) {
            error!("Failed to set request read timeout: {error}");
            return;
        }

        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                request.extend_from_slice(&chunk[..n]);

                if request.windows(4).any(|window| window == b"\r\n\r\n") || request.len() >= 1024 {
                    break;
                }
            }
            Err(error) => {
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) {
                    write_response(&mut stream, "HTTP/1.1 408 Request Timeout", "error.html");
                    return;
                }
                error!("Failed to read request: {error}");
                return;
            }
        }
    }

    let request_line = request
        .split(|byte| *byte == b'\n')
        .next()
        .unwrap_or_default();
    let request_line = String::from_utf8_lossy(request_line);
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();

    let client_ip = match stream.peer_addr() {
        Ok(address) => address.ip(),
        Err(error) => {
            error!("Failed to get client address: {error}");
            return;
        }
    };
    info!("request from {client_ip}: {}", request_line.trim());
    let now = Instant::now();
    let allowed = {
        let mut requests = rate_limit.lock().unwrap();
        requests
            .retain(|_, window| now.duration_since(window.started_at) < Duration::from_secs(60));

        match requests.get_mut(&client_ip) {
            Some(window) if now.duration_since(window.started_at) < Duration::from_secs(1) => {
                if window.count >= config.max_requests_per_ip_per_second {
                    false
                } else {
                    window.count += 1;
                    true
                }
            }
            Some(window) => {
                window.started_at = now;
                window.count = 1;
                true
            }
            None => {
                requests.insert(
                    client_ip,
                    RequestWindow {
                        started_at: now,
                        count: 1,
                    },
                );
                true
            }
        }
    };

    if !allowed {
        write_response(&mut stream, "HTTP/1.1 429 Too Many Requests", "error.html");
        return;
    }

    let configured_computers = computers.lock().unwrap().clone();
    let (mut status_line, targets) = match (method, path) {
        ("GET", path) if path.starts_with('/') && path.len() > 1 => {
            match configured_computers
                .iter()
                .find(|computer| computer.name == path[1..])
            {
                Some(computer) => ("HTTP/1.1 200 OK", vec![computer.clone()]),
                None => ("HTTP/1.1 404 Not Found", Vec::new()),
            }
        }
        ("GET" | "POST", _) => ("HTTP/1.1 404 Not Found", Vec::new()),
        _ => ("HTTP/1.1 405 Method Not Allowed", Vec::new()),
    };

    if status_line == "HTTP/1.1 200 OK" {
        for computer in &targets {
            if let Err(error) = wake_on_wan_server::send_wake_on_lan_signal(
                computer.clone(),
                SocketAddr::new(Ipv4Addr::new(0, 0, 0, 0).into(), 0),
            ) {
                error!("Impossible to send wake on lan signal: {error}");
                status_line = "HTTP/1.1 503 Service Unavailable";
            } else {
                debug!("Wake on lan signal sent");
            }
        }
    }

    let filename = if status_line == "HTTP/1.1 200 OK" {
        "success.html"
    } else {
        "error.html"
    };

    write_response(&mut stream, status_line, filename);
}

fn write_response(stream: &mut TcpStream, status_line: &str, filename: &str) {
    let contents = match fs::read_to_string(filename) {
        Ok(file) => file,
        Err(error) => {
            error!("Failed to read response file {filename}: {error}");
            return;
        }
    };

    let response = format!(
        "{}\r\nContent-Length: {}\r\n\r\n{}",
        status_line,
        contents.len(),
        contents
    );

    if let Err(error) = stream.write_all(response.as_bytes()) {
        error!("Failed to write response: {error}");
        return;
    }

    if let Err(error) = stream.flush() {
        error!("Failed to flush response: {error}");
    }
}
