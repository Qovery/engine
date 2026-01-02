data "aws_caller_identity" "current" {}

## Karpenter Node Role

resource "aws_iam_role" "karpenter_node_role" {
  name = "KarpenterNodeRole-${var.kubernetes_cluster_name}"
  assume_role_policy = jsonencode(
    {
      "Version" : "2012-10-17",
      "Statement" : [
        {
          "Effect" : "Allow",
          "Principal" : {
            "Service" : "ec2.amazonaws.com"
          },
          "Action" : "sts:AssumeRole"
        }
      ]
    }
  )
  tags = local.tags_eks
}

{% if enable_karpenter -%}
resource "aws_eks_access_entry" "qovery_karpenter_access_entry" {
  cluster_name  = aws_eks_cluster.eks_cluster.name
  principal_arn = aws_iam_role.karpenter_node_role.arn
  type          = "EC2_LINUX"
}
{% endif -%}

resource "aws_iam_instance_profile" "karpenter_instance_profile" {
  name = "KarpenterNodeInstanceProfile-${var.kubernetes_cluster_name}"
  role = aws_iam_role.karpenter_node_role.name
}


resource "aws_iam_role_policy_attachment" "karpenter_eks_worker_policy_node" {
  role       = aws_iam_role.karpenter_node_role.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy"
}

resource "aws_iam_role_policy_attachment" "karpenter_eks_cni_policy" {
  role       = aws_iam_role.karpenter_node_role.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy"
}

resource "aws_iam_role_policy_attachment" "karpenter_ec2_container_registry_read_only" {
  role       = aws_iam_role.karpenter_node_role.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly"
}

resource "aws_iam_role_policy_attachment" "karpenter_ssm_managed_instance_core" {
  role       = aws_iam_role.karpenter_node_role.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

## Karpenter Controller Role

resource "aws_iam_role" "karpenter_controller_role" {
  name        = "KarpenterControllerRole-${var.kubernetes_cluster_name}"
  description = "IAM Role for Karpenter Controller (pod) to assume"

  assume_role_policy = jsonencode(
    {
      "Version" : "2012-10-17",
      "Statement" : [
        {
          "Effect" : "Allow",
          "Principal" : {
            "Federated" : "arn:aws:iam::${data.aws_caller_identity.current.account_id}:oidc-provider/${aws_iam_openid_connect_provider.oidc.url}"
          },
          "Action" : "sts:AssumeRoleWithWebIdentity",
          "Condition" : {
            "StringEquals" : {
              "${aws_iam_openid_connect_provider.oidc.url}:aud" : "sts.amazonaws.com",
              "${aws_iam_openid_connect_provider.oidc.url}:sub" : "system:serviceaccount:kube-system:karpenter"
            }
          }
        }
      ]
    }
  )
  tags = local.tags_eks
}

resource "aws_iam_role_policy" "karpenter_controller" {
  name = aws_iam_role.karpenter_controller_role.name
  role = aws_iam_role.karpenter_controller_role.name
  policy = jsonencode(
    {
      "Statement" : [
        {
          "Action" : [
            "ssm:GetParameter",
            "ec2:DescribeImages",
            "ec2:RunInstances",
            "ec2:DescribeSubnets",
            "ec2:DescribeSecurityGroups",
            "ec2:DescribeLaunchTemplates",
            "ec2:DescribeInstances",
            "ec2:DescribeInstanceTypes",
            "ec2:DescribeInstanceTypeOfferings",
            "ec2:DescribeAvailabilityZones",
            "ec2:DeleteLaunchTemplate",
            "ec2:CreateTags",
            "ec2:CreateLaunchTemplate",
            "ec2:CreateFleet",
            "ec2:DescribeSpotPriceHistory",
            "pricing:GetProducts"
          ],
          "Effect" : "Allow",
          "Resource" : "*",
          "Sid" : "Karpenter"
        },
        {
          "Action" : "ec2:TerminateInstances",
          "Condition" : {
            "StringLike" : {
              "ec2:ResourceTag/karpenter.sh/nodepool" : "*"
            }
          },
          "Effect" : "Allow",
          "Resource" : "*",
          "Sid" : "ConditionalEC2Termination"
        },
        {
          "Effect" : "Allow",
          "Action" : "iam:PassRole",
          "Resource" : "arn:aws:iam::${data.aws_caller_identity.current.account_id}:role/${aws_iam_role.karpenter_node_role.name}",
          "Sid" : "PassNodeIAMRole"
        },
        {
          "Effect" : "Allow",
          "Action" : "eks:DescribeCluster",
          "Resource" : "arn:aws:eks:${var.region}:${data.aws_caller_identity.current.account_id}:cluster/${var.kubernetes_cluster_name}",
          "Sid" : "EKSClusterEndpointLookup"
        },
        {
          "Sid" : "AllowScopedInstanceProfileCreationActions",
          "Effect" : "Allow",
          "Resource" : "*",
          "Action" : [
            "iam:CreateInstanceProfile"
          ],
          "Condition" : {
            "StringEquals" : {
              "aws:RequestTag/kubernetes.io/cluster/${var.kubernetes_cluster_name}" : "owned",
              "aws:RequestTag/topology.kubernetes.io/region" : "${var.region}"
            },
            "StringLike" : {
              "aws:RequestTag/karpenter.k8s.aws/ec2nodeclass" : "*"
            }
          }
        },
        {
          "Sid" : "AllowScopedInstanceProfileTagActions",
          "Effect" : "Allow",
          "Resource" : "*",
          "Action" : [
            "iam:TagInstanceProfile"
          ],
          "Condition" : {
            "StringEquals" : {
              "aws:ResourceTag/kubernetes.io/cluster/${var.kubernetes_cluster_name}" : "owned",
              "aws:ResourceTag/topology.kubernetes.io/region" : "${var.region}",
              "aws:RequestTag/kubernetes.io/cluster/${var.kubernetes_cluster_name}" : "owned",
              "aws:RequestTag/topology.kubernetes.io/region" : "${var.region}"
            },
            "StringLike" : {
              "aws:ResourceTag/karpenter.k8s.aws/ec2nodeclass" : "*",
              "aws:RequestTag/karpenter.k8s.aws/ec2nodeclass" : "*"
            }
          }
        },
        {
          "Sid" : "AllowScopedInstanceProfileActions",
          "Effect" : "Allow",
          "Resource" : "*",
          "Action" : [
            "iam:AddRoleToInstanceProfile",
            "iam:RemoveRoleFromInstanceProfile",
            "iam:DeleteInstanceProfile"
          ],
          "Condition" : {
            "StringEquals" : {
              "aws:ResourceTag/kubernetes.io/cluster/${var.kubernetes_cluster_name}" : "owned",
              "aws:ResourceTag/topology.kubernetes.io/region" : "${var.region}"
            },
            "StringLike" : {
              "aws:ResourceTag/karpenter.k8s.aws/ec2nodeclass" : "*"
            }
          }
        },
        {
          "Sid" : "AllowInstanceProfileReadActions",
          "Effect" : "Allow",
          "Resource" : "*",
          "Action" : "iam:GetInstanceProfile"
        }
        {% if enable_karpenter %}
        ,{
          "Action": [
            "sqs:DeleteMessage",
            "sqs:GetQueueUrl",
            "sqs:ReceiveMessage"
          ],
          "Effect": "Allow",
          "Resource": aws_sqs_queue.qovery-eks-queue.arn
          "Sid": "AllowInterruptionQueueActions"
        }
        {% endif %}
      ],
      "Version" : "2012-10-17"
    }
  )
}