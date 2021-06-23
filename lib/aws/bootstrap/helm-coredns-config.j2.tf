//# On the first boot, it's required to remove the existing CoreDNS config to get it managed by helm
//resource "null_resource" "delete_aws_default_coredns_config" {
//  provisioner "local-exec" {
//    command = <<EOT
//kubectl -n kube-system delete configmap coredns
//EOT
//    environment = {
//      KUBECONFIG = local_file.kubeconfig.filename
//      AWS_ACCESS_KEY_ID = "{{ aws_access_key }}"
//      AWS_SECRET_ACCESS_KEY = "{{ aws_secret_key }}"
//      AWS_DEFAULT_REGION = "{{ aws_region }}"
//    }
//  }
//
//  depends_on = [
//    aws_eks_cluster.eks_cluster,
//    helm_release.aws_vpc_cni,
//  ]
//}
//
//resource "helm_release" "coredns-config" {
//  name = "coredns-config"
//  chart = "charts/coredns-config"
//  namespace = "kube-system"
//  atomic = true
//  max_history = 50
//  force_update = true
//
//  set {
//    name = "managed_dns"
//    value = "{{ managed_dns_domains_terraform_format }}"
//  }
//
//  set {
//    name = "managed_dns_resolvers"
//    value = "{{ managed_dns_resolvers_terraform_format }}"
//  }
//
//  set {
//    name = "forced_upgrade"
//    value = var.forced_upgrade
//  }
//
//  provisioner "local-exec" {
//    command = <<EOT
//kubectl -n kube-system rollout restart deployment coredns
//EOT
//    environment = {
//      KUBECONFIG = local_file.kubeconfig.filename
//      AWS_ACCESS_KEY_ID = "{{ aws_access_key }}"
//      AWS_SECRET_ACCESS_KEY = "{{ aws_secret_key }}"
//      AWS_DEFAULT_REGION = "{{ aws_region }}"
//    }
//  }
//
//  depends_on = [
//    aws_eks_cluster.eks_cluster,
//    helm_release.aws_vpc_cni,
//    null_resource.delete_aws_default_coredns_config
//  ]
//}