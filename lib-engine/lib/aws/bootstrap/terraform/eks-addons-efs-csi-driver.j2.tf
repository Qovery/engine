{% if eks_efs_addon_enabled -%}
# =============================================================================
# EFS CSI Driver EKS Add-on
# =============================================================================

resource "aws_eks_addon" "aws_efs_csi_driver" {
  cluster_name             = aws_eks_cluster.eks_cluster.name
  addon_name               = "aws-efs-csi-driver"
  service_account_role_arn = aws_iam_role.efs_csi_irsa_role.arn

  addon_version               = "{{ eks_addon_efs_csi.version }}"
  resolve_conflicts_on_update = "OVERWRITE"
  resolve_conflicts_on_create = "OVERWRITE"

  tags = local.tags_eks

  configuration_values = jsonencode({
    controller = {
      tolerations = [
        {
          key    = "node.qovery.com/infrastructure"
          value  = "true"
          effect = "NoSchedule"
        }
      ]
    }
  })

  depends_on = [aws_efs_mount_target.efs_zone_a, aws_efs_mount_target.efs_zone_b, aws_efs_mount_target.efs_zone_c]
}

# =============================================================================
# EFS File System
# =============================================================================

resource "aws_efs_file_system" "efs" {
  creation_token   = "qovery-${var.kubernetes_cluster_id}"
  encrypted        = true
  throughput_mode   = "{{ eks_efs_throughput_mode }}"
  performance_mode  = "{{ eks_efs_performance_mode }}"

{% if eks_efs_transition_to_ia != "" -%}
  lifecycle_policy {
    transition_to_ia = "{{ eks_efs_transition_to_ia }}"
  }
{% endif -%}

  tags = merge(
    local.tags_eks,
    {
      Name = "qovery-${var.kubernetes_cluster_id}-efs"
    }
  )
}

# =============================================================================
# EFS Security Group — allows NFS from the VPC CIDR
# =============================================================================

resource "aws_security_group" "efs" {
  name        = "qovery-${var.kubernetes_cluster_id}-efs"
  description = "Allow NFS traffic from the VPC for EFS"
  vpc_id      = aws_vpc.eks.id
  tags        = local.tags_eks

  ingress {
    description = "NFS from VPC"
    from_port   = 2049
    to_port     = 2049
    protocol    = "tcp"
    cidr_blocks = [var.vpc_cidr_block]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

# =============================================================================
# EFS Mount Targets — one per private EKS subnet / AZ
# =============================================================================

resource "aws_efs_mount_target" "efs_zone_a" {
  for_each = toset(aws_subnet.eks_zone_a[*].id)

  file_system_id  = aws_efs_file_system.efs.id
  subnet_id       = each.value
  security_groups = [aws_security_group.efs.id]
}

resource "aws_efs_mount_target" "efs_zone_b" {
  for_each = toset(aws_subnet.eks_zone_b[*].id)

  file_system_id  = aws_efs_file_system.efs.id
  subnet_id       = each.value
  security_groups = [aws_security_group.efs.id]
}

resource "aws_efs_mount_target" "efs_zone_c" {
  for_each = toset(aws_subnet.eks_zone_c[*].id)

  file_system_id  = aws_efs_file_system.efs.id
  subnet_id       = each.value
  security_groups = [aws_security_group.efs.id]
}

# =============================================================================
# IAM — IRSA role for the EFS CSI driver (controller + node service accounts)
# =============================================================================

resource "aws_iam_role" "efs_csi_irsa_role" {
  name        = "eks-efs-csi-plugin-${var.kubernetes_cluster_id}"
  description = "EFS CSI plugin role for EKS cluster ${var.kubernetes_cluster_id}"
  tags        = local.tags_eks

  assume_role_policy = <<POLICY
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Federated": "${aws_iam_openid_connect_provider.oidc.arn}"
      },
      "Action": "sts:AssumeRoleWithWebIdentity",
      "Condition": {
        "StringLike": {
          "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:sub": "system:serviceaccount:kube-system:efs-csi-*-sa"
        }
      }
    }
  ]
}
POLICY
}

resource "aws_iam_role_policy_attachment" "efs_csi_irsa_policy" {
  role       = aws_iam_role.efs_csi_irsa_role.name
  policy_arn = aws_iam_policy.efs_csi_policy.arn
}

resource "aws_iam_policy" "efs_csi_policy" {
  name        = "AmazonEKS_EFS_CSI_Policy-${var.kubernetes_cluster_id}"
  description = "EFS CSI policy for assume role"

  policy = <<EOF
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "elasticfilesystem:DescribeAccessPoints",
        "elasticfilesystem:DescribeFileSystems",
        "elasticfilesystem:DescribeMountTargets",
        "ec2:DescribeAvailabilityZones"
      ],
      "Resource": "*"
    },
    {
      "Effect": "Allow",
      "Action": [
        "elasticfilesystem:CreateAccessPoint"
      ],
      "Resource": "*",
      "Condition": {
        "StringLike": {
          "aws:RequestTag/efs.csi.aws.com/cluster": "true"
        }
      }
    },
    {
      "Effect": "Allow",
      "Action": [
        "elasticfilesystem:TagResource"
      ],
      "Resource": "*",
      "Condition": {
        "StringLike": {
          "aws:ResourceTag/efs.csi.aws.com/cluster": "true"
        }
      }
    },
    {
      "Effect": "Allow",
      "Action": "elasticfilesystem:DeleteAccessPoint",
      "Resource": "*",
      "Condition": {
        "StringEquals": {
          "aws:ResourceTag/efs.csi.aws.com/cluster": "true"
        }
      }
    }
  ]
}
EOF
}

# =============================================================================
# Outputs
# =============================================================================

output "efs_file_system_id" {
  value       = aws_efs_file_system.efs.id
  description = "EFS file system ID for use in StorageClass configuration"
}
{% endif -%}
