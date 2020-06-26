# AWS IAM user

locals {
  kubeconfig = <<KUBECONFIG


apiVersion: v1
clusters:
- cluster:
    server: ${aws_eks_cluster.eks_cluster.endpoint}
    certificate-authority-data: ${aws_eks_cluster.eks_cluster.certificate_authority.0.data}
  name: aws_${replace(var.region_cluster_name, "-", "_")}
contexts:
- context:
    cluster: aws_${replace(var.region_cluster_name, "-", "_")}
    user: aws_${replace(var.region_cluster_name, "-", "_")}
  name: aws_${replace(var.region_cluster_name, "-", "_")}
current-context: aws_${replace(var.region_cluster_name, "-", "_")}
kind: Config
preferences: {}
users:
- name: aws_${replace(var.region_cluster_name, "-", "_")}
  user:
    exec:
      apiVersion: client.authentication.k8s.io/v1alpha1
      command: aws-iam-authenticator
      args:
        - "token"
        - "-i"
        - "${var.region_cluster_name}"
KUBECONFIG
}

resource "local_file" "kubeconfig" {
  filename = "kubeconfig/aws_${var.region_cluster_name}.yaml"
  content = local.kubeconfig
}
