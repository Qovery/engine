##############################
# Worker Node Security Group #
##############################

resource "aws_security_group" "eks_cluster_workers" {
  name        = "qovery-eks-workers-${var.kubernetes_cluster_id}"
  description = "Security group for all nodes in the cluster"
  vpc_id      = {%- if user_provided_network -%} data.aws_vpc.eks.id {%- else -%} aws_vpc.eks.id {%- endif %}

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = merge(
    local.tags_eks,
    {
      Name = "qovery-eks-workers",
      "kubernetes.io/cluster/${var.kubernetes_cluster_id}" = "owned",
    }
  )
}

resource "aws_security_group_rule" "node_ingress_self" {
  description              = "Allow workers to communicate with each other"
  from_port                = 0
  protocol                 = "-1"
  security_group_id        = aws_security_group.eks_cluster_workers.id
  source_security_group_id = aws_security_group.eks_cluster_workers.id
  to_port                  = 65535
  type                     = "ingress"
}

resource "aws_security_group_rule" "node_ingress_cluster" {
  description              = "Allow worker Kubelets and pods to receive communication from the cluster control plane"
  from_port                = 1025
  protocol                 = "tcp"
  security_group_id        = aws_security_group.eks_cluster_workers.id
  source_security_group_id = aws_security_group.eks_cluster.id
  to_port                  = 65535
  type                     = "ingress"
}

############################################
# Worker Node Access to EKS Master Cluster #
############################################

resource "aws_security_group_rule" "cluster_ingress_node_https" {
  description              = "Allow pods to communicate with the cluster API Server"
  from_port                = 443
  protocol                 = "tcp"
  security_group_id        = aws_security_group.eks_cluster.id
  source_security_group_id = aws_security_group.eks_cluster_workers.id
  to_port                  = 443
  type                     = "ingress"
}
