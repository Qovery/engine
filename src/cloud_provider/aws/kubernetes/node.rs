use std::any::Any;

use crate::cloud_provider::kubernetes::KubernetesNode;

pub struct Node {
    instance_type: String,
}

impl Node {
    /// Number of CPUs and total memory wanted - the right AWS EC2 instance type is found algorithmically
    /// Eg. total_cpu = 1 and total_memory_in_gib = 2 means `t2.small` instance type
    /// BUT total_cpu = 1 and total_memory_in_gib = 3 does not have an existing instance - so we will pick the upper closest,
    /// which is `t2.medium` with 2 cpu and 4 GiB
    /// ```
    /// use qovery_engine::cloud_provider::aws::kubernetes::node::Node;
    /// use qovery_engine::cloud_provider::kubernetes::KubernetesNode;
    ///
    /// let node = Node::new_with_cpu_and_mem(2, 4);
    /// assert_eq!(node.instance_type(), "t2.medium")
    /// ```
    pub fn new_with_cpu_and_mem(total_cpu: u8, total_memory_in_gib: u16) -> Self {
        let instance_types_table = [
            (1, 1, "t2.micro"),
            (1, 2, "t2.small"),
            (2, 4, "t2.medium"),
            (2, 8, "t2.large"),
            (4, 16, "t2.xlarge"),
            (8, 32, "t2.2xlarge"),
            // TODO add other instance types
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

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::node::Node;
    use crate::cloud_provider::kubernetes::KubernetesNode;

    #[test]
    fn test_instance_types() {
        assert_eq!(Node::new_with_cpu_and_mem(0, 0).instance_type(), "t2.micro");
        assert_eq!(Node::new_with_cpu_and_mem(1, 0).instance_type(), "t2.micro");
        assert_eq!(Node::new_with_cpu_and_mem(0, 1).instance_type(), "t2.micro");
        assert_eq!(Node::new_with_cpu_and_mem(1, 1).instance_type(), "t2.micro");
        assert_eq!(Node::new_with_cpu_and_mem(1, 2).instance_type(), "t2.small");
        assert_eq!(Node::new_with_cpu_and_mem(2, 4).instance_type(), "t2.medium");
        assert_eq!(Node::new_with_cpu_and_mem(2, 5).instance_type(), "t2.large");
        assert_eq!(Node::new_with_cpu_and_mem(1, 6).instance_type(), "t2.large");
        assert_eq!(Node::new_with_cpu_and_mem(1, 7).instance_type(), "t2.large");
        assert_eq!(Node::new_with_cpu_and_mem(2, 8).instance_type(), "t2.large");
        assert_eq!(Node::new_with_cpu_and_mem(3, 8).instance_type(), "t2.large");
        assert_eq!(Node::new_with_cpu_and_mem(3, 10).instance_type(), "t2.xlarge");
        assert_eq!(Node::new_with_cpu_and_mem(3, 12).instance_type(), "t2.xlarge");
        assert_eq!(Node::new_with_cpu_and_mem(4, 16).instance_type(), "t2.xlarge");
        assert_eq!(Node::new_with_cpu_and_mem(4, 17).instance_type(), "t2.2xlarge");
        assert_eq!(Node::new_with_cpu_and_mem(8, 32).instance_type(), "t2.2xlarge");
        assert_eq!(Node::new_with_cpu_and_mem(16, 64).instance_type(), "t2.2xlarge");
    }
}
