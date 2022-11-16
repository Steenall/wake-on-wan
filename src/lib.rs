use std::net::SocketAddr;
use std::net::UdpSocket;
use std::str::FromStr;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::error::Error;
use std::thread;
use serde::Deserialize;
use std::net::Ipv4Addr;

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: mpsc::Sender<Message>,
}

const MAGIC_BYTES_HEADER: [u8; 6] = [0xFF; 6];

#[derive(Debug, Clone)]
pub struct Computer {
	mac: [u8; 6],
	pub ip: Ipv4Addr,
	pub port: u16,
}

#[derive(Debug, Deserialize)]
struct ComputerDeserialized {
	mac: String,
	ip: Option<String>,
	port: String
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

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);

        self.sender.send(Message::NewJob(job)).unwrap();
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        println!("Sending terminate message to all workers.");

        for _ in &self.workers {
            self.sender.send(Message::Terminate).unwrap();
        }

        println!("Shutting down all workers.");

        for worker in &mut self.workers {
            println!("Shutting down worker {}", worker.id);

            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
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
        let thread = thread::spawn(move || loop {
            let message = receiver.lock().unwrap().recv().unwrap();

            match message {
                Message::NewJob(job) => {
                    println!("Worker {} got a job; executing.", id);

                    job();
                }
                Message::Terminate => {
                    println!("Worker {} was told to terminate.", id);

                    break;
                }
            }
        });

        Worker {
            id,
            thread: Some(thread),
        }
    }
}

pub fn read_csv_file(file_name : &str) -> Result<Vec<Computer>, Box<dyn Error>>  {
    let mut rdr = csv::ReaderBuilder::new().has_headers(true).delimiter(b';').from_path(file_name)?;
    let mut vec = Vec::new();
    for result in rdr.deserialize() {

        let record: ComputerDeserialized = result?;

        let mac_result = record.mac.split("-").map(|x| u8::from_str_radix(x, 16).unwrap()).collect::<Vec<u8>>().try_into();
        let mac;
        if mac_result.is_ok() {
            mac = mac_result.unwrap();
        }else {
            return Err(("Invalid MAC address").into());
        }
        let port = u16::from_str_radix(&record.port, 10).unwrap();
        let ip;
        if record.ip.is_some() {
            ip = Ipv4Addr::from_str(&record.ip.unwrap()).unwrap();
        }else {
            ip = Ipv4Addr::new(255, 255, 255, 255);
        }

        let computer = Computer {
            mac,
            ip,
            port
        };
        vec.push(computer);
    }
    #[cfg(build = "debug")]
    println!("{:?}", vec);
    Ok(vec)
}

pub fn send_wake_on_lan_signal(computer : Computer, ip : SocketAddr) -> Result<(), Box<dyn Error>> {
    let socket = UdpSocket::bind(ip)?;
    socket.set_broadcast(true)?;
    let mut current_magic_packet: [u8; 102 ] = [0; 102];
    //We repeat 6 times the magic bytes
    current_magic_packet[..6].copy_from_slice(&MAGIC_BYTES_HEADER);
    //We repeat 16 times the mac adress
    current_magic_packet[6..102].chunks_mut(6).for_each(|chunk| chunk.copy_from_slice(&computer.mac));

    socket.send_to(&current_magic_packet, (computer.ip, computer.port))?;

    #[cfg(build = "debug")]
    print!("Wake on lan signal sent to {} with content {:?}", computer.ip, current_magic_packet);

    Ok(())
}