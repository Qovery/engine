resource "aws_iam_user" "iam_eks_cluster_autoscaler" {
  name = "qovery-clustauto-${var.kubernetes_cluster_id}"
  tags = local.tags_eks
}

resource "aws_iam_access_key" "iam_eks_cluster_autoscaler" {
  user    = aws_iam_user.iam_eks_cluster_autoscaler.name
}

resource "aws_iam_policy" "cluster_autoscaler_policy" {
  name = aws_iam_user.iam_eks_cluster_autoscaler.name
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
        "autoscaling:DescribeTags",
        "autoscaling:SetDesiredCapacity",
        "autoscaling:TerminateInstanceInAutoScalingGroup"
      ],
      "Resource": "*"
    }
  ]
}
POLICY
}

resource "aws_iam_user_policy_attachment" "s3_cluster_autoscaler_attachment" {
  user       = aws_iam_user.iam_eks_cluster_autoscaler.name
  policy_arn = aws_iam_policy.cluster_autoscaler_policy.arn
}