use anyhow::Context;
use sans_io_blog_example::{make_binding_request, parse_binding_response};
use std::{
    collections::VecDeque,
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
};

fn main() -> anyhow::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let server = "stun.cloudflare.com:3478"
        .to_socket_addrs()?
        .find(|addr| addr.is_ipv4())
        .context("Failed to resolve hostname")?;
    let mut binding = StunBinding::new(server);

    let address = loop {
        if let Some(transmit) = binding.poll_transmit() {
            socket.send_to(&transmit.payload, transmit.dst)?;
            continue;
        }

        let mut buf = vec![0u8; 100];
        let num_read = socket.recv(&mut buf)?;

        binding.handle_input(&buf[..num_read]);

        if let Some(address) = binding.public_address() {
            break address;
        }
    };

    println!("Our public IP is: {address}");

    Ok(())
}

struct StunBinding {
    state: State,
    buffered_transmits: VecDeque<Transmit>,
}

impl StunBinding {
    fn new(server: SocketAddr) -> Self {
        Self {
            state: State::Sent,
            buffered_transmits: VecDeque::from([Transmit {
                dst: server,
                payload: make_binding_request(),
            }]),
        }
    }

    fn handle_input(&mut self, packet: &[u8]) {
        let address = parse_binding_response(packet);

        self.state = State::Received { address };
    }

    fn poll_transmit(&mut self) -> Option<Transmit> {
        self.buffered_transmits.pop_front()
    }

    fn public_address(&self) -> Option<SocketAddr> {
        match self.state {
            State::Sent => None,
            State::Received { address } => Some(address),
        }
    }
}

enum State {
    Sent,
    Received { address: SocketAddr },
}

struct Transmit {
    dst: SocketAddr,
    payload: Vec<u8>,
}
