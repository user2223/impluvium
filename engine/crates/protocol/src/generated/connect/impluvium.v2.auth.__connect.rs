///Shorthand for `OwnedView<StartPairingRequestView<'static>>`.
pub type OwnedStartPairingRequestView = ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::StartPairingRequestView<'static>,
>;
///Shorthand for `OwnedView<StartPairingResponseView<'static>>`.
pub type OwnedStartPairingResponseView = ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::StartPairingResponseView<'static>,
>;
///Shorthand for `OwnedView<PollPairingRequestView<'static>>`.
pub type OwnedPollPairingRequestView = ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::PollPairingRequestView<'static>,
>;
///Shorthand for `OwnedView<PollPairingResponseView<'static>>`.
pub type OwnedPollPairingResponseView = ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::PollPairingResponseView<'static>,
>;
///Shorthand for `OwnedView<BeginSessionRequestView<'static>>`.
pub type OwnedBeginSessionRequestView = ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::BeginSessionRequestView<'static>,
>;
///Shorthand for `OwnedView<BeginSessionResponseView<'static>>`.
pub type OwnedBeginSessionResponseView = ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::BeginSessionResponseView<'static>,
>;
///Shorthand for `OwnedView<CompleteSessionRequestView<'static>>`.
pub type OwnedCompleteSessionRequestView = ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::CompleteSessionRequestView<'static>,
>;
///Shorthand for `OwnedView<CompleteSessionResponseView<'static>>`.
pub type OwnedCompleteSessionResponseView = ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::CompleteSessionResponseView<'static>,
>;
///Shorthand for `OwnedView<RevokeSelfRequestView<'static>>`.
pub type OwnedRevokeSelfRequestView = ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::RevokeSelfRequestView<'static>,
>;
///Shorthand for `OwnedView<RevokeSelfResponseView<'static>>`.
pub type OwnedRevokeSelfResponseView = ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::RevokeSelfResponseView<'static>,
>;
impl ::connectrpc::Encodable<crate::proto::impluvium::v2::StartPairingResponse>
for crate::proto::impluvium::v2::__buffa::view::StartPairingResponseView<'_> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<crate::proto::impluvium::v2::StartPairingResponse>
for ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::StartPairingResponseView<'static>,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<crate::proto::impluvium::v2::PollPairingResponse>
for crate::proto::impluvium::v2::__buffa::view::PollPairingResponseView<'_> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<crate::proto::impluvium::v2::PollPairingResponse>
for ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::PollPairingResponseView<'static>,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<crate::proto::impluvium::v2::BeginSessionResponse>
for crate::proto::impluvium::v2::__buffa::view::BeginSessionResponseView<'_> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<crate::proto::impluvium::v2::BeginSessionResponse>
for ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::BeginSessionResponseView<'static>,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<crate::proto::impluvium::v2::CompleteSessionResponse>
for crate::proto::impluvium::v2::__buffa::view::CompleteSessionResponseView<'_> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<crate::proto::impluvium::v2::CompleteSessionResponse>
for ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::CompleteSessionResponseView<'static>,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
impl ::connectrpc::Encodable<crate::proto::impluvium::v2::RevokeSelfResponse>
for crate::proto::impluvium::v2::__buffa::view::RevokeSelfResponseView<'_> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self, codec)
    }
}
impl ::connectrpc::Encodable<crate::proto::impluvium::v2::RevokeSelfResponse>
for ::buffa::view::OwnedView<
    crate::proto::impluvium::v2::__buffa::view::RevokeSelfResponseView<'static>,
