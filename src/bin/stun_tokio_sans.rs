use std::{
    collections::VecDeque,
    net::{Ipv4Addr, SocketAddr, ToSocketAddrs},
};

use anyhow::{anyhow, Context};
use bytecodec::{DecodeExt, EncodeExt};
use bytes::{BufMut, BytesMut};
use futures::{Sink, SinkExt, Stream, StreamExt};
use stun_codec::{
    rfc5389::{attributes::XorMappedAddress, methods::BINDING, Attribute},
    Message, MessageClass, MessageDecoder, MessageEncoder, TransactionId,
};
use tokio::net::UdpSocket;
use tokio_util::{
    codec::{Decoder, Encoder},
    udp::UdpFramed,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    let server = "stun.cloudflare.com:3478"
        .to_socket_addrs()?
        .find(|addr| addr.is_ipv4())
        .context("Failed to resolve hostname")?;

    let (sink, stream) = UdpFramed::new(socket, StunCodec).split();

    let mut binding = StunBinding::new(server, sink, stream);
    let address = binding.public_address().await?.unwrap();
    println!("Our public IP is: {address}");

    Ok(())
}

type BindingRequest = Message<Attribute>;
type BindingResponse = Message<Attribute>;

struct StunBinding<Req, Res>
where
    Req: Sink<(BindingRequest, SocketAddr)> + Unpin,
    Res: Stream<Item = Result<(BindingResponse, SocketAddr), anyhow::Error>>,
{
    requests: VecDeque<Request>,
    sink: Req,
    stream: Res,
}

impl<Req, Res> StunBinding<Req, Res>
where
    Req: Sink<(BindingRequest, SocketAddr), Error = anyhow::Error> + Unpin,
    Res: Stream<Item = Result<(BindingResponse, SocketAddr), anyhow::Error>> + Unpin,
{
    fn new(server: SocketAddr, sink: Req, stream: Res) -> Self {
        Self {
            requests: VecDeque::from([Request {
                dst: server,
                payload: make_binding_request(),
            }]),
            sink,
            stream,
        }
    }

    async fn public_address(&mut self) -> anyhow::Result<Option<SocketAddr>> {
        loop {
            if let Some(transmit) = self.requests.pop_front() {
                self.sink.send((transmit.payload, transmit.dst)).await?;
                continue;
            }

            if let Some(address) = self.stream.next().await {
                let (message, _) = address?;
                break Ok(parse_binding_response(message));
            }
        }
    }
}

struct StunCodec;

impl Encoder<BindingRequest> for StunCodec {
    type Error = anyhow::Error;

    fn encode(&mut self, item: BindingRequest, buf: &mut BytesMut) -> anyhow::Result<()> {
        let bytes = MessageEncoder::<Attribute>::default()
            .encode_into_bytes(item)
            .context("Failed to encode message")?;

        buf.reserve(bytes.len());
        buf.put_slice(&bytes);

        Ok(())
    }
}

impl Decoder for StunCodec {
    type Item = BindingResponse;
    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut BytesMut) -> anyhow::Result<Option<BindingResponse>> {
        let message = MessageDecoder::<Attribute>::default()
            .decode_from_bytes(src)
            .context("Failed to decode message")?
            .map_err(|e| anyhow!("incomplete message {e:?}"))?;
        Ok(Some(message))
    }
}

struct Request {
    dst: SocketAddr,
    payload: BindingRequest,
}

/// note that this just handles message -> attribute conversion, not the bytes -> attribute
/// conversion that the original code does.
fn parse_binding_response(response: BindingResponse) -> Option<SocketAddr> {
    response
        .get_attribute::<XorMappedAddress>()
        .map(XorMappedAddress::address)
}

/// note that this just handles the message creation, not the message -> bytes conversion that the
/// original code does.
fn make_binding_request() -> BindingRequest {
    Message::<Attribute>::new(
        MessageClass::Request,
        BINDING,
        TransactionId::new(rand::random()),
    )
}

#[cfg(test)]
mod tests {

    use anyhow::anyhow;
    use futures::stream;
    use stun_codec::rfc5389;

    use super::*;

    #[tokio::test]
    async fn public_address() {
        let server = ([1, 1, 1, 1], 3478).into();
        let expected_address = ([2, 2, 2, 2], 1234).into();
        let mut response = BindingResponse::new(
            MessageClass::SuccessResponse,
            rfc5389::methods::BINDING,
            TransactionId::new([0; 12]),
        );
        response.add_attribute(XorMappedAddress::new(expected_address));

        // No io here, just a couple of vecs with the input / output
        let sink = Vec::new().sink_map_err(|_| anyhow!("sink error"));
        let stream = stream::iter([Ok((response, server))]);
        let mut binding = StunBinding::new(server, sink, stream);

        let address = binding.public_address().await.unwrap().unwrap();

        assert_eq!(address, expected_address);
    }
}
