#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DeploymentRequest {
    #[prost(enumeration = "DeploymentType", tag = "1")]
    pub deployment_type: i32,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DeploymentInfo {
    #[prost(string, tag = "1")]
    pub organization_id: ::prost::alloc::string::String,
    #[prost(string, tag = "2")]
    pub cluster_id: ::prost::alloc::string::String,
    #[prost(string, tag = "3")]
    pub execution_id: ::prost::alloc::string::String,
    #[prost(string, tag = "4")]
    pub request_id: ::prost::alloc::string::String,
    #[prost(enumeration = "DeploymentType", tag = "5")]
    pub r#type: i32,
    #[prost(string, tag = "6")]
    pub last_message_id: ::prost::alloc::string::String,
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EngineMessageTx {
    #[prost(oneof = "engine_message_tx::Message", tags = "1, 2")]
    pub message: ::core::option::Option<engine_message_tx::Message>,
}
/// Nested message and enum types in `EngineMessageTx`.
pub mod engine_message_tx {
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Message {
        #[prost(message, tag = "1")]
        DeploymentRequest(super::DeploymentInfo),
        #[prost(string, tag = "2")]
        Log(::prost::alloc::string::String),
    }
}
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct EngineMessageRx {
    #[prost(string, tag = "1")]
    pub message_id: ::prost::alloc::string::String,
    #[prost(oneof = "engine_message_rx::Request", tags = "2, 3")]
    pub request: ::core::option::Option<engine_message_rx::Request>,
}
/// Nested message and enum types in `EngineMessageRx`.
pub mod engine_message_rx {
    #[allow(clippy::derive_partial_eq_without_eq)]
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Request {
        #[prost(string, tag = "2")]
        DeploymentRequest(::prost::alloc::string::String),
        #[prost(int32, tag = "3")]
        DeploymentCancel(i32),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum DeploymentType {
    /// empty value in proto map to 0
    Unknown = 0,
    Environment = 1,
    Infrastructure = 2,
}
impl DeploymentType {
    /// String value of the enum field names used in the ProtoBuf definition.
    ///
    /// The values are not transformed in any way and thus are considered stable
    /// (if the ProtoBuf definition does not change) and safe for programmatic use.
    pub fn as_str_name(&self) -> &'static str {
        match self {
            DeploymentType::Unknown => "Unknown",
            DeploymentType::Environment => "Environment",
            DeploymentType::Infrastructure => "Infrastructure",
        }
    }
    /// Creates an enum from field names used in the ProtoBuf definition.
    pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
        match value {
            "Unknown" => Some(Self::Unknown),
            "Environment" => Some(Self::Environment),
            "Infrastructure" => Some(Self::Infrastructure),
            _ => None,
        }
    }
}
/// Generated client implementations.
pub mod engine_client {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::http::Uri;
    use tonic::codegen::*;
    #[derive(Debug, Clone)]
    pub struct EngineClient<T> {
        inner: tonic::client::Grpc<T>,
    }
    impl EngineClient<tonic::transport::Channel> {
        /// Attempt to create a new client by connecting to a given endpoint.
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: std::convert::TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }
    impl<T> EngineClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::BoxBody>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            let inner = tonic::client::Grpc::new(inner);
            Self { inner }
        }
        pub fn with_origin(inner: T, origin: Uri) -> Self {
            let inner = tonic::client::Grpc::with_origin(inner, origin);
            Self { inner }
        }
        pub fn with_interceptor<F>(inner: T, interceptor: F) -> EngineClient<InterceptedService<T, F>>
        where
            F: tonic::service::Interceptor,
            T::ResponseBody: Default,
            T: tonic::codegen::Service<
                http::Request<tonic::body::BoxBody>,
                Response = http::Response<<T as tonic::client::GrpcService<tonic::body::BoxBody>>::ResponseBody>,
            >,
            <T as tonic::codegen::Service<http::Request<tonic::body::BoxBody>>>::Error: Into<StdError> + Send + Sync,
        {
            EngineClient::new(InterceptedService::new(inner, interceptor))
        }
        /// Compress requests with the given encoding.
        ///
        /// This requires the server to support it otherwise it might respond with an
        /// error.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.send_compressed(encoding);
            self
        }
        /// Enable decompressing responses.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.inner = self.inner.accept_compressed(encoding);
            self
        }
        pub async fn get_new_deployment(
            &mut self,
            request: impl tonic::IntoRequest<super::DeploymentRequest>,
        ) -> Result<tonic::Response<super::DeploymentInfo>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into()))
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/engine.Engine/GetNewDeployment");
            self.inner.unary(request.into_request(), path, codec).await
        }
        pub async fn exec_deployment(
            &mut self,
            request: impl tonic::IntoStreamingRequest<Message = super::EngineMessageTx>,
        ) -> Result<tonic::Response<tonic::codec::Streaming<super::EngineMessageRx>>, tonic::Status> {
            self.inner.ready().await.map_err(|e| {
                tonic::Status::new(tonic::Code::Unknown, format!("Service was not ready: {}", e.into()))
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path = http::uri::PathAndQuery::from_static("/engine.Engine/ExecDeployment");
            self.inner
                .streaming(request.into_streaming_request(), path, codec)
                .await
        }
    }
}
/// Generated server implementations.
pub mod engine_server {
    #![allow(unused_variables, dead_code, missing_docs, clippy::let_unit_value)]
    use tonic::codegen::*;
    /// Generated trait containing gRPC methods that should be implemented for use with EngineServer.
    #[async_trait]
    pub trait Engine: Send + Sync + 'static {
        async fn get_new_deployment(
            &self,
            request: tonic::Request<super::DeploymentRequest>,
        ) -> Result<tonic::Response<super::DeploymentInfo>, tonic::Status>;
        /// Server streaming response type for the ExecDeployment method.
        type ExecDeploymentStream: futures_core::Stream<Item = Result<super::EngineMessageRx, tonic::Status>>
            + Send
            + 'static;
        async fn exec_deployment(
            &self,
            request: tonic::Request<tonic::Streaming<super::EngineMessageTx>>,
        ) -> Result<tonic::Response<Self::ExecDeploymentStream>, tonic::Status>;
    }
    #[derive(Debug)]
    pub struct EngineServer<T: Engine> {
        inner: _Inner<T>,
        accept_compression_encodings: EnabledCompressionEncodings,
        send_compression_encodings: EnabledCompressionEncodings,
    }
    struct _Inner<T>(Arc<T>);
    impl<T: Engine> EngineServer<T> {
        pub fn new(inner: T) -> Self {
            Self::from_arc(Arc::new(inner))
        }
        pub fn from_arc(inner: Arc<T>) -> Self {
            let inner = _Inner(inner);
            Self {
                inner,
                accept_compression_encodings: Default::default(),
                send_compression_encodings: Default::default(),
            }
        }
        pub fn with_interceptor<F>(inner: T, interceptor: F) -> InterceptedService<Self, F>
        where
            F: tonic::service::Interceptor,
        {
            InterceptedService::new(Self::new(inner), interceptor)
        }
        /// Enable decompressing requests with the given encoding.
        #[must_use]
        pub fn accept_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.accept_compression_encodings.enable(encoding);
            self
        }
        /// Compress responses with the given encoding, if the client supports it.
        #[must_use]
        pub fn send_compressed(mut self, encoding: CompressionEncoding) -> Self {
            self.send_compression_encodings.enable(encoding);
            self
        }
    }
    impl<T, B> tonic::codegen::Service<http::Request<B>> for EngineServer<T>
    where
        T: Engine,
        B: Body + Send + 'static,
        B::Error: Into<StdError> + Send + 'static,
    {
        type Response = http::Response<tonic::body::BoxBody>;
        type Error = std::convert::Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;
        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
        fn call(&mut self, req: http::Request<B>) -> Self::Future {
            let inner = self.inner.clone();
            match req.uri().path() {
                "/engine.Engine/GetNewDeployment" => {
                    #[allow(non_camel_case_types)]
                    struct GetNewDeploymentSvc<T: Engine>(pub Arc<T>);
                    impl<T: Engine> tonic::server::UnaryService<super::DeploymentRequest> for GetNewDeploymentSvc<T> {
                        type Response = super::DeploymentInfo;
                        type Future = BoxFuture<tonic::Response<Self::Response>, tonic::Status>;
                        fn call(&mut self, request: tonic::Request<super::DeploymentRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).get_new_deployment(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = GetNewDeploymentSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(accept_compression_encodings, send_compression_encodings);
                        let res = grpc.unary(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                "/engine.Engine/ExecDeployment" => {
                    #[allow(non_camel_case_types)]
                    struct ExecDeploymentSvc<T: Engine>(pub Arc<T>);
                    impl<T: Engine> tonic::server::StreamingService<super::EngineMessageTx> for ExecDeploymentSvc<T> {
                        type Response = super::EngineMessageRx;
                        type ResponseStream = T::ExecDeploymentStream;
                        type Future = BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
                        fn call(
                            &mut self,
                            request: tonic::Request<tonic::Streaming<super::EngineMessageTx>>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            let fut = async move { (*inner).exec_deployment(request).await };
                            Box::pin(fut)
                        }
                    }
                    let accept_compression_encodings = self.accept_compression_encodings;
                    let send_compression_encodings = self.send_compression_encodings;
                    let inner = self.inner.clone();
                    let fut = async move {
                        let inner = inner.0;
                        let method = ExecDeploymentSvc(inner);
                        let codec = tonic::codec::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec)
                            .apply_compression_config(accept_compression_encodings, send_compression_encodings);
                        let res = grpc.streaming(method, req).await;
                        Ok(res)
                    };
                    Box::pin(fut)
                }
                _ => Box::pin(async move {
                    Ok(http::Response::builder()
                        .status(200)
                        .header("grpc-status", "12")
                        .header("content-type", "application/grpc")
                        .body(empty_body())
                        .unwrap())
                }),
            }
        }
    }
    impl<T: Engine> Clone for EngineServer<T> {
        fn clone(&self) -> Self {
            let inner = self.inner.clone();
            Self {
                inner,
                accept_compression_encodings: self.accept_compression_encodings,
                send_compression_encodings: self.send_compression_encodings,
            }
        }
    }
    impl<T: Engine> Clone for _Inner<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }
    impl<T: std::fmt::Debug> std::fmt::Debug for _Inner<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }
    impl<T: Engine> tonic::server::NamedService for EngineServer<T> {
        const NAME: &'static str = "engine.Engine";
    }
}
