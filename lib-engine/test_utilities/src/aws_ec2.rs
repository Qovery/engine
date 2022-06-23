use qovery_engine::cloud_provider::models::InstanceEc2;

pub const AWS_K3S_VERSION: &str = "v1.23.6+k3s1";

pub fn ec2_kubernetes_instance() -> InstanceEc2 {
    InstanceEc2::new("t3.medium".to_string(), 20)
}
