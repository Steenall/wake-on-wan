use wake_on_wan_server::ThreadPool;
use wake_on_wan_server::read_csv_file;
use core::time;
use std::fs;
use std::io::prelude::*;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use wake_on_wan_server::Computer;

fn main() {
    let res = read_csv_file("computer_to_wake.csv");
	if res.is_err() {
        println!("Impossible to read computer_to_wake.csv: {:?}", res.err());
        return;
    }

    let list : Vec<Computer> = res.unwrap();
    let arc_list = Arc::new(Mutex::new(list));

    let listener = TcpListener::bind("0.0.0.0:44844").unwrap();

    let pool = ThreadPool::new(4);

    for stream in listener.incoming() {

        let temp_list = arc_list.clone();

        match stream {
            Ok(s) => {
                pool.execute(move || {
                    handle_connection(s,temp_list);
                });
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
        thread::sleep(time::Duration::from_millis(10));
    }

    drop(listener);
    println!("Shutting down.");
}

fn handle_connection(mut stream: TcpStream, computers : Arc<Mutex<Vec<Computer>>>) {
    let mut buffer = [0; 1024];
    stream.read(&mut buffer).unwrap();

    let mut success = true;
    
    for i in computers.lock().unwrap().iter() {
        let res = wake_on_wan_server::send_wake_on_lan_signal(i.to_owned(), SocketAddr::new(Ipv4Addr::new(0, 0, 0, 0).into(), 0));
        if res.is_ok() {
            println!("Wake on lan signal sent");
        }else {
            println!("Impossible to send wake on lan signal: {:?}", res.err());
            success = false;
        }
    }

    let get = b"GET / HTTP/1.1\r\n";

    let (status_line, filename) = if buffer.starts_with(get) {

       if success {
            ("HTTP/1.1 200 OK", "success.txt")
        } else {
            ("HTTP/1.1 503", "error.txt")
        }
    } else {
        ("HTTP/1.1 404", "error.txt")
    };

    let contents = fs::read_to_string(filename).unwrap();

    let response = format!(
        "{}\r\nContent-Length: {}\r\n\r\n{}",
        status_line,
        contents.len(),
        contents
    );

    stream.write(response.as_bytes()).unwrap();
    stream.flush().unwrap();
}
