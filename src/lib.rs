use std::error::Error;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::str::FromStr;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use log::debug;
use serde::Deserialize;

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: mpsc::Sender<Message>,
}

const MAGIC_BYTES_HEADER: [u8; 6] = [0xFF; 6];

#[derive(Clone)]
pub struct Computer {
    pub name: String,
    mac: [u8; 6],
    pub ip: Ipv4Addr,
    pub port: u16,
}

#[derive(Deserialize)]
struct ComputerDeserialized {
    name: String,
    mac: String,
    ip: Option<String>,
    port: String,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

enum Message {
    NewJob(Job),
    Terminate,
}

impl ThreadPool {
    /// Create a new ThreadPool.
    ///
    /// The size is the number of threads in the pool.
    ///
    /// # Panics
    ///
    /// The `new` function will panic if the size is zero.
    pub fn new(size: usize) -> ThreadPool {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            workers.push(Worker::new(id, Arc::clone(&receiver)));
        }

        ThreadPool { workers, sender }
    }

    pub fn execute<F>(&self, f: F) -> Result<(), Box<dyn Error + Send + Sync>>
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        self.sender.send(Message::NewJob(job)).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "thread pool is shutting down",
            )
        })?;
        Ok(())
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        debug!("Sending terminate message to all workers.");

        for _ in &self.workers {
            if self.sender.send(Message::Terminate).is_err() {
                break;
            }
        }

        debug!("Shutting down all workers.");

        for worker in &mut self.workers {
            debug!("Shutting down worker {}", worker.id);

            if let Some(thread) = worker.thread.take() {
                thread
                    .join()
                    .unwrap_or_else(|_| panic!("worker {} panicked", worker.id));
            }
        }
    }
}

struct Worker {
    id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Message>>>) -> Worker {
        let thread = thread::spawn(move || {
            loop {
                let message = receiver.lock().unwrap().recv().unwrap();

                match message {
                    Message::NewJob(job) => {
                        job();
                    }
                    Message::Terminate => {
                        debug!("Worker {} was told to terminate.", id);
                        break;
                    }
                }
            }
        });

        Worker {
            id,
            thread: Some(thread),
        }
    }
}

fn parse_mac_address(input: &str) -> Result<[u8; 6], Box<dyn Error>> {
    let normalized = input.replace('-', "").replace(':', "").replace('.', "");

    if normalized.len() != 12 {
        return Err(format!("Invalid MAC address: {input}").into());
    }

    let mut mac = [0u8; 6];
    for (index, bytes) in normalized.as_bytes().chunks_exact(2).enumerate() {
        mac[index] = u8::from_str_radix(std::str::from_utf8(bytes)?, 16)?;
    }

    Ok(mac)
}

pub fn read_csv_file(file_name: &str) -> Result<Vec<Computer>, Box<dyn Error>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .delimiter(b';')
        .from_path(file_name)?;

    let mut computers = Vec::new();

    for result in rdr.deserialize() {
        let record: ComputerDeserialized = result?;
        let name = record.name.trim();
        if name.is_empty() {
            return Err("Computer name must not be empty".into());
        }
        if computers
            .iter()
            .any(|computer: &Computer| computer.name == name)
        {
            return Err(format!("Duplicate computer name: {name}").into());
        }
        let mac = parse_mac_address(&record.mac)?;
        let port = record.port.parse::<u16>()?;
        let ip = match record.ip.as_deref() {
            Some(ip) => Ipv4Addr::from_str(ip)?,
            None => Ipv4Addr::new(255, 255, 255, 255),
        };
        if ip.eq(&Ipv4Addr::new(255, 255, 255, 255)) {
            return Err("Invalid IP address".into());
        }
        computers.push(Computer {
            name: name.to_owned(),
            mac,
            ip,
            port,
        });
    }

    Ok(computers)
}

pub fn send_wake_on_lan_signal(computer: Computer, ip: SocketAddr) -> Result<(), Box<dyn Error>> {
    let socket = UdpSocket::bind(ip)?;
    socket.set_broadcast(true)?;

    let mut current_magic_packet: [u8; 102] = [0; 102];
    current_magic_packet[..6].copy_from_slice(&MAGIC_BYTES_HEADER);
    current_magic_packet[6..102]
        .chunks_mut(6)
        .for_each(|chunk| chunk.copy_from_slice(&computer.mac));

    socket.send_to(&current_magic_packet, (computer.ip, computer.port))?;

    debug!("Wake on lan signal sent to {}", computer.ip);

    Ok(())
}
