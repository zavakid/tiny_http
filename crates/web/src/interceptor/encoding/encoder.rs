use crate::interceptor::encoding::Writer;
use crate::interceptor::Interceptor;
use crate::{RequestContext, ResponseBody};
use async_trait::async_trait;
use bytes::{Buf, Bytes};
use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;
use http::{Response, StatusCode};
use http_body::{Body, Frame};
use http_body_util::combinators::UnsyncBoxBody;
use micro_http::protocol::{HttpError, SendError};
use pin_project_lite::pin_project;
use std::fmt::Debug;
use std::io;
use std::io::Write;
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use tracing::{error, trace};
use zstd::stream::write::Encoder as ZstdEncoder;

// (almost thanks and) copy from actix-http: https://github.com/actix/actix-web/blob/master/actix-http/src/encoding/encoder.rs

pub(crate) enum Encoder {
    Gzip(GzEncoder<Writer>),
    Deflate(ZlibEncoder<Writer>),
    Zstd(ZstdEncoder<'static, Writer>),
    Br(Box<brotli::CompressorWriter<Writer>>),
}

impl Encoder {
    fn gzip() -> Self {
        Self::Gzip(GzEncoder::new(Writer::new(), Compression::best()))
    }

    fn deflate() -> Self {
        Self::Deflate(ZlibEncoder::new(Writer::new(), Compression::best()))
    }

    fn zstd() -> Self {
        // todo: remove the unwrap
        Self::Zstd(ZstdEncoder::new(Writer::new(), 6).unwrap())
    }

    fn br() -> Self {
        Self::Br(Box::new(brotli::CompressorWriter::new(
            Writer::new(),
            32 * 1024, // 32 KiB buffer
            3,         // BROTLI_PARAM_QUALITY
            22,        // BROTLI_PARAM_LGWIN
        )))
    }

    fn select(accept_encodings: &str) -> Option<Self> {
        if accept_encodings.contains("zstd") {
            Some(Self::zstd())
        } else if accept_encodings.contains("br") {
            Some(Self::br())
        } else if accept_encodings.contains("gzip") {
            Some(Self::gzip())
        } else if accept_encodings.contains("deflate") {
            Some(Self::deflate())
        } else {
            None
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Encoder::Gzip(_) => "gzip",
            Encoder::Deflate(_) => "deflate",
            Encoder::Zstd(_) => "zstd",
            Encoder::Br(_) => "br",
        }
    }

    fn write(&mut self, data: &[u8]) -> Result<(), io::Error> {
        match self {
            Self::Gzip(ref mut encoder) => match encoder.write_all(data) {
                Ok(_) => Ok(()),
                Err(err) => {
                    trace!("Error encoding gzip encoding: {}", err);
                    Err(err)
                }
            },

            Self::Deflate(ref mut encoder) => match encoder.write_all(data) {
                Ok(_) => Ok(()),
                Err(err) => {
                    trace!("Error encoding deflate encoding: {}", err);
                    Err(err)
                }
            },

            Self::Zstd(ref mut encoder) => match encoder.write_all(data) {
                Ok(_) => Ok(()),
                Err(err) => {
                    trace!("Error encoding zstd encoding: {}", err);
                    Err(err)
                }
            },

            Self::Br(ref mut encoder) => match encoder.write_all(data) {
                Ok(_) => Ok(()),
                Err(err) => {
                    trace!("Error encoding br encoding: {}", err);
                    Err(err)
                }
            },
        }
    }

    fn take(&mut self) -> Bytes {
        match *self {
            Self::Gzip(ref mut encoder) => encoder.get_mut().take(),
            Self::Deflate(ref mut encoder) => encoder.get_mut().take(),
            Self::Zstd(ref mut encoder) => encoder.get_mut().take(),
            Self::Br(ref mut encoder) => encoder.get_mut().take(),
        }
    }

