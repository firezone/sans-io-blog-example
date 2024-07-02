use sans_io_blog_example::{make_binding_request, parse_binding_response};
use tokio::net::UdpSocket;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect("stun.cloudflare.com:3478").await?;
    socket.send(&make_binding_request()).await?;

    let mut buf = vec![0u8; 100];
    let num_read = socket.recv(&mut buf).await?;
    let address = parse_binding_response(&buf[..num_read]);

    println!("Our public IP is: {address}");

    Ok(())
}
