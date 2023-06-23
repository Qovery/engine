locals {
  kubeconfig = <<KUBECONFIG
apiVersion: v1
clusters:
- cluster:
    server: ${aws_eks_cluster.eks_cluster.endpoint}
    certificate-authority-data: ${aws_eks_cluster.eks_cluster.certificate_authority.0.data}
  name: aws_${replace(var.kubernetes_cluster_id, "-", "_")}
contexts:
- context:
    cluster: aws_${replace(var.kubernetes_cluster_id, "-", "_")}
    user: aws_${replace(var.kubernetes_cluster_id, "-", "_")}
  name: aws_${replace(var.kubernetes_cluster_id, "-", "_")}
current-context: aws_${replace(var.kubernetes_cluster_id, "-", "_")}
kind: Config
preferences: {}
users:
- name: aws_${replace(var.kubernetes_cluster_id, "-", "_")}
  user:
    exec:
      apiVersion: client.authentication.k8s.io/v1
      command: aws
      args:
        - "eks"
        - "get-token"
        - "--cluster-name"
        - "${aws_eks_cluster.eks_cluster.name}"
KUBECONFIG
}

resource "local_file" "kubeconfig" {
  filename = "{{ s3_kubeconfig_bucket }}/${var.kubernetes_cluster_id}.yaml"
  content = local.kubeconfig
  file_permission = "0644"
  depends_on = [aws_eks_cluster.eks_cluster]
}