> {
    fn encode(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::buffa::bytes::Bytes, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body(self.reborrow(), codec)
    }
    /// An `OwnedView` still holds the buffer it was decoded from, so
    /// its large fields can be handed to the response body by
    /// reference count instead of copied. The bare view impl above
    /// cannot do this: it has borrows but no buffer to name.
    fn encode_segments(
        &self,
        codec: ::connectrpc::CodecFormat,
    ) -> ::std::result::Result<::connectrpc::EncodedBody, ::connectrpc::ConnectError> {
        ::connectrpc::__codegen::encode_view_body_segments(
            self.reborrow(),
            self.bytes(),
            codec,
        )
    }
}
/// Full service name for this service.
pub const AUTH_SERVICE_SERVICE_NAME: &str = "impluvium.v2.AuthService";
/// Static [`Spec`](::connectrpc::Spec) for the `StartPairing` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const AUTH_SERVICE_START_PAIRING_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/impluvium.v2.AuthService/StartPairing",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `PollPairing` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const AUTH_SERVICE_POLL_PAIRING_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/impluvium.v2.AuthService/PollPairing",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `BeginSession` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const AUTH_SERVICE_BEGIN_SESSION_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/impluvium.v2.AuthService/BeginSession",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `CompleteSession` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const AUTH_SERVICE_COMPLETE_SESSION_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/impluvium.v2.AuthService/CompleteSession",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Static [`Spec`](::connectrpc::Spec) for the `RevokeSelf` RPC, as seen by the server; the generated client passes it with [`origin`](::connectrpc::Spec::origin) `Client` (compare across sides with [`Spec::same_method`](::connectrpc::Spec::same_method)).
pub const AUTH_SERVICE_REVOKE_SELF_SPEC: ::connectrpc::Spec = ::connectrpc::Spec::server(
        "/impluvium.v2.AuthService/RevokeSelf",
        ::connectrpc::StreamType::Unary,
    )
    .with_idempotency_level(::connectrpc::IdempotencyLevel::Unknown);