    fn finish(self) -> Result<Bytes, io::Error> {
        match self {
            Self::Gzip(encoder) => match encoder.finish() {
                Ok(writer) => Ok(writer.buf.freeze()),
                Err(err) => Err(err),
            },

            Self::Deflate(encoder) => match encoder.finish() {
                Ok(writer) => Ok(writer.buf.freeze()),
                Err(err) => Err(err),
            },

            Self::Zstd(encoder) => match encoder.finish() {
                Ok(writer) => Ok(writer.buf.freeze()),
                Err(err) => Err(err),
            },

            Self::Br(mut encoder) => match encoder.flush() {
                Ok(()) => Ok(encoder.into_inner().buf.freeze()),
                Err(err) => Err(err),
            },
        }
    }
}

pin_project! {
    struct EncodedBody<B: Body> {
        #[pin]
        inner: B,
        encoder: Option<Encoder>,
        state: Option<bool>,
    }
}

impl<B: Body> EncodedBody<B> {
    fn new(b: B, encoder: Encoder) -> Self {
        Self { inner: b, encoder: Some(encoder), state: Some(true) }
    }
}

impl<B> Body for EncodedBody<B>
where
    B: Body + Unpin,
    B::Data: Buf + Debug,
    B::Error: ToString,
{
    type Data = Bytes;
    type Error = HttpError;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let mut this = self.project();

        if this.state.is_none() {
            return Poll::Ready(None);
        }

        loop {
            return match ready!(this.inner.as_mut().poll_frame(cx)) {
                Some(Ok(frame)) => {
                    let data = match frame.into_data() {
                        Ok(data) => data,
                        Err(mut frame) => {
                            let debug_info = frame.trailers_mut();
                            error!("want to data from body, but receive trailer header: {:?}", debug_info);
                            return Poll::Ready(Some(Err(SendError::invalid_body(format!(
                                "invalid body frame : {:?}",
                                debug_info
                            ))
                            .into())));
                        }
                    };

                    match this.encoder.as_mut().unwrap().write(data.chunk()) {
                        Ok(_) => (),
                        Err(e) => {
                            return Poll::Ready(Some(Err(SendError::from(e).into())));
                        }
                    }
                    // use wrap here is safe, because we only take it when receive None
                    let bytes = this.encoder.as_mut().unwrap().take();
                    if bytes.is_empty() {
                        continue;
                    }
                    Poll::Ready(Some(Ok(Frame::data(bytes))))
                }
                Some(Err(e)) => Poll::Ready(Some(Err(SendError::invalid_body(e.to_string()).into()))),
                None => {
                    if this.state.is_some() {
                        // will only run below  code once
                        this.state.take();

                        // unwrap here is safe, because we only take once
                        let bytes = match this.encoder.take().unwrap().finish() {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                return Poll::Ready(Some(Err(SendError::from(e).into())));
                            }
                        };
                        if !bytes.is_empty() {
                            Poll::Ready(Some(Ok(Frame::data(bytes))))
                        } else {
                            Poll::Ready(None)
                        }
                    } else {
                        Poll::Ready(None)
                    }
                }
            };
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
}

pub struct EncodeInterceptor;

#[async_trait]
impl Interceptor for EncodeInterceptor {
    async fn on_response(&self, req: &RequestContext, resp: &mut Response<ResponseBody>) {
        let status_code = resp.status();
        if status_code == StatusCode::NO_CONTENT || status_code == StatusCode::SWITCHING_PROTOCOLS {
            return;
        }

        // response has already encoded
        if req.headers().contains_key(http::header::CONTENT_ENCODING) {
            return;
        }

        // request doesn't have any accept encodings
        let possible_encodings = req.headers().get(http::header::ACCEPT_ENCODING);
        if possible_encodings.is_none() {
            return;
        }

        // here using unwrap is safe because we has checked
        let accept_encodings = match possible_encodings.unwrap().to_str() {
            Ok(s) => s,
            Err(_) => {
                return;
            }
        };

        let encoder = match Encoder::select(accept_encodings) {
            Some(encoder) => encoder,
            None => {
                return;
            }
        };

        let body = resp.body_mut();

        if body.is_empty() {
            return;
        }

        match body.size_hint().upper() {
            Some(upper) if upper <= 1024 => {
                // less then 1k, we needn't compress
                return;
            }
            _ => (),
        }

        let encoder_name = encoder.name();
        let encoded_body = EncodedBody::new(body.take(), encoder);
        body.replace(ResponseBody::stream(UnsyncBoxBody::new(encoded_body)));

        resp.headers_mut().remove(http::header::CONTENT_LENGTH);
        resp.headers_mut().append(http::header::CONTENT_ENCODING, encoder_name.parse().unwrap());
    }
}
