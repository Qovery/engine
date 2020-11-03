use crate::cloud_provider::kubernetes::KubernetesNode;
use std::any::Any;

pub struct Node {
    total_cpu: u8,
    total_memory_in_gib: u16,
    instance_types_table: [(u8, u16, &'static str); 6],
}

impl Node {
    pub fn new(total_cpu: u8, total_memory_in_gib: u16) -> Self {
        let instance_types_table = [
            (1, 1, "s-1vcpu-1gb"),
            (1, 2, "s-1vcpu-2gb"),
            (2, 4, "s-2vcpu-4gb"),
            (4, 8, "s-4vcpu-8gb"),
            (6, 16, "s-6vcpu-16gb"),
            (8, 32, "s-8vcpu-32gb"),
        ];

        Node {
            total_cpu,
            total_memory_in_gib,
            instance_types_table,
        }
    }
}

impl KubernetesNode for Node {
    fn total_cpu(&self) -> u8 {
        self.total_cpu
    }

    fn total_memory_in_gib(&self) -> u16 {
        self.total_memory_in_gib
    }

    fn instance_type(&self) -> &str {
        if self.total_cpu() == 0 || self.total_memory_in_gib() == 0 {
            let (_, _, instance_type) = self.instance_types_table.first().unwrap();
            return instance_type;
        }

        for (_cpu, mem, instance_type) in self.instance_types_table.iter() {
            if self.total_memory_in_gib() <= *mem {
                return instance_type;
            }
        }

        let (_, _, instance_type) = self.instance_types_table.last().unwrap();
        return instance_type;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
