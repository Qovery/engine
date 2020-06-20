mod registry;

use crate::cloud_provider::CloudProvider;
use crate::continuous_integration::ContinuousIntegration;

mod cloud_provider;
mod config;
mod continuous_integration;
mod models;
mod session;
mod transaction;

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::EKS;
    use crate::cloud_provider::aws::AWS;
    use crate::cloud_provider::do_launch_workflow;
    use crate::cloud_provider::gcp::GCP;
    use crate::config::{Config, ConfigError};
    use crate::transaction::{ProgressInfo, ProgressListener};

    struct QoveryStatusSender;

    impl ProgressListener for QoveryStatusSender {
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
    fn test_deploy() {
        let config = Config::<EKS>::from_json("{}");

        let session = match config.session() {
            Ok(session) => session,
            Err(err) => panic!(err),
        };

        let mut tx = session.transaction();

        tx.build();
        tx.push();
        tx.deploy();

        tx.add_build_listener(Box::new(QoveryStatusSender {}));

        tx.commit();
    }
}
