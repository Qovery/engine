use std::any::Any;

use crate::cloud_provider::kubernetes::KubernetesNode;

pub struct Node {
    instance_type: String,
}

impl Node {
    pub fn new_with_cpu_and_mem(total_cpu: u8, total_memory_in_gib: u16) -> Self {
        let instance_types_table = [
            (1, 1, "s-1vcpu-1gb"),
            (1, 2, "s-1vcpu-2gb"),
            (2, 4, "s-2vcpu-4gb"),
            (4, 8, "s-4vcpu-8gb"),
            (6, 16, "s-6vcpu-16gb"),
            (8, 32, "s-8vcpu-32gb"),
        ];

        if total_cpu == 0 || total_memory_in_gib == 0 {
            let (_, _, instance_type) = instance_types_table.first().unwrap();
            return Node::new(*instance_type);
        }

        for (_cpu, mem, instance_type) in instance_types_table.iter() {
            if total_memory_in_gib <= *mem {
                return Node::new(*instance_type);
            }
        }

        let (_, _, instance_type) = instance_types_table.last().unwrap();
        Node::new(*instance_type)
    }

    pub fn new<T: Into<String>>(instance_type: T) -> Self {
        Node {
            instance_type: instance_type.into(),
        }
    }
}

impl KubernetesNode for Node {
    fn instance_type(&self) -> &str {
        self.instance_type.as_str()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
