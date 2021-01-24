use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Deserialize, Serialize)]
pub struct LoadBalancer {
    pub load_balancer: LoadBalancerInfo,
}

#[derive(Default, Debug, Deserialize, Serialize)]
pub struct LoadBalancerInfo {
    pub ip: String,
}
