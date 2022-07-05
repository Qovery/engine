resource "aws_security_group" "eks_cluster" {
  name        = "qovery-eks-${var.kubernetes_cluster_id}"
  description = "Cluster communication with worker nodes"
  vpc_id      = {%- if user_provided_network -%} data.aws_vpc.eks.id {%- else -%} aws_vpc.eks.id {%- endif %}

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = local.tags_eks
}

# OPTIONAL: Allow inbound traffic from your local workstation external IP
#           to the Kubernetes. You will need to replace A.B.C.D below with
#           your real IP. Services like icanhazip.com can help you find this.
resource "aws_security_group_rule" "cluster_ingress_workstation_https" {
  cidr_blocks       = var.eks_access_cidr_blocks
  description       = "Allow workstation to communicate with the cluster API Server"
  from_port         = 443
  protocol          = "tcp"
  security_group_id = aws_security_group.eks_cluster.id
  to_port           = 443
  type              = "ingress"
}
