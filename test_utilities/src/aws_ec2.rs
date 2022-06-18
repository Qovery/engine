use qovery_engine::cloud_provider::models::InstanceEc2;

pub const AWS_K3S_VERSION: &str = "v1.20.15+k3s1";

pub fn ec2_kubernetes_instance() -> InstanceEc2 {
    InstanceEc2::new("t3.small".to_string(), 20)
}
