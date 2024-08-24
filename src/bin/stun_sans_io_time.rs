use anyhow::Context;
use futures::{Future, FutureExt};
use sans_io_blog_example::{make_binding_request, parse_binding_response};
use std::{
    collections::VecDeque,
    net::{SocketAddr, ToSocketAddrs},
    pin::Pin,
    task::{ready, Poll, Waker},
    time::{Duration, Instant},
};
use tokio::net::UdpSocket;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let server = "stun.cloudflare.com:3478"
        .to_socket_addrs()?
        .find(|addr| addr.is_ipv4())
        .context("Failed to resolve hostname")?;
    let mut binding = StunBinding::new(server);
    let mut timer = Timer::default();

    loop {
        if let Some(transmit) = binding.poll_transmit() {
            socket.send_to(&transmit.payload, transmit.dst).await?;
            continue;
        }

        let mut buf = vec![0u8; 100];

        tokio::select! {
            Some(time) = &mut timer => {
                binding.handle_timeout(time);
            },
            res = socket.recv(&mut buf) => {
                let num_read = res?;
                binding.handle_input(&buf[..num_read], Instant::now());

            }
        }

        timer.reset_to(binding.poll_timeout());

        if let Some(address) = binding.public_address() {
            println!("Our public IP is: {address}");
        }
    }
}

#[derive(Default)]
struct Timer {
    inner: Option<Pin<Box<tokio::time::Sleep>>>,
    waker: Option<Waker>,
}

impl Timer {
    fn reset_to(&mut self, next: Option<Instant>) {
        let next = match next {
            Some(next) => next,
            None => {
                self.inner = None;
                return;
            }
        };

        match self.inner.as_mut() {
            Some(timer) => timer.as_mut().reset(next.into()),
            None => {
                self.inner = Some(Box::pin(tokio::time::sleep_until(next.into())));
                if let Some(waker) = self.waker.take() {
                    waker.wake()
                }
            }
        }
    }
}

impl Future for Timer {
    type Output = Option<Instant>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut this = self.as_mut();

        let Some(timer) = this.inner.as_mut() else {
            self.waker = Some(cx.waker().clone());
            return Poll::Ready(None);
        };

        ready!(timer.as_mut().poll_unpin(cx));

        Poll::Ready(Some(timer.as_ref().deadline().into()))
    }
}

struct StunBinding {
    server: SocketAddr,
    state: State,
    buffered_transmits: VecDeque<Transmit>,
}

impl StunBinding {
    fn new(server: SocketAddr) -> Self {
        Self {
            server,
            state: State::Sent,
            buffered_transmits: VecDeque::from([Transmit {
                dst: server,
                payload: make_binding_request(),
            }]),
        }
    }

    fn handle_input(&mut self, packet: &[u8], now: Instant) {
        let address = parse_binding_response(packet);

        self.state = State::Received { address, at: now };
    }

    fn poll_transmit(&mut self) -> Option<Transmit> {
        self.buffered_transmits.pop_front()
    }

    /// Notifies `StunBinding` that time has advanced to `now`.
    fn handle_timeout(&mut self, now: Instant) {
        let last_received_at = match self.state {
            State::Sent => return,
            State::Received { at, .. } => at,
        };

        if now.duration_since(last_received_at) < Duration::from_secs(5) {
            return;
        }

        self.buffered_transmits.push_front(Transmit {
            dst: self.server,
            payload: make_binding_request(),
        });
        self.state = State::Sent;
    }

    /// Returns the timestamp when we next expect `handle_timeout` to be called.
    fn poll_timeout(&self) -> Option<Instant> {
        match self.state {
            State::Sent => None,
            State::Received { at, .. } => Some(at + Duration::from_secs(5)),
        }
    }

    fn public_address(&self) -> Option<SocketAddr> {
        match self.state {
            State::Sent => None,
            State::Received { address, .. } => Some(address),
        }
    }
}

enum State {
    Sent,
    Received { address: SocketAddr, at: Instant },
}

struct Transmit {
    dst: SocketAddr,
    payload: Vec<u8>,
}
