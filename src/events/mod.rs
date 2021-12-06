mod io;

extern crate url;

use crate::cloud_provider::Kind;
use crate::errors::EngineError;
use crate::models::QoveryIdentifier;

#[derive(Debug, Clone)]
pub enum EngineEvent {
    Error(EngineError),
    Waiting,
    Deploying,
    Pausing,
    Deleting,
    Deployed,
    Paused,
    Deleted,
}

#[derive(Debug, Clone)]
pub enum Stage {
    Infrastructure(InfrastructureStep),
    Environment(EnvironmentStep),
}

#[derive(Debug, Clone)]
pub enum InfrastructureStep {
    Instantiate,
    Create,
    Pause,
    Upgrade,
    Delete,
}

#[derive(Debug, Clone)]
pub enum EnvironmentStep {
    Build,
    Deploy,
    Update,
    Delete,
}

type TranmsmitterId = String;
type TransmitterName = String;
type TransmitterType = String;

#[derive(Debug, Clone)]
pub enum Transmitter {
    Engine,
    BuildPlatform(TranmsmitterId, TransmitterName),
    ContainerRegistry(TranmsmitterId, TransmitterName),
    CloudProvider(TranmsmitterId, TransmitterName),
    Kubernetes(TranmsmitterId, TransmitterName),
    DnsProvider(TranmsmitterId, TransmitterName),
    ObjectStorage(TranmsmitterId, TransmitterName),
    Environment(TranmsmitterId, TransmitterName),
    Database(TranmsmitterId, TransmitterType, TransmitterName),
    Application(TranmsmitterId, TransmitterName),
    Router(TranmsmitterId, TransmitterName),
}

#[derive(Debug, Clone)]
pub enum Tag {
    UnsupportedInstanceType(String),
}

#[derive(Debug, Clone)]
pub struct EventDetails {
    provider_kind: Kind,
    organisation_id: QoveryIdentifier,
    cluster_id: QoveryIdentifier,
    execution_id: QoveryIdentifier,
    stage: Stage,
    transmitter: Transmitter,
}

impl EventDetails {
    pub fn new(
        provider_kind: Kind,
        organisation_id: QoveryIdentifier,
        cluster_id: QoveryIdentifier,
        execution_id: QoveryIdentifier,
        stage: Stage,
        transmitter: Transmitter,
    ) -> Self {
        EventDetails {
            provider_kind,
            organisation_id,
            cluster_id,
            execution_id,
            stage,
            transmitter,
        }
    }
    pub fn provider_kind(&self) -> &Kind {
        &self.provider_kind
    }
    pub fn organisation_id(&self) -> &QoveryIdentifier {
        &self.organisation_id
    }
    pub fn cluster_id(&self) -> &QoveryIdentifier {
        &self.cluster_id
    }
    pub fn execution_id(&self) -> &QoveryIdentifier {
        &self.execution_id
    }
    pub fn stage(&self) -> &Stage {
        &self.stage
    }
    pub fn transmitter(&self) -> Transmitter {
        self.transmitter.clone()
    }
}
