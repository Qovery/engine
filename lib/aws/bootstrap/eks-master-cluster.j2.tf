data "aws_subnet_ids" "eks_subnet_ids" {
  vpc_id = aws_vpc.eks.id
  tags = aws_vpc.eks.tags
  depends_on = [
    aws_subnet.eks-zone-a-{{ eks_zone_a_subnet_blocks|length }},
    aws_subnet.eks-zone-b-{{ eks_zone_a_subnet_blocks|length }},
    aws_subnet.eks-zone-c-{{ eks_zone_a_subnet_blocks|length }},
  ]
}

resource "aws_cloudwatch_log_group" "eks_cluster_logs" {
  name              = "/aws/eks/${var.region_cluster_name}/cluster"
  retention_in_days = 7

  tags = {
    Name = "eks-${var.region_cluster_name}"
    ClusterName = var.cluster_name
    Region = var.region
  }
}

resource "aws_eks_cluster" "eks_cluster" {
  name            = var.region_cluster_name
  role_arn        = aws_iam_role.eks_cluster.arn
  version         = var.k8s_versions.masters

  enabled_cluster_log_types = ["api","audit","authenticator","controllerManager","scheduler"]

  vpc_config {
    security_group_ids = [aws_security_group.eks_cluster.id]
    subnet_ids         = flatten([data.aws_subnet_ids.eks_subnet_ids.*.id])
  }

  depends_on = [
    aws_iam_role_policy_attachment.eks_cluster-AmazonEKSClusterPolicy,
    aws_iam_role_policy_attachment.eks_cluster-AmazonEKSServicePolicy,
    aws_cloudwatch_log_group.eks_cluster_logs,
  ]
}