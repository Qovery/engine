use crate::cloud_provider::KubernetesNode;

pub struct Node {
    total_cpu: u8,
    total_memory_in_gib: u16,
}

impl Node {
    /// Number of CPUs and total memory wanted - the right AWS EC2 instance type is found algorithmically
    /// Eg. total_cpu = 1 and total_memory_in_gib = 2 means `t2.small` instance type
    /// BUT total_cpu = 1 and total_memory_in_gib = 3 does not have an existing instance - so we will pick the upper closest,
    /// which is `t2.medium` with 2 cpu and 4 GiB
    pub fn new(total_cpu: u8, total_memory_in_gib: u16) -> Self {
        Node {
            total_cpu,
            total_memory_in_gib,
        }
    }
}

impl KubernetesNode for Node {
    fn total_cpu(&self) -> u8 {
        unimplemented!()
    }

    fn total_memory_in_gib(&self) -> u16 {
        unimplemented!()
    }

    fn instance_type(&self) -> &str {
        // FIXME: return the right instance type
        if self.total_cpu == 1 {
            "t2.small"
        } else {
            "t2.medium"
        }
    }
}
