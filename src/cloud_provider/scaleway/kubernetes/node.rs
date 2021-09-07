use crate::cloud_provider::kubernetes::KubernetesNode;
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
            NodeType::Gp1Xs => "gp1-xs",
            NodeType::Gp1S => "gp1-s",
            NodeType::Gp1M => "gp1-m",
            NodeType::Gp1L => "gp1-l",
            NodeType::Gp1Xl => "gp1-xl",
            NodeType::Dev1M => "dev1-m",
            NodeType::Dev1L => "dev1-l",
            NodeType::Dev1Xl => "dev1-xl",
            NodeType::RenderS => "render-s",
        }
    }
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NodeType::Gp1Xs => write!(f, "gp1-xs"),
            NodeType::Gp1S => write!(f, "gp1-s"),
            NodeType::Gp1M => write!(f, "gp1-m"),
            NodeType::Gp1L => write!(f, "gp1-l"),
            NodeType::Gp1Xl => write!(f, "gp1-xl"),
            NodeType::Dev1M => write!(f, "dev1-m"),
            NodeType::Dev1L => write!(f, "dev1-l"),
            NodeType::Dev1Xl => write!(f, "dev1-xl"),
            NodeType::RenderS => write!(f, "render-s"),
        }
    }
}

impl FromStr for NodeType {
    type Err = ();

    fn from_str(s: &str) -> Result<NodeType, ()> {
        match s {
            "gp1-xs" => Ok(NodeType::Gp1Xs),
            "gp1-s" => Ok(NodeType::Gp1S),
            "gp1-m" => Ok(NodeType::Gp1M),
            "gp1-l" => Ok(NodeType::Gp1L),
            "gp1-xl" => Ok(NodeType::Gp1Xl),
            "dev1-m" => Ok(NodeType::Dev1M),
            "dev1-l" => Ok(NodeType::Dev1L),
            "dev1-xl" => Ok(NodeType::Dev1Xl),
            "render-s" => Ok(NodeType::RenderS),
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
        assert_eq!(Node::new(NodeType::Dev1M).instance_type(), "dev1-m");
        assert_eq!(Node::new(NodeType::Dev1L).instance_type(), "dev1-l");
        assert_eq!(Node::new(NodeType::Dev1Xl).instance_type(), "dev1-xl");
        assert_eq!(Node::new(NodeType::Gp1Xs).instance_type(), "gp1-xs");
        assert_eq!(Node::new(NodeType::Gp1S).instance_type(), "gp1-s");
        assert_eq!(Node::new(NodeType::Gp1M).instance_type(), "gp1-m");
        assert_eq!(Node::new(NodeType::Gp1L).instance_type(), "gp1-l");
        assert_eq!(Node::new(NodeType::Gp1Xl).instance_type(), "gp1-xl");
        assert_eq!(Node::new(NodeType::RenderS).instance_type(), "render-s");
    }
}
