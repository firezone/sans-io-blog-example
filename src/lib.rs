use bytecodec::{DecodeExt, EncodeExt as _};
use std::net::SocketAddr;
use stun_codec::rfc5389::{attributes::XorMappedAddress, methods::BINDING, Attribute};
use stun_codec::*;

pub fn make_binding_request() -> Vec<u8> {
    let request = Message::<Attribute>::new(
        MessageClass::Request,
        BINDING,
        TransactionId::new(rand::random()),
    );

    MessageEncoder::<Attribute>::default()
        .encode_into_bytes(request)
        .unwrap()
}

pub fn parse_binding_response(buf: &[u8]) -> SocketAddr {
    let message = MessageDecoder::<Attribute>::default()
        .decode_from_bytes(buf)
        .unwrap()
        .unwrap();

    message
        .get_attribute::<XorMappedAddress>()
        .unwrap()
        .address()
}
