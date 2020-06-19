use crate::cloud_provider::CloudProvider;
use crate::continuous_integration::ContinuousIntegration;
use crate::registry::Registry;

mod cloud_provider;
mod config;
mod continuous_integration;
mod models;
mod registry;
mod session;
mod transaction;

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::EKS;
    use crate::cloud_provider::aws::AWS;
    use crate::cloud_provider::do_launch_workflow;
    use crate::cloud_provider::gcp::GCP;
    use crate::config::{Config, ConfigError};
    use crate::transaction::{ProgressCallback, ProgressInfo};

    struct QoveryStatusSender;

    impl ProgressCallback for QoveryStatusSender {
        fn on_progress(&self, info: &ProgressInfo) {
            unimplemented!()
        }

        fn on_complete(&self, info: &ProgressInfo) {
            unimplemented!()
        }

        fn on_error(&self, info: &ProgressInfo) {
            unimplemented!()
        }
    }

    #[test]
    fn test() {
        let config = Config::<EKS>::from_json("{}");

        let session = match config.session() {
            Ok(session) => session,
            Err(err) => panic!(err),
        };

        let tx = session.transaction();

        tx.build(Box::new(QoveryStatusSender {}));
        tx.push();
        tx.deploy();

        tx.commit();
    }

    #[test]
    fn aws() {
        let aws = AWS::<EKS>::new("", "");
        let gcp = GCP::<EKS>::new("");

        do_launch_workflow(&gcp);
        do_launch_workflow(&aws);
        do_launch_workflow(&gcp);
    }
}
