resource "aws_iam_role" "iam_eks_user_mapper" {
  name        = "qovery-aws-iam-eks-user-mapper-${var.kubernetes_cluster_id}"
  description = "AWS IAM EKS user mapper role ${var.kubernetes_cluster_id}"
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
      "Action": ["sts:AssumeRoleWithWebIdentity"],
      "Condition": {
        "StringEquals": {
          "${replace(aws_iam_openid_connect_provider.oidc.url, "https://", "")}:sub": "system:serviceaccount:kube-system:iam-eks-user-mapper"
        }
      }
    },
    {
      "Sid": "AllowRole",
      "Effect": "Allow",
      "Principal": {
        "AWS": "${aws_iam_role.eks_workers.arn}"
      },
      "Action": "sts:AssumeRole"
    }
  ]
}
POLICY
}

resource "aws_iam_policy" "iam_eks_user_mapper_policy" {
  name = aws_iam_role.iam_eks_user_mapper.name
  description = "Policy for AWS IAM EKS user mapper"

  policy = <<EOF
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Effect": "Allow",
            "Action": [
                "iam:GenerateCredentialReport",
                "iam:GenerateServiceLastAccessedDetails",
                "iam:Get*",
                "iam:List*",
                "iam:SimulateCustomPolicy",
                "iam:SimulatePrincipalPolicy"
            ],
            "Resource": "*"
        }
    ]
}
EOF
}

resource "aws_iam_role_policy_attachment" "iam_eks_user_mapper_attachment" {
  role       = aws_iam_role.iam_eks_user_mapper.name
  policy_arn = aws_iam_policy.iam_eks_user_mapper_policy.arn
}