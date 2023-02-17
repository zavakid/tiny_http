use crate::codec::body::PayloadEncoder;
use crate::codec::header::HeaderEncoder;
use crate::protocol::{Message, PayloadSize, ResponseHead, SendError};
use bytes::BytesMut;
use std::io;
use std::io::ErrorKind;
use tokio_util::codec::Encoder;
use tracing::error;

pub struct ResponseEncoder {
    header_encoder: HeaderEncoder,
    payload_encoder: Option<PayloadEncoder>,
}

impl ResponseEncoder {
    pub fn new() -> Self {
        Self { header_encoder: HeaderEncoder, payload_encoder: None }
    }
}

impl Encoder<Message<(ResponseHead, PayloadSize)>> for ResponseEncoder {
    type Error = SendError;

    fn encode(&mut self, item: Message<(ResponseHead, PayloadSize)>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match item {
            Message::Header((head, payload_size)) => {
                if self.payload_encoder.is_some() {
                    error!("expect payload item but receive response head");
                    return Err(io::Error::from(ErrorKind::InvalidInput).into());
                }

                let payload_encoder = parse_payload_encoder(payload_size);
                self.payload_encoder = Some(payload_encoder);
                self.header_encoder.encode((head, payload_size), dst)
            }

            Message::Payload(payload_item) => {
                let payload_encoder = if let Some(encoder) = &mut self.payload_encoder {
                    encoder
                } else {
                    error!("expect response header but receive payload item");
                    return Err(io::Error::from(ErrorKind::InvalidInput).into());
                };

                let is_eof = payload_item.is_eof();
                let result = payload_encoder.encode(payload_item, dst);

                if is_eof {
                    self.payload_encoder.take();
                }

                result
            }
        }
    }
}

fn parse_payload_encoder(payload_size: PayloadSize) -> PayloadEncoder {
    match payload_size {
        PayloadSize::Length(size) => PayloadEncoder::fix_length(size),
        PayloadSize::Chunked => PayloadEncoder::chunked(),
        PayloadSize::Empty => PayloadEncoder::empty(),
    }
}
