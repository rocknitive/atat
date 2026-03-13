use atat_examples::common;

use atat::{
    AtDigester, AtatIngress, Config, Ingress, ResponseSlot, UrcChannel,
    asynch::{AtatClient, Client},
    digest::ParseError,
};
use embedded_io_adapters::tokio_1::FromTokio;
use static_cell::StaticCell;
use std::process::exit;
use tokio::io::{AsyncReadExt, DuplexStream};

const INGRESS_BUF_SIZE: usize = 1024;
const URC_CAPACITY: usize = 128;
const URC_SUBSCRIBERS: usize = 1;

static INGRESS_BUF: StaticCell<[u8; INGRESS_BUF_SIZE]> = StaticCell::new();
static RES_SLOT: ResponseSlot<INGRESS_BUF_SIZE> = ResponseSlot::new();
static URC_CHANNEL: UrcChannel<common::Urc, URC_CAPACITY, URC_SUBSCRIBERS> = UrcChannel::new();

// Responses: Trigger
const RESPONSE_MANUFACTURER: &str = "\r\nQuectel\r\n\r\nOK\r\n";
const RESPONSE_PROMPT: &str = "\r\n> ";
const RESPONSE_SEND_OK: &str = "\r\nSEND OK\r\n";

fn parse_send_ok(buf: &[u8]) -> Result<(&[u8], usize), ParseError> {
    if buf.starts_with(RESPONSE_SEND_OK.as_bytes()) {
        Ok((&[], RESPONSE_SEND_OK.len()))
    } else if RESPONSE_SEND_OK.as_bytes().starts_with(buf) {
        Err(ParseError::Incomplete)
    } else {
        Err(ParseError::NoMatch)
    }
}

fn parse_send_fail(buf: &[u8]) -> Result<(&[u8], usize), ParseError> {
    const RESPONSE_SEND_FAIL: &str = "\r\nSEND FAIL\r\n";

    if buf.starts_with(RESPONSE_SEND_FAIL.as_bytes()) {
        Ok((b"SEND FAIL", RESPONSE_SEND_FAIL.len()))
    } else if RESPONSE_SEND_FAIL.as_bytes().starts_with(buf) {
        Err(ParseError::Incomplete)
    } else {
        Err(ParseError::NoMatch)
    }
}

#[tokio::main]
async fn main() -> ! {
    env_logger::init();

    let (host, device) = tokio::io::duplex(1024);

    let (host_rx, host_tx) = tokio::io::split(host);
    let (device_rx, device_tx) = tokio::io::split(device);

    let ingress = Ingress::new(
        AtDigester::<common::Urc>::new()
            .with_custom_success(parse_send_ok)
            .with_custom_error(parse_send_fail),
        INGRESS_BUF.init([0; INGRESS_BUF_SIZE]),
        &RES_SLOT,
        &URC_CHANNEL,
    );

    tokio::spawn(ingress_task(ingress, host_rx));
    tokio::spawn(device_task(
        FromTokio::new(device_rx),
        FromTokio::new(device_tx),
    ));

    static BUF: StaticCell<[u8; 1024]> = StaticCell::new();
    let buf = BUF.init([0; 1024]);
    let mut client = Client::new(FromTokio::new(host_tx), &RES_SLOT, buf, Config::default());

    let response = client.send(&common::general::GetManufacturerId).await;

    match response {
        Ok(response) => log::info!("Manufacturer: {:?}", response.id),
        Err(e) => {
            log::error!("Error: {:?}", e);
        }
    }

    let response = client
        .send(&common::general::SendSocketData {
            connect_id: 0,
            data: b"hello",
        })
        .await;

    match response {
        Ok(_) => log::info!("Prompt data send completed"),
        Err(e) => log::error!("Prompt data send failed: {:?}", e),
    }

    exit(0);
}

async fn device_task(
    mut reader: impl embedded_io_async::Read,
    mut writer: impl embedded_io_async::Write,
) -> ! {
    let mut buf = [0; 1024];
    loop {
        let n = reader.read(&mut buf).await.unwrap();
        let received = &buf[..n];
        log::debug!(
            "Received from host: {:?}",
            core::str::from_utf8(received).unwrap()
        );

        if received == b"AT+CGMI\r" {
            writer
                .write_all(RESPONSE_MANUFACTURER.as_bytes())
                .await
                .unwrap();
            writer.flush().await.unwrap();
            continue;
        }

        if received == b"AT+QISEND=0,5\r" {
            writer.write_all(RESPONSE_PROMPT.as_bytes()).await.unwrap();
            writer.flush().await.unwrap();

            let n = reader.read(&mut buf).await.unwrap();
            let payload = &buf[..n];
            log::debug!("Received raw payload: {:?}", payload);
            assert_eq!(payload, b"hello");

            writer.write_all(RESPONSE_SEND_OK.as_bytes()).await.unwrap();
            writer.flush().await.unwrap();
            continue;
        }

        panic!("Unexpected host message: {:?}", received);
    }
}

async fn ingress_task<'a>(
    mut ingress: Ingress<
        'a,
        AtDigester<common::Urc>,
        common::Urc,
        INGRESS_BUF_SIZE,
        URC_CAPACITY,
        URC_SUBSCRIBERS,
    >,
    mut read: tokio::io::ReadHalf<DuplexStream>,
) -> ! {
    let mut buf = [0; 1024];

    while let Ok(n) = read.read(&mut buf).await {
        let received = core::str::from_utf8(&buf[..n]).unwrap();
        log::debug!("Received from device: {:?}", received);

        ingress
            .try_write(&buf[..n])
            .expect("Failed to write to ingress");
    }

    panic!("Failed to read data");
}
