locals {
  tags_eks = {
    ClusterId = var.eks_cluster_id,
    ClusterName = var.eks_cluster_name,
    Region = var.region
  }
}

resource "aws_cloudwatch_log_group" "eks_cluster_logs" {
  name = "/aws/eks/${var.eks_cluster_id}/cluster"
  retention_in_days = 7

  tags = local.tags_eks
}

resource "aws_eks_cluster" "eks_cluster" {
  name            = var.eks_cluster_id
  role_arn        = aws_iam_role.eks_cluster.arn
  version         = var.eks_k8s_versions.masters

  enabled_cluster_log_types = ["api","audit","authenticator","controllerManager","scheduler"]

  vpc_config {
    security_group_ids = [aws_security_group.eks_cluster.id]
    subnet_ids = flatten([aws_subnet.eks-zone-a.*.id, aws_subnet.eks-zone-b.*.id,aws_subnet.eks-zone-c.*.id])
  }

  tags = local.tags_eks

  depends_on = [
    aws_iam_role_policy_attachment.eks_cluster-AmazonEKSClusterPolicy,
    aws_iam_role_policy_attachment.eks_cluster-AmazonEKSServicePolicy,
    aws_cloudwatch_log_group.eks_cluster_logs,
  ]
}