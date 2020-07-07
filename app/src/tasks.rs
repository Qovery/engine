use std::borrow::Borrow;

use crossbeam_channel::Sender;

use qovery_engine_task_manager::models::Request;
use uuid::Uuid;

use qovery_engine::cloud_provider::CloudProviderError;
use qovery_engine::config::Config;
use qovery_engine::error::ConfigurationError;
use qovery_engine_task_manager::task_manager::{InternalTask, Message, Status, Task};

#[derive(Clone)]
pub struct CreateInfrastructureTask {
    id: Uuid,
    request: Request,
}

impl CreateInfrastructureTask {
    pub fn new(request: Request) -> Self {
        CreateInfrastructureTask {
            id: Uuid::new_v4(),
            request,
        }
    }
}

impl Task for CreateInfrastructureTask {
    fn id(&self) -> &Uuid {
        &self.id
    }

    fn update_status(&self, sender: &Sender<Message>, status: Status) {
        let it = InternalTask {
            task: Box::new(self.clone()),
            status,
        };
        let _ = sender.send(Ok(it));
    }

    fn run(&self, sender: Sender<Message>) {
        self.update_status(&sender, Status::Running);

        let build_platform = self.request.build_platform.as_engine_build_platform();
        let cloud_provider = self.request.cloud_provider.as_engine_cloud_provider();

        let container_registry = self
            .request
            .container_registry
            .as_engine_container_registry();

        let nodes = self
            .request
            .cloud_provider
            .kubernetes
            .as_engine_kubernetes_nodes();

        let kubernetes = self
            .request
            .cloud_provider
            .kubernetes
            .as_engine_kubernetes(cloud_provider.borrow(), nodes.borrow());

        let config = Config::new(
            build_platform.borrow(),
            container_registry.borrow(),
            cloud_provider.borrow(),
        );

        // FIXME - return errors with Sender
        let session = match config.session() {
            Ok(session) => session,
            Err(err) => match err {
                ConfigurationError::BuildPlatform(e) => panic!(e),
                ConfigurationError::ContainerRegistry(e) => panic!(e),
                ConfigurationError::CloudProvider(e) => match e {
                    CloudProviderError::Credentials => panic!("bad cloud provider credentials"),
                    CloudProviderError::Error(err) => panic!("qerror: err"),
                    CloudProviderError::Unknown => panic!("cloud provider unknown error"),
                },
            },
        };

        let mut tx = session.transaction();

        tx.create_kubernetes(kubernetes.borrow());

        tx.commit();

        self.update_status(&sender, Status::Done);
    }
}
