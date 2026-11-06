//! Transport layer for the launcher's host-authoritative multiplayer.
//!
//! A [`Transport`] moves opaque byte messages between the host and its peers.
//! The TCP implementation here works on a LAN or over the internet by IP; the
//! launcher adds a Steam implementation on top of the same trait.
//!
//! Every connection gets a background reader thread that pushes events into a
//! channel, so [`Transport::poll`] never blocks and can be called once per frame.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Identifies a remote endpoint. On a client the host is always [`HOST_PEER`].
pub type PeerId = u64;
pub const HOST_PEER: PeerId = 0;
pub const DEFAULT_PORT: u16 = 27_015;
/// Upper bound for one message; anything larger is treated as a broken stream.
pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetEvent {
    Connected(PeerId),
    Disconnected(PeerId),
    Message(PeerId, Vec<u8>),
}

pub trait Transport: Send {
    /// Drains everything that arrived since the last call.
    fn poll(&mut self) -> Vec<NetEvent>;
    fn send(&mut self, peer: PeerId, data: &[u8]);
    fn disconnect(&mut self, peer: PeerId);
    /// Human-readable address others can use to join (host only).
    fn join_hint(&self) -> Option<String> {
        None
    }
}

fn write_frame(stream: &mut TcpStream, data: &[u8]) -> io::Result<()> {
    let len = u32::try_from(data.len()).map_err(|_| io::Error::other("message too large"))?;
    let mut buf = Vec::with_capacity(4 + data.len());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(data);
    stream.write_all(&buf)
}

fn read_frame(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut data = vec![0u8; len];
    stream.read_exact(&mut data)?;
    Ok(data)
}

fn spawn_reader(mut stream: TcpStream, peer: PeerId, events: Sender<NetEvent>) {
    thread::spawn(move || {
        loop {
            match read_frame(&mut stream) {
                Ok(data) => {
                    if events.send(NetEvent::Message(peer, data)).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = events.send(NetEvent::Disconnected(peer));
    });
}

/// Accepts clients and talks to all of them.
pub struct TcpHost {
    events: Receiver<NetEvent>,
    peers: Arc<Mutex<HashMap<PeerId, TcpStream>>>,
    local_addr: SocketAddr,
}

impl TcpHost {
    pub fn bind(port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        let local_addr = listener.local_addr()?;
        let (tx, rx) = channel();
        let peers: Arc<Mutex<HashMap<PeerId, TcpStream>>> = Arc::default();
        let accept_peers = peers.clone();
        thread::spawn(move || {
            let mut next_id: PeerId = 1;
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let _ = stream.set_nodelay(true);
                let Ok(reader) = stream.try_clone() else { continue };
                let id = next_id;
                next_id += 1;
                accept_peers.lock().unwrap().insert(id, stream);
                if tx.send(NetEvent::Connected(id)).is_err() {
                    break;
                }
                spawn_reader(reader, id, tx.clone());
            }
        });
        Ok(TcpHost {
            events: rx,
            peers,
            local_addr,
        })
    }

    pub fn port(&self) -> u16 {
        self.local_addr.port()
    }
}

impl Transport for TcpHost {
    fn poll(&mut self) -> Vec<NetEvent> {
        let events: Vec<NetEvent> = self.events.try_iter().collect();
        for e in &events {
            if let NetEvent::Disconnected(id) = e {
                self.peers.lock().unwrap().remove(id);
            }
        }
        events
    }

    fn send(&mut self, peer: PeerId, data: &[u8]) {
        let mut peers = self.peers.lock().unwrap();
        let failed = match peers.get_mut(&peer) {
            Some(stream) => write_frame(stream, data).is_err(),
            None => false,
        };
        if failed {
            if let Some(s) = peers.remove(&peer) {
                let _ = s.shutdown(Shutdown::Both);
            }
        }
    }

    fn disconnect(&mut self, peer: PeerId) {
        if let Some(s) = self.peers.lock().unwrap().remove(&peer) {
            let _ = s.shutdown(Shutdown::Both);
        }
    }

    fn join_hint(&self) -> Option<String> {
        let ip = local_ip().unwrap_or_else(|| "127.0.0.1".into());
        Some(format!("{ip}:{}", self.port()))
    }
}

impl Drop for TcpHost {
    fn drop(&mut self) {
        for (_, s) in self.peers.lock().unwrap().drain() {
            let _ = s.shutdown(Shutdown::Both);
        }
        // Wake the accept loop so its thread can notice the channel is gone.
        let _ = TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], self.local_addr.port())),
            Duration::from_millis(200),
        );
    }
}

/// A connection to a host.
pub struct TcpClient {
    events: Receiver<NetEvent>,
    stream: Option<TcpStream>,
}

impl TcpClient {
    /// Connects to `addr` (`host:port`; the default port is used if omitted).
    pub fn connect(addr: &str) -> io::Result<Self> {
        let addr = if addr.contains(':') {
            addr.to_string()
        } else {
            format!("{addr}:{DEFAULT_PORT}")
        };
        let target = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "адрес не найден"))?;
        let stream = TcpStream::connect_timeout(&target, Duration::from_secs(5))?;
        stream.set_nodelay(true)?;
        let reader = stream.try_clone()?;
        let (tx, rx) = channel();
        tx.send(NetEvent::Connected(HOST_PEER)).ok();
        spawn_reader(reader, HOST_PEER, tx);
        Ok(TcpClient {
            events: rx,
            stream: Some(stream),
        })
    }
}

impl Transport for TcpClient {
    fn poll(&mut self) -> Vec<NetEvent> {
        self.events.try_iter().collect()
    }

    fn send(&mut self, _peer: PeerId, data: &[u8]) {
        if let Some(stream) = &mut self.stream {
            if write_frame(stream, data).is_err() {
                let _ = stream.shutdown(Shutdown::Both);
                self.stream = None;
            }
        }
    }

    fn disconnect(&mut self, _peer: PeerId) {
        if let Some(s) = self.stream.take() {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for TcpClient {
    fn drop(&mut self) {
        self.disconnect(HOST_PEER);
    }
}

/// Best guess at this machine's LAN address (no packets are sent).
pub fn local_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn wait_for<T: Transport>(t: &mut T, pred: impl Fn(&NetEvent) -> bool) -> NetEvent {
        let start = Instant::now();
        loop {
            for e in t.poll() {
                if pred(&e) {
                    return e;
                }
            }
            assert!(start.elapsed() < Duration::from_secs(5), "timed out");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn host_and_client_exchange_messages() {
        let mut host = TcpHost::bind(0).unwrap();
        let mut client = TcpClient::connect(&format!("127.0.0.1:{}", host.port())).unwrap();
        let NetEvent::Connected(peer) = wait_for(&mut host, |e| matches!(e, NetEvent::Connected(_))) else {
            unreachable!()
        };
        client.send(HOST_PEER, b"hello");
        let e = wait_for(&mut host, |e| matches!(e, NetEvent::Message(..)));
        assert_eq!(e, NetEvent::Message(peer, b"hello".to_vec()));

        let big = vec![7u8; 300_000];
        host.send(peer, &big);
        let e = wait_for(&mut client, |e| matches!(e, NetEvent::Message(..)));
        assert_eq!(e, NetEvent::Message(HOST_PEER, big));

        drop(client);
        let e = wait_for(&mut host, |e| matches!(e, NetEvent::Disconnected(_)));
        assert_eq!(e, NetEvent::Disconnected(peer));
    }
}
