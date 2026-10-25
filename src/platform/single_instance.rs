//! One resident instance per user. A second launch asks the running one to show its window
//! (and exits); `--quit` asks it to exit. Commands travel over a loopback socket.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

const PORT: u16 = 47613;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Show,
    Quit,
}

fn addr() -> SocketAddr {
    (Ipv4Addr::LOCALHOST, PORT).into()
}

/// Sends `command` to the running instance. False when none is running.
pub fn send(command: Command) -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&addr(), Duration::from_millis(500)) else {
        return false;
    };
    let word: &[u8] = match command {
        Command::Show => b"show",
        Command::Quit => b"quit",
    };
    s.write_all(word).is_ok()
}

/// Becomes the resident instance. `None` means another one already owns the socket.
pub fn claim() -> Option<Receiver<Command>> {
    let listener = TcpListener::bind(addr()).ok()?;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = String::new();
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let _ = stream.read_to_string(&mut buf);
            let command = match buf.trim() {
                "show" => Command::Show,
                "quit" => Command::Quit,
                _ => continue,
            };
            if tx.send(command).is_err() {
                break;
            }
        }
    });
    Some(rx)
}
