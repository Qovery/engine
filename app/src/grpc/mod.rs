use crate::grpc::engine::engine_client::EngineClient;
use std::convert::TryFrom;
use std::time::Duration;
use tonic::codec::CompressionEncoding;
use tonic::codegen::InterceptedService;
use tonic::metadata::{AsciiMetadataValue, MetadataValue};
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig, Uri};
use tonic::{Request, Status};
use tower::ServiceBuilder;
use uuid::Uuid;

pub mod engine;
pub mod qovery_api;

const GRPC_CLUSTER_ID_HEADER_NAME: &str = "x-qovery-cluster";

#[derive(Debug, Clone)]
pub struct QoveryInterceptor {
    token: AsciiMetadataValue,
    cluster_id: AsciiMetadataValue,
}

impl Interceptor for QoveryInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let metadata = request.metadata_mut();
        metadata.insert("authorization", self.token.clone());
        metadata.insert(GRPC_CLUSTER_ID_HEADER_NAME, self.cluster_id.clone());
        Ok(request)
    }
}

pub type GrpcEngineClient = EngineClient<InterceptedService<Channel, QoveryInterceptor>>;

pub async fn new_engine_client(
    grpc_server: Uri,
    cluster_id: &Uuid,
    cluster_token: &str,
) -> Result<GrpcEngineClient, tonic::transport::Error> {
    let enable_tls = grpc_server.scheme_str().unwrap_or("https") == "https";
    let channel = if enable_tls {
        Channel::builder(grpc_server).tls_config(ClientTlsConfig::new())?
    } else {
        Channel::builder(grpc_server)
    };

    let channel = channel
        .connect_timeout(Duration::from_secs(30))
        .http2_keep_alive_interval(Duration::from_secs(60))
        .keep_alive_while_idle(true)
        .tcp_nodelay(true)
        .connect()
        .await?;

    let token =
        MetadataValue::try_from(format!("Bearer {cluster_token}").as_str()).unwrap_or_else(|_| MetadataValue::from(0));
    let cluster_id = MetadataValue::try_from(&cluster_id.to_string()).unwrap_or_else(|_| MetadataValue::from(0));
    let channel = ServiceBuilder::new()
        .layer(tonic::service::interceptor(QoveryInterceptor { token, cluster_id }))
        .service(channel);

    let client = EngineClient::new(channel)
        .accept_compressed(CompressionEncoding::Gzip)
        .send_compressed(CompressionEncoding::Gzip);

    Ok(client)
}

#[cfg(test)]
pub mod test {
    use super::*;
    use tokio::io::DuplexStream;
    use tonic::transport::Endpoint;
    use tower::service_fn;

    pub async fn new_engine_client_test(mut client: Option<DuplexStream>) -> GrpcEngineClient {
        let channel = Endpoint::try_from("http://[::]:50051")
            .unwrap()
            .connect_with_connector(service_fn(move |_: Uri| {
                let client = client.take();
                async move {
                    if let Some(client) = client {
                        Ok(client)
                    } else {
                        Err(std::io::Error::new(std::io::ErrorKind::Other, "Client already taken"))
                    }
                }
            }))
            .await
            .unwrap();

        let channel = ServiceBuilder::new()
            .layer(tonic::service::interceptor(QoveryInterceptor {
                token: AsciiMetadataValue::from_static(""),
                cluster_id: AsciiMetadataValue::from_static(""),
            }))
            .service(channel);

        EngineClient::new(channel)
    }
}
