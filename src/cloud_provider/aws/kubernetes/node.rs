use crate::cloud_provider::KubernetesNode;

pub struct Node {
    pub instance_type: String,
}

impl Node {
    pub fn new(instance_type: &str) -> Self {
        Node {
            instance_type: instance_type.to_string(),
        }
    }
}

impl KubernetesNode for Node {
    fn total_cpu(&self) -> u8 {
        unimplemented!()
    }

    fn total_memory_in_mib(&self) -> u32 {
        unimplemented!()
    }

    fn instance_type(&self) -> &str {
        self.instance_type.as_str()
    }
}
