{% for eks_worker_node in eks_worker_nodes %}
resource "aws_eks_node_group" "eks-cluster-workers-{{ loop.index }}" {
  cluster_name    = aws_eks_cluster.eks_cluster.name
  version         = var.k8s_versions.workers
  node_group_name = "{{ region_cluster_id }}-{{ loop.index }}"
  node_role_arn   = aws_iam_role.eks_workers.arn
  subnet_ids      = flatten([aws_subnet.eks-zone-a.*.id, aws_subnet.eks-zone-b.*.id, aws_subnet.eks-zone-c.*.id])
  instance_types  = ["{{ eks_worker_node.instance_type }}"]
  ami_type        = "AL2_x86_64"

  tags = {
    Name = "eks-${var.region_cluster_name}"
    ClusterName = var.cluster_name
    Region = var.region
  }

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

  # Ensure that IAM Role permissions are created before and deleted after EKS Node Group handling.
  # Otherwise, EKS will not be able to properly delete EC2 Instances and Elastic Network Interfaces.
  depends_on = [
    aws_iam_role_policy_attachment.node-AmazonEKSWorkerNodePolicy,
    aws_iam_role_policy_attachment.node-AmazonEKS_CNI_Policy,
    aws_iam_role_policy_attachment.node-AmazonEC2ContainerRegistryReadOnly,
  ]
}
{% endfor %}