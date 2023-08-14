{% for eks_worker_node in eks_worker_nodes %}

resource "aws_launch_template" "eks_workers_nodes_{{ loop.index }}" {
  metadata_options {
    http_endpoint = "enabled"
    http_tokens = var.ec2_metadata_imds_version
    # https://github.com/kubernetes/autoscaler/issues/3592
    http_put_response_hop_limit = 2
  }

  block_device_mappings {
    device_name = "/dev/xvda"

    ebs {
      volume_size = {{ eks_worker_node.disk_size_in_gib }}
    }
  }

  tags = local.tags_eks
}

resource "aws_eks_node_group" "eks_cluster_workers_{{ loop.index }}" {
  cluster_name           = aws_eks_cluster.eks_cluster.name
  version                = var.eks_k8s_versions.workers
  node_role_arn          = aws_iam_role.eks_workers.arn
  node_group_name_prefix = "qovery-"
  {% if user_provided_network -%}
  subnet_ids       = flatten([data.aws_subnet.eks_zone_a[*].id, data.aws_subnet.eks_zone_b[*].id, data.aws_subnet.eks_zone_c[*].id])
  {%- else -%}
  subnet_ids       = flatten([aws_subnet.eks_zone_a[*].id, aws_subnet.eks_zone_b[*].id, aws_subnet.eks_zone_c[*].id])
  {%- endif %}
  instance_types   = ["{{ eks_worker_node.instance_type }}"]
  {% if eks_worker_node.instance_architecture == "ARM64" -%}
  ami_type         = "AL2_ARM_64"
  {%- else -%}
  ami_type         = "AL2_x86_64"
  {%- endif %}

  tags = merge(
  local.tags_eks,
  {
    "QoveryNodeGroupId" = "${var.kubernetes_cluster_id}-{{ loop.index }}"
    "QoveryNodeGroupName" = "{{ eks_worker_node.name }}"
  }
  )

  launch_template {
    id      = aws_launch_template.eks_workers_nodes_{{ loop.index }}.id
    version = aws_launch_template.eks_workers_nodes_{{ loop.index }}.latest_version
  }

  scaling_config {
    desired_size = "{{ eks_worker_node.desired_size }}"
    max_size     = "{{ eks_worker_node.max_nodes }}"
    min_size     = "{{ eks_worker_node.min_nodes }}"
  }

  lifecycle {
    // don't update the desired size and let the cluster-autoscaler do the job
    {% if not eks_worker_node.enable_desired_size %}
    ignore_changes = [scaling_config[0].desired_size]
    {% endif %}
    create_before_destroy = true
  }

  update_config {
    max_unavailable_percentage = 10
  }

  timeouts {
    create = "60m"
    delete = "{{ eks_upgrade_timeout_in_min }}m"
    update = "60m"
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


// We suspend AZ Rebalance because it is hard terminating EC2/nodes of the cluster without gracefully draining the nodes.
// https://github.com/terraform-aws-modules/terraform-aws-eks/pull/369
// We use local-exec provisioner because recreating the ASG from the aws_eks_node_group is too flaky to be reliable.
resource "null_resource" "autoscaling_suspend_workers_nodes_{{ loop.index }}" {
  triggers = {
    always_run = "${timestamp()}"
  }

  provisioner "local-exec" {
    command = "aws autoscaling suspend-processes --auto-scaling-group-name ${flatten(aws_eks_node_group.eks_cluster_workers_{{ loop.index }}.resources[*].autoscaling_groups[0].name)[0]} --scaling-processes AZRebalance"
  }

  depends_on = [
    aws_eks_node_group.eks_cluster_workers_{{ loop.index }}
  ]
}
{% endfor %}
