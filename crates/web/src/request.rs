use http::{HeaderMap, Method, Uri, Version};
use matchit::Params;
use micro_http::protocol::RequestHeader;

pub struct RequestContext<'server: 'req, 'req> {
    request_header: &'req RequestHeader,
    path_params: PathParams<'server, 'req>,
}

impl<'server, 'req> RequestContext<'server, 'req> {
    pub fn new(request_header: &'req RequestHeader, path_params: PathParams<'server, 'req>) -> Self {
        Self { request_header, path_params }
    }

    pub fn request_header(&self) -> &RequestHeader {
        self.request_header
    }

    pub fn method(&self) -> &Method {
        self.request_header.method()
    }

    pub fn uri(&self) -> &Uri {
        self.request_header.uri()
    }

    pub fn version(&self) -> Version {
        self.request_header.version()
    }

    pub fn headers(&self) -> &HeaderMap {
        self.request_header.headers()
    }

    pub fn path_params(&self) -> &PathParams {
        &self.path_params
    }
}

pub struct PathParams<'server, 'req> {
    kind: PathParamsKind<'server, 'req>,
}

enum PathParamsKind<'server, 'req> {
    None,
    Params(Params<'server, 'req>),
}

impl<'server, 'req> PathParams<'server, 'req> {
    fn new(params: Params<'server, 'req>) -> Self {
        Self { kind: PathParamsKind::Params(params) }
    }

    pub fn empty() -> Self {
        Self { kind: PathParamsKind::None }
    }

    pub fn get(&self, key: impl AsRef<str>) -> Option<&'req str> {
        match &self.kind {
            PathParamsKind::Params(params) => params.get(key),
            PathParamsKind::None => None,
        }
    }
}

impl<'server, 'req> From<Params<'server, 'req>> for PathParams<'server, 'req> {
    fn from(params: Params<'server, 'req>) -> Self {
        PathParams::new(params)
    }
}
