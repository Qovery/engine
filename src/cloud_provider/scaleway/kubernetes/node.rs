use crate::cloud_provider::kubernetes::KubernetesNode;
use itertools::Itertools;
use std::any::Any;
use std::fmt;
use std::str::FromStr;

#[derive(Clone)]
pub enum NodeType {
    Gp1Xs,   // 4 cores 16 Go RAM
    Gp1S,    // 8 cores 32 Go RAM
    Gp1M,    // 16 cores 64 Go RAM
    Gp1L,    // 32 cores 128 Go RAM
    Gp1Xl,   // 64 cores 256 Go RAM
    Dev1M,   // 3 cores 4 Go RAM
    Dev1L,   // 4 cores 8 Go RAM
    Dev1Xl,  // 4 cores 12 Go RAM
    RenderS, // 10 cores 45 Go RAM 1 GPU 1 Go VRAM
}

impl NodeType {
    pub fn as_str(&self) -> &str {
        match self {
            NodeType::Gp1Xs => "GP1-XS",
            NodeType::Gp1S => "GP1-S",
            NodeType::Gp1M => "GP1-M",
            NodeType::Gp1L => "GP1-L",
            NodeType::Gp1Xl => "GP1-XL",
            NodeType::Dev1M => "DEV1-M",
            NodeType::Dev1L => "DEV1-L",
            NodeType::Dev1Xl => "DEV1-XL",
            NodeType::RenderS => "RENDER-S",
        }
    }
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NodeType::Gp1Xs => write!(f, "GP1-XS"),
            NodeType::Gp1S => write!(f, "GP1-S"),
            NodeType::Gp1M => write!(f, "GP1-M"),
            NodeType::Gp1L => write!(f, "GP1-L"),
            NodeType::Gp1Xl => write!(f, "GP1-XL"),
            NodeType::Dev1M => write!(f, "DEV1-M"),
            NodeType::Dev1L => write!(f, "DEV1-L"),
            NodeType::Dev1Xl => write!(f, "DEV1-XL"),
            NodeType::RenderS => write!(f, "RENDER-S"),
        }
    }
}

impl FromStr for NodeType {
    type Err = ();

    fn from_str(s: &str) -> Result<NodeType, ()> {
        match s {
            "GP1-XS" => Ok(NodeType::Gp1Xs),
            "GP1-S" => Ok(NodeType::Gp1S),
            "GP1-M" => Ok(NodeType::Gp1M),
            "GP1-L" => Ok(NodeType::Gp1L),
            "GP1-XL" => Ok(NodeType::Gp1Xl),
            "DEV1-M" => Ok(NodeType::Dev1M),
            "DEV1-L" => Ok(NodeType::Dev1L),
            "DEV1-XL" => Ok(NodeType::Dev1Xl),
            "RENDER-S" => Ok(NodeType::RenderS),
            _ => Err(()),
        }
    }
}

#[derive(Clone)]
pub struct Node {
    node_type: NodeType,
}

impl Node {
    pub fn new(node_type: NodeType) -> Node {
        Node {
            node_type: node_type.clone(),
        }
    }
}

impl KubernetesNode for Node {
    fn instance_type(&self) -> &str {
        self.node_type.as_str()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::kubernetes::KubernetesNode;
    use crate::cloud_provider::scaleway::kubernetes::node::{Node, NodeType};

    #[test]
    fn test_node_types() {
        assert_eq!(Node::new(NodeType::Dev1M).instance_type(), "DEV1-M");
        assert_eq!(Node::new(NodeType::Dev1L).instance_type(), "DEV1-L");
        assert_eq!(Node::new(NodeType::Dev1Xl).instance_type(), "DEV1-XL");
        assert_eq!(Node::new(NodeType::Gp1Xs).instance_type(), "GP1-XS");
        assert_eq!(Node::new(NodeType::Gp1S).instance_type(), "GP1-S");
        assert_eq!(Node::new(NodeType::Gp1M).instance_type(), "GP1-M");
        assert_eq!(Node::new(NodeType::Gp1L).instance_type(), "GP1-L");
        assert_eq!(Node::new(NodeType::Gp1Xl).instance_type(), "GP1-XL");
        assert_eq!(Node::new(NodeType::RenderS).instance_type(), "RENDER-S");
    }
}
