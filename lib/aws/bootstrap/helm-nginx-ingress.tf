/*
 Adding a policy to cluster IAM role that allow permissions
 required to create AWSServiceRoleForElasticLoadBalancing service-linked role by EKS during ELB provisioning
 https://github.com/terraform-aws-modules/terraform-aws-eks/issues/183
*/

resource "aws_iam_role_policy" "eks_cluster_ingress_loadbalancer_creation" {
  name   = "ingress-loadbalancer-creation-${var.kubernetes_cluster_id}"
  role       = aws_iam_role.eks_cluster.name

  policy = <<POLICY
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "ec2:DescribeAccountAttributes",
        "ec2:DescribeInternetGateways"
      ],
      "Resource": "*"
    }
  ]
}
POLICY
}

resource "helm_release" "nginx_ingress" {
  name = "nginx-ingress"
  chart = "common/charts/nginx-ingress"
  namespace = "nginx-ingress"
  create_namespace = true
  atomic = true
  max_history = 50

  # Because of NLB, svc can take some time to start
  timeout = 300
  values = [file("chart_values/nginx-ingress.yaml")]

  // make a fake arg to avoid TF to validate update on failure because of the atomic option
  set {
    name = "fake"
    value = timestamp()
  }

  # Controller resources
  set {
    name = "controller.resources.limits.cpu"
    value = "200m"
  }

  set {
    name = "controller.resources.requests.cpu"
    value = "100m"
  }

  set {
    name = "controller.resources.limits.memory"
    value = "768Mi"
  }

  set {
    name = "controller.resources.requests.memory"
    value = "768Mi"
  }

  # Default backend resources
  set {
    name = "defaultBackend.resources.limits.cpu"
    value = "20m"
  }

  set {
    name = "defaultBackend.resources.requests.cpu"
    value = "10m"
  }

  set {
    name = "defaultBackend.resources.limits.memory"
    value = "32Mi"
  }

  set {
    name = "defaultBackend.resources.requests.memory"
    value = "32Mi"
  }

  depends_on = [
    aws_iam_role_policy.eks_cluster_ingress_loadbalancer_creation,
    aws_eks_cluster.eks_cluster,
    helm_release.prometheus_operator,
    helm_release.aws_vpc_cni,
  ]
}
