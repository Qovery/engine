{% for eks_worker_node in eks_worker_nodes %}
resource "aws_eks_node_group" "eks_cluster_workers_{{ loop.index }}" {
  cluster_name     = aws_eks_cluster.eks_cluster.name
  version          = var.eks_k8s_versions.workers
  node_group_name  = "qovery-${var.kubernetes_cluster_id}-{{ loop.index }}"
  node_role_arn    = aws_iam_role.eks_workers.arn
  subnet_ids       = flatten([aws_subnet.eks_zone_a[*].id, aws_subnet.eks_zone_b[*].id, aws_subnet.eks_zone_c[*].id])
  instance_types   = ["{{ eks_worker_node.instance_type }}"]
  ami_type         = "AL2_x86_64"


  tags = local.tags_eks

  scaling_config {
    desired_size = "{{ eks_worker_node.desired_size }}"
    max_size     = "{{ eks_worker_node.max_size }}"
    min_size     = "{{ eks_worker_node.min_size }}"
  }

  remote_access {
    ec2_ssh_key = var.ec2_ssh_default_key.key_name
    source_security_group_ids = [aws_security_group.eks_cluster_workers.id]
  }

  timeouts {
    create = "60m"
    delete = "60m"
    update = "60m"
  }

  lifecycle {
    // don't update the desired size and let the cluster-autoscaler do the job
    ignore_changes = [scaling_config[0].desired_size]
  }

  # Ensure that IAM Role permissions are created before and deleted after EKS Node Group handling.
  # Otherwise, EKS will not be able to properly delete EC2 Instances and Elastic Network Interfaces.
  depends_on = [
    aws_iam_role_policy_attachment.node_AmazonEKSWorkerNodePolicy,
    aws_iam_role_policy_attachment.node_AmazonEKS_CNI_Policy,
    aws_iam_role_policy_attachment.node_AmazonEC2ContainerRegistryReadOnly,
    aws_eks_cluster.eks_cluster,
  ]
}
{% endfor %}
