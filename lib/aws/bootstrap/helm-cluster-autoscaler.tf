resource "aws_iam_role" "iam_eks_cluster_autoscaler" {
  name        = "qovery-clustauto-${var.kubernetes_cluster_id}"
  description = "Cluster auto-scaler role for EKS cluster ${var.kubernetes_cluster_id}"
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
        "StringEquals": {
          "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:sub": "system:serviceaccount:kube-system:cluster-autoscaler-aws-cluster-autoscaler"
        }
      }
    }
  ]
}
POLICY
}

resource "aws_iam_policy" "cluster_autoscaler_policy" {
  name = aws_iam_role.iam_eks_cluster_autoscaler.name
  description = "Policy for cluster autoscaler"

  policy = <<POLICY
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "autoscaling:DescribeAutoScalingGroups",
        "autoscaling:DescribeAutoScalingInstances",
        "autoscaling:DescribeLaunchConfigurations",
        "autoscaling:DescribeScalingActivities",
        "autoscaling:DescribeTags",
        "ec2:DescribeInstanceTypes",
        "ec2:DescribeLaunchTemplateVersions"
      ],
      "Resource": ["*"]
    },
    {
      "Effect": "Allow",
      "Action": [
        "autoscaling:SetDesiredCapacity",
        "autoscaling:TerminateInstanceInAutoScalingGroup",
        "ec2:DescribeImages",
        "ec2:GetInstanceTypesFromInstanceRequirements",
        "eks:DescribeNodegroup"
      ],
      "Resource": ["*"]
    }
  ]
}
POLICY
}

# remove this block after migration
resource "aws_iam_user" "iam_eks_cluster_autoscaler" {
  name = "qovery-clustauto-${var.kubernetes_cluster_id}"
  tags = local.tags_eks
}

resource "aws_iam_access_key" "iam_eks_cluster_autoscaler" {
  user = aws_iam_user.iam_eks_cluster_autoscaler.name
}
# end of removal

resource "aws_iam_role_policy_attachment" "cluster_autoscaler_attachment" {
  role       = aws_iam_role.iam_eks_cluster_autoscaler.name
  policy_arn = aws_iam_policy.cluster_autoscaler_policy.arn
}