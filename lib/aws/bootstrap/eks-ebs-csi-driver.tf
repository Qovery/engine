resource "aws_iam_policy" "eks_workers_ebs_csi" {
  name = "qovery-aws-EBS-CSI-Driver-${var.kubernetes_cluster_id}"
  description = "Policy for AWS CSI driver"

  policy = <<POLICY
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "ec2:AttachVolume",
        "ec2:CreateSnapshot",
        "ec2:CreateTags",
        "ec2:CreateVolume",
        "ec2:DeleteSnapshot",
        "ec2:DeleteTags",
        "ec2:DeleteVolume",
        "ec2:DescribeInstances",
        "ec2:DescribeSnapshots",
        "ec2:DescribeTags",
        "ec2:DescribeVolumes",
        "ec2:DetachVolume",
        "ec2:ModifyVolume"
      ],
      "Resource": "*"
    }
  ]
}
POLICY
}

resource "aws_iam_role_policy_attachment" "workers_csi" {
  policy_arn = aws_iam_policy.eks_workers_ebs_csi.arn
  role       = aws_iam_role.eks_workers.name
}