/// Device pairing, session establishment and sign-out.
///
/// # Implementing handlers
///
/// Implement methods with plain `async fn`; the returned future satisfies
/// the `Send` bound automatically.
///
/// **Unary and server-streaming requests** arrive as
/// [`ServiceRequest<'_, Req>`](::connectrpc::ServiceRequest): a zero-copy
/// view of the request plus its body, valid for the duration of the call.
/// Fields are read directly (`request.name` is a `&str` into the decoded
/// buffer) and the borrow may be held across `.await` points. Anything
/// that must outlive the call — `tokio::spawn`, channels, server state,
/// or data captured by a returned response stream — takes owned data:
/// call `request.to_owned_message()` (or copy the specific fields)
/// first.
///
/// **Client-streaming and bidi requests** arrive as
/// [`InboundStream<Req>`](::connectrpc::InboundStream) — a
/// `ServiceStream` of [`StreamMessage`](::connectrpc::StreamMessage)s.
/// Each item owns its decoded buffer and is `Send + 'static`, so items
/// can be buffered or moved into spawned tasks; read fields zero-copy
/// through the generated accessor methods (`item.name()`) or `.view()`,
/// convert with `.to_owned_message()`, or yield an item back unchanged —
/// `StreamMessage<M>` implements `Encodable<M>`.
///
/// Request types resolved through `extern_path` (e.g. well-known types
/// from another crate) use the same wrappers; the crate that owns the
/// type must be generated with buffa ≥ 0.9.0 and views enabled so the
/// backing `HasMessageView` impl exists.
///
/// The `impl Encodable<Out>` return bound accepts the owned `Out`, the
/// generated `OutView<'_>` / `OwnedOutView`,
/// [`MaybeBorrowed`](::connectrpc::MaybeBorrowed), or
/// [`PreEncoded`](::connectrpc::PreEncoded) for handlers that encode a
/// non-`'static` view internally and pass the bytes across the handler
/// boundary. View bodies are not emitted for output types mapped via
/// `extern_path` (the impl would be an orphan); return owned for
/// WKT/extern outputs.
///
/// Server-streaming and bidi-streaming methods return
/// `ServiceStream<impl Encodable<Out> + Send + use<Self>>`. The
/// `use<Self>` precise-capturing clause excludes `&self`'s lifetime and
/// the request's lifetime (unary methods use `use<'a, Self>` and may
/// borrow from `&self`), so stream items must be `'static` and cannot
/// borrow from the request. To stream view-encoded data, encode each
/// item inside the stream body and yield
/// [`PreEncoded`](::connectrpc::PreEncoded) — see its `# Streaming
/// example` doc.
#[allow(clippy::type_complexity)]
pub trait AuthService: Send + Sync + 'static {
    /// Starts a pairing. Unauthenticated, rate-limited.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn start_pairing<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::impluvium::v2::StartPairingRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::impluvium::v2::StartPairingResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Polls a pending pairing. Authenticated by a pairing proof.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn poll_pairing<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::impluvium::v2::PollPairingRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::impluvium::v2::PollPairingResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Issues a single-use session challenge. Unauthenticated.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn begin_session<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::impluvium::v2::BeginSessionRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::impluvium::v2::BeginSessionResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Exchanges a session proof for an access token.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn complete_session<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::impluvium::v2::CompleteSessionRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::impluvium::v2::CompleteSessionResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
    /// Revokes the calling device. Requires an access token.
    ///
    /// `'a` lets the response body borrow from `&self` (e.g. server-resident state).
    ///
    /// `request` is borrowed from the request body and is valid for the
    /// duration of the call; message fields are read directly on it
    /// (zero-copy). The response cannot borrow from `request` — use
    /// `.to_owned_message()` (or copy the specific fields) for anything
    /// returned, stored, or moved into `tokio::spawn`.
    fn revoke_self<'a>(
        &'a self,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::ServiceRequest<
            '_,
            crate::proto::impluvium::v2::RevokeSelfRequest,
        >,
    ) -> impl ::std::future::Future<
        Output = ::connectrpc::ServiceResult<
            impl ::connectrpc::Encodable<
                crate::proto::impluvium::v2::RevokeSelfResponse,
            > + Send + use<'a, Self>,
        >,
    > + Send;
}
/// Extension trait for registering a service implementation with a Router.
///
/// This trait is automatically implemented for all types that implement the service trait.
/// Prefer [`Router::add_service`](::connectrpc::Router::add_service) for
/// top-down registration; `register` remains available for compatibility
/// and cases where the service-first call shape is more convenient.
///
/// # Example
///
/// ```rust,ignore
/// use std::sync::Arc;
///
/// let service = Arc::new(MyServiceImpl);
/// let router = service.register(Router::new());
/// ```
pub trait AuthServiceExt: AuthService {
    /// Register this service implementation with a Router.
    ///
    /// Takes ownership of the `Arc<Self>` and returns a new Router with
    /// this service's methods registered.
    fn register(
        self: ::std::sync::Arc<Self>,
        router: ::connectrpc::Router,
    ) -> ::connectrpc::Router;
}
impl<S: AuthService> AuthServiceExt for S {
    fn register(
        self: ::std::sync::Arc<Self>,
        router: ::connectrpc::Router,
    ) -> ::connectrpc::Router {
        router
            .route_view(
                AUTH_SERVICE_SERVICE_NAME,
                "StartPairing",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::impluvium::v2::__buffa::view::StartPairingRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::impluvium::v2::StartPairingRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.start_pairing(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::impluvium::v2::StartPairingResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(AUTH_SERVICE_START_PAIRING_SPEC)
            .route_view(
                AUTH_SERVICE_SERVICE_NAME,
                "PollPairing",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::impluvium::v2::__buffa::view::PollPairingRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::impluvium::v2::PollPairingRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.poll_pairing(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::impluvium::v2::PollPairingResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(AUTH_SERVICE_POLL_PAIRING_SPEC)
            .route_view(
                AUTH_SERVICE_SERVICE_NAME,
                "BeginSession",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::impluvium::v2::__buffa::view::BeginSessionRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::impluvium::v2::BeginSessionRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.begin_session(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::impluvium::v2::BeginSessionResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(AUTH_SERVICE_BEGIN_SESSION_SPEC)
            .route_view(
                AUTH_SERVICE_SERVICE_NAME,
                "CompleteSession",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::impluvium::v2::__buffa::view::CompleteSessionRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::impluvium::v2::CompleteSessionRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.complete_session(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::impluvium::v2::CompleteSessionResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(AUTH_SERVICE_COMPLETE_SESSION_SPEC)
            .route_view(
                AUTH_SERVICE_SERVICE_NAME,
                "RevokeSelf",
                {
                    let svc = ::std::sync::Arc::clone(&self);
                    ::connectrpc::view_handler_fn(move |
                        ctx,
                        req: ::buffa::view::OwnedView<
                            crate::proto::impluvium::v2::__buffa::view::RevokeSelfRequestView<
                                'static,
                            >,
                        >,
                        format|
                    {
                        let svc = ::std::sync::Arc::clone(&svc);
                        async move {
                            let sreq = ::connectrpc::ServiceRequest::<
                                crate::proto::impluvium::v2::RevokeSelfRequest,
                            >::from_parts(req.reborrow(), req.bytes());
                            svc.revoke_self(ctx, sreq)
                                .await?
                                .encode::<
                                    crate::proto::impluvium::v2::RevokeSelfResponse,
                                >(format)
                        }
                    })
                },
            )
            .with_spec(AUTH_SERVICE_REVOKE_SELF_SPEC)
    }
}
/// Type-inference marker used by [`Router::add_service`](::connectrpc::Router::add_service).
#[doc(hidden)]
pub struct AuthServiceRegisterMarker;
impl<S: AuthService> ::connectrpc::ServiceRegister<AuthServiceRegisterMarker>
for ::std::sync::Arc<S> {
    fn register_service(self, router: ::connectrpc::Router) -> ::connectrpc::Router {
        <S as AuthServiceExt>::register(self, router)
    }
}
/// Monomorphic dispatcher for `AuthService`.
///
/// Unlike `.register(Router)` which type-erases each method into an `Arc<dyn ErasedHandler>` stored in a `HashMap`, this struct dispatches via a compile-time `match` on method name: no vtable, no hash lookup.
///
/// # Example
///
/// ```rust,ignore
/// use connectrpc::ConnectRpcService;
///
/// let server = AuthServiceServer::new(MyImpl);
/// let service = ConnectRpcService::new(server);
/// // hand `service` to axum/hyper as a fallback_service
/// ```
pub struct AuthServiceServer<T> {
    inner: ::std::sync::Arc<T>,
}
impl<T: AuthService> AuthServiceServer<T> {
    /// Wrap a service implementation in a monomorphic dispatcher.
    pub fn new(service: T) -> Self {
        Self {
            inner: ::std::sync::Arc::new(service),
        }
    }
    /// Wrap an already-`Arc`'d service implementation.
    pub fn from_arc(inner: ::std::sync::Arc<T>) -> Self {
        Self { inner }
    }
}
impl<T> Clone for AuthServiceServer<T> {
    fn clone(&self) -> Self {
        Self {
            inner: ::std::sync::Arc::clone(&self.inner),
        }
    }
}
impl<T: AuthService> ::connectrpc::Dispatcher for AuthServiceServer<T> {
    #[inline]
    fn lookup(
        &self,
        path: &str,
    ) -> Option<::connectrpc::dispatcher::codegen::MethodDescriptor> {
        let method = path.strip_prefix("impluvium.v2.AuthService/")?;
        match method {
            "StartPairing" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(AUTH_SERVICE_START_PAIRING_SPEC),
                )
            }
            "PollPairing" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(AUTH_SERVICE_POLL_PAIRING_SPEC),
                )
            }
            "BeginSession" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(AUTH_SERVICE_BEGIN_SESSION_SPEC),
                )
            }
            "CompleteSession" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(AUTH_SERVICE_COMPLETE_SESSION_SPEC),
                )
            }
            "RevokeSelf" => {
                Some(
                    ::connectrpc::dispatcher::codegen::MethodDescriptor::unary(false)
                        .with_spec(AUTH_SERVICE_REVOKE_SELF_SPEC),
                )
            }
            _ => None,
        }
    }
    fn call_unary(
        &self,
        path: &str,
        ctx: ::connectrpc::RequestContext,
        request: ::connectrpc::Payload,
        format: ::connectrpc::CodecFormat,
    ) -> ::connectrpc::dispatcher::codegen::UnaryResult {
        let Some(method) = path.strip_prefix("impluvium.v2.AuthService/") else {
            return ::connectrpc::dispatcher::codegen::unimplemented_unary(path);
        };
        let _ = (&ctx, &request, &format);
        match method {
            "StartPairing" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::impluvium::v2::StartPairingRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::impluvium::v2::__buffa::view::StartPairingRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::impluvium::v2::StartPairingRequest,
                    >::from_parts(&req, &body);
                    svc.start_pairing(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::impluvium::v2::StartPairingResponse,
                        >(format)
                })
            }
            "PollPairing" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::impluvium::v2::PollPairingRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::impluvium::v2::__buffa::view::PollPairingRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::impluvium::v2::PollPairingRequest,
                    >::from_parts(&req, &body);
                    svc.poll_pairing(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::impluvium::v2::PollPairingResponse,
                        >(format)
                })
            }
            "BeginSession" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::impluvium::v2::BeginSessionRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::impluvium::v2::__buffa::view::BeginSessionRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::impluvium::v2::BeginSessionRequest,
                    >::from_parts(&req, &body);
                    svc.begin_session(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::impluvium::v2::BeginSessionResponse,
                        >(format)
                })
            }
            "CompleteSession" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::impluvium::v2::CompleteSessionRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::impluvium::v2::__buffa::view::CompleteSessionRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::impluvium::v2::CompleteSessionRequest,
                    >::from_parts(&req, &body);
                    svc.complete_session(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::impluvium::v2::CompleteSessionResponse,
                        >(format)
                })
            }
            "RevokeSelf" => {
                let svc = ::std::sync::Arc::clone(&self.inner);
                Box::pin(async move {
                    let body = ::connectrpc::dispatcher::codegen::request_proto_bytes::<
                        crate::proto::impluvium::v2::RevokeSelfRequest,
                    >(request.encoded()?, format)?;
                    let req: crate::proto::impluvium::v2::__buffa::view::RevokeSelfRequestView<
                        '_,
                    > = ::connectrpc::dispatcher::codegen::decode_borrowed_request_view(
                        &body,
                        ctx.decode_options(),
                    )?;
                    let req = ::connectrpc::ServiceRequest::<
                        crate::proto::impluvium::v2::RevokeSelfRequest,
                    >::from_parts(&req, &body);
                    svc.revoke_self(ctx, req)
                        .await?
                        .encode::<
                            crate::proto::impluvium::v2::RevokeSelfResponse,
                        >(format)
                })
            }
            _ => ::connectrpc::dispatcher::codegen::unimplemented_unary(path),
        }
    }
    fn call_server_streaming(
        &self,
        path: &str,
        ctx: ::connectrpc::RequestContext,
        request: ::buffa::bytes::Bytes,
        format: ::connectrpc::CodecFormat,
    ) -> ::connectrpc::dispatcher::codegen::StreamingResult {
        let Some(method) = path.strip_prefix("impluvium.v2.AuthService/") else {
            return ::connectrpc::dispatcher::codegen::unimplemented_streaming(path);
        };
        let _ = (&ctx, &request, &format);
        match method {
            _ => ::connectrpc::dispatcher::codegen::unimplemented_streaming(path),
        }
    }
    fn call_client_streaming(
        &self,
        path: &str,
        ctx: ::connectrpc::RequestContext,
        requests: ::connectrpc::dispatcher::codegen::RequestStream,
        format: ::connectrpc::CodecFormat,
    ) -> ::connectrpc::dispatcher::codegen::UnaryResult {
        let Some(method) = path.strip_prefix("impluvium.v2.AuthService/") else {
            return ::connectrpc::dispatcher::codegen::unimplemented_unary(path);
        };
        let _ = (&ctx, &requests, &format);
        match method {
            _ => ::connectrpc::dispatcher::codegen::unimplemented_unary(path),
        }
    }
    fn call_bidi_streaming(
        &self,
        path: &str,
        ctx: ::connectrpc::RequestContext,
        requests: ::connectrpc::dispatcher::codegen::RequestStream,
        format: ::connectrpc::CodecFormat,
    ) -> ::connectrpc::dispatcher::codegen::StreamingResult {
        let Some(method) = path.strip_prefix("impluvium.v2.AuthService/") else {
            return ::connectrpc::dispatcher::codegen::unimplemented_streaming(path);
        };
        let _ = (&ctx, &requests, &format);
        match method {
            _ => ::connectrpc::dispatcher::codegen::unimplemented_streaming(path),
        }
    }
}
/// Client for this service.
///
/// Generic over `T: ClientTransport`. For **gRPC** (HTTP/2), use
/// `Http2Connection` — it has honest `poll_ready` and composes with
/// `tower::balance` for multi-connection load balancing. For **Connect
/// over HTTP/1.1** (or unknown protocol), use `HttpClient`.
///
/// # Example (gRPC / HTTP/2)
///
/// ```rust,ignore
/// use connectrpc::client::{Http2Connection, ClientConfig};
/// use connectrpc::Protocol;
///
/// let uri: http::Uri = "http://localhost:8080".parse()?;
/// let conn = Http2Connection::connect_plaintext(uri.clone()).await?.shared(1024);
/// let config = ClientConfig::new(uri).with_protocol(Protocol::Grpc);
///
/// let client = AuthServiceClient::new(conn, config);
/// let response = client.start_pairing(request).await?;
/// ```
///
/// # Example (Connect / HTTP/1.1 or ALPN)
///
/// ```rust,ignore
/// use connectrpc::client::{HttpClient, ClientConfig};
///
/// let http = HttpClient::plaintext();  // cleartext http:// only
/// let config = ClientConfig::new("http://localhost:8080".parse()?);
///
/// let client = AuthServiceClient::new(http, config);
/// let response = client.start_pairing(request).await?;
/// ```
///
/// # Working with the response
///
/// Unary calls return [`UnaryResponse<OwnedView<FooView>>`](::connectrpc::client::UnaryResponse).
/// [`view()`](::connectrpc::client::UnaryResponse::view) borrows the response
/// message, so field access is zero-copy:
///
/// ```rust,ignore
/// let resp = client.start_pairing(request).await?;
/// let name: &str = resp.view().name;  // borrow into the response buffer
/// ```
///
/// If you need the owned struct (e.g. to store or pass by value), use
/// [`into_owned()`](::connectrpc::client::UnaryResponse::into_owned):
///
/// ```rust,ignore
/// let owned = client.start_pairing(request).await?.into_owned();
/// ```
///
/// [`into_view()`](::connectrpc::client::UnaryResponse::into_view) keeps the
/// zero-copy decoded body (an `OwnedView`) without copying; field access on it
/// goes through `.reborrow()`. Streaming responses yield one
/// [`StreamMessage`](::connectrpc::StreamMessage) per received message from
/// `.message().await` — read fields zero-copy through the generated accessor
/// methods (`msg.name()`) or `.view()`, or convert with `.to_owned_message()`.
#[cfg(feature = "client")]
#[derive(Clone)]
pub struct AuthServiceClient<T> {
    transport: T,
    config: ::connectrpc::client::ClientConfig,
}
#[cfg(feature = "client")]
impl<T> AuthServiceClient<T>
where
    T: ::connectrpc::client::ClientTransport,
    <T::ResponseBody as ::connectrpc::http_body::Body>::Error: ::std::fmt::Display,
{
    /// Create a new client with the given transport and configuration.
    pub fn new(transport: T, config: ::connectrpc::client::ClientConfig) -> Self {
        Self { transport, config }
    }
    /// Get the client configuration.
    pub fn config(&self) -> &::connectrpc::client::ClientConfig {
        &self.config
    }
    /// Get a mutable reference to the client configuration.
    pub fn config_mut(&mut self) -> &mut ::connectrpc::client::ClientConfig {
        &mut self.config
    }
    /// Call the StartPairing RPC. Sends a request to /impluvium.v2.AuthService/StartPairing.
    pub async fn start_pairing(
        &self,
        request: crate::proto::impluvium::v2::StartPairingRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::impluvium::v2::__buffa::view::StartPairingResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.start_pairing_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the StartPairing RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn start_pairing_with_options(
        &self,
        request: crate::proto::impluvium::v2::StartPairingRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::impluvium::v2::__buffa::view::StartPairingResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                AUTH_SERVICE_START_PAIRING_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the PollPairing RPC. Sends a request to /impluvium.v2.AuthService/PollPairing.
    pub async fn poll_pairing(
        &self,
        request: crate::proto::impluvium::v2::PollPairingRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::impluvium::v2::__buffa::view::PollPairingResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.poll_pairing_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the PollPairing RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn poll_pairing_with_options(
        &self,
        request: crate::proto::impluvium::v2::PollPairingRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::impluvium::v2::__buffa::view::PollPairingResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                AUTH_SERVICE_POLL_PAIRING_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the BeginSession RPC. Sends a request to /impluvium.v2.AuthService/BeginSession.
    pub async fn begin_session(
        &self,
        request: crate::proto::impluvium::v2::BeginSessionRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::impluvium::v2::__buffa::view::BeginSessionResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.begin_session_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the BeginSession RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn begin_session_with_options(
        &self,
        request: crate::proto::impluvium::v2::BeginSessionRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::impluvium::v2::__buffa::view::BeginSessionResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                AUTH_SERVICE_BEGIN_SESSION_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the CompleteSession RPC. Sends a request to /impluvium.v2.AuthService/CompleteSession.
    pub async fn complete_session(
        &self,
        request: crate::proto::impluvium::v2::CompleteSessionRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::impluvium::v2::__buffa::view::CompleteSessionResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.complete_session_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the CompleteSession RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn complete_session_with_options(
        &self,
        request: crate::proto::impluvium::v2::CompleteSessionRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::impluvium::v2::__buffa::view::CompleteSessionResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                AUTH_SERVICE_COMPLETE_SESSION_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
    /// Call the RevokeSelf RPC. Sends a request to /impluvium.v2.AuthService/RevokeSelf.
    pub async fn revoke_self(
        &self,
        request: crate::proto::impluvium::v2::RevokeSelfRequest,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::impluvium::v2::__buffa::view::RevokeSelfResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        self.revoke_self_with_options(
                request,
                ::connectrpc::client::CallOptions::default(),
            )
            .await
    }
    /// Call the RevokeSelf RPC with explicit per-call options. Options override [`ClientConfig`](::connectrpc::client::ClientConfig) defaults.
    pub async fn revoke_self_with_options(
        &self,
        request: crate::proto::impluvium::v2::RevokeSelfRequest,
        options: ::connectrpc::client::CallOptions,
    ) -> Result<
        ::connectrpc::client::UnaryResponse<
            ::buffa::view::OwnedView<
                crate::proto::impluvium::v2::__buffa::view::RevokeSelfResponseView<
                    'static,
                >,
            >,
        >,
        ::connectrpc::ConnectError,
    > {
        ::connectrpc::client::call_unary(
                &self.transport,
                &self.config,
                AUTH_SERVICE_REVOKE_SELF_SPEC
                    .with_origin(::connectrpc::SpecOrigin::Client),
                request,
                options,
            )
            .await
    }
}
