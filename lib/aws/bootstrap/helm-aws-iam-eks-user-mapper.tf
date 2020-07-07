resource "aws_iam_user" "iam-eks-user-mapper" {
  name = "aws-iam-eks-user-mapper-${var.region_cluster_name}"
}

resource "aws_iam_access_key" "iam-eks-user-mapper" {
  user    = aws_iam_user.iam-eks-user-mapper.name
}

resource "aws_iam_user_policy" "iam-eks-user-mapper" {
  name = aws_iam_user.iam-eks-user-mapper.name
  user = aws_iam_user.iam-eks-user-mapper.name

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

resource "helm_release" "iam-eks-user-mapper" {
  name = "iam-eks-user-mapper"
  chart = "../../../lib/aws/charts/iam-eks-user-mapper"
  namespace = "kube-system"
  atomic = true
  max_history = 50

  set {
    name = "aws.accessKey"
    value = aws_iam_access_key.iam-eks-user-mapper.id
  }

  set {
    name = "aws.secretKey"
    value = aws_iam_access_key.iam-eks-user-mapper.secret
  }

  set {
    name = "aws.region"
    value = var.region
  }

  set {
    name = "syncIamGroup"
    value = "Admins"
  }

  depends_on = [
    aws_eks_cluster.eks_cluster,
  ]
}