use crate::build_info;
use crate::grpc::engine::engine_client::EngineClient;
use std::convert::TryFrom;
use std::time::Duration;
use tonic::codec::CompressionEncoding;
use tonic::codegen::InterceptedService;
use tonic::metadata::{AsciiMetadataValue, MetadataValue};
use tonic::service::{Interceptor, InterceptorLayer};
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
        Channel::builder(grpc_server).tls_config(ClientTlsConfig::new().with_native_roots())?
    } else {
        Channel::builder(grpc_server)
    };

    let user_agent = format!(
        "engine/{} pod/{}",
        build_info::ENGINE_VERSION,
        std::env::var("ENGINE_NAME").unwrap_or_default()
    );
    let channel = channel
        .connect_timeout(Duration::from_secs(30))
        // Worst case scenario is engine gtw re-dispatching the deployment after 1min while we ack it.
        // So we need to detect the connection is lost before this time
        .http2_keep_alive_interval(Duration::from_secs(20))
        .keep_alive_timeout(Duration::from_secs(10))
        // engine is polling for new deployment. So we are not interested in keeping the connection alive
        // while no deployment is on-going
        .keep_alive_while_idle(false)
        .tcp_nodelay(true)
        .user_agent(user_agent)?
        .connect()
        .await?;

    let token =
        MetadataValue::try_from(format!("Bearer {cluster_token}").as_str()).unwrap_or_else(|_| MetadataValue::from(0));
    let cluster_id = MetadataValue::try_from(&cluster_id.to_string()).unwrap_or_else(|_| MetadataValue::from(0));
    let channel = ServiceBuilder::new()
        .layer(InterceptorLayer::new(QoveryInterceptor { token, cluster_id }))
        .service(channel);

    let client = EngineClient::new(channel)
        .accept_compressed(CompressionEncoding::Zstd)
        .send_compressed(CompressionEncoding::Zstd);

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
                        Ok(hyper_util::rt::TokioIo::new(client))
                    } else {
                        Err(std::io::Error::new(std::io::ErrorKind::Other, "Client already taken"))
                    }
                }
            }))
            .await
            .unwrap();

        let channel = ServiceBuilder::new()
            .layer(InterceptorLayer::new(QoveryInterceptor {
                token: AsciiMetadataValue::from_static(""),
                cluster_id: AsciiMetadataValue::from_static(""),
            }))
            .service(channel);

        EngineClient::new(channel)
    }
}
