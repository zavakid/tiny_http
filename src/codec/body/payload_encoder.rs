use crate::codec::body::chunked_encoder::ChunkedEncoder;
use crate::codec::body::length_encoder::LengthEncoder;
use crate::protocol::PayloadItem;
use bytes::BytesMut;
use std::io;
use tokio_util::codec::Encoder;

/// encode payload for request body
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadEncoder {
    kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// content-length payload
    Length(LengthEncoder),

    /// transfer-encoding chunked payload
    Chunked(ChunkedEncoder),

    /// have no body with the request
    NoBody,
}

impl PayloadEncoder {
    /// create an empty `PayloadEncoder`
    pub fn empty() -> Self {
        Self { kind: Kind::NoBody }
    }

    /// create a chunked `PayloadEncoder`
    pub fn chunked() -> Self {
        Self { kind: Kind::Chunked(ChunkedEncoder::new()) }
    }

    /// create a fixed length `PayloadEncoder`
    pub fn fix_length(size: usize) -> Self {
        Self { kind: Kind::Length(LengthEncoder::new(size)) }
    }

    pub fn is_chunked(&self) -> bool {
        match &self.kind {
            Kind::Length(_) => false,
            Kind::Chunked(_) => true,
            Kind::NoBody => false,
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.kind {
            Kind::Length(_) => false,
            Kind::Chunked(_) => false,
            Kind::NoBody => true,
        }
    }

    pub fn is_fix_length(&self) -> bool {
        match &self.kind {
            Kind::Length(_) => true,
            Kind::Chunked(_) => false,
            Kind::NoBody => false,
        }
    }
}

impl Encoder<PayloadItem> for PayloadEncoder {
    type Error = io::Error;

    fn encode(&mut self, item: PayloadItem, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match &mut self.kind {
            Kind::Length(encoder) => encoder.encode(item, dst),
            Kind::Chunked(encoder) => encoder.encode(item, dst),
            Kind::NoBody => Ok(()),
        }
    }
}