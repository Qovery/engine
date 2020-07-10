provider "aws" {
  profile    = "default"
  access_key = "{{ aws_access_key }}"
  secret_key = "{{ aws_secret_key }}"
  region     = "{{ aws_region }}"
  version    = "~> 2.63"
}

provider "local" {
  version = "~> 1.4"
}

provider "external" {
  version = "~> 1.2"
}

provider "helm" {
  version = "~> 1.2"
  kubernetes {
    host = aws_eks_cluster.eks_cluster.endpoint
    cluster_ca_certificate = base64decode(aws_eks_cluster.eks_cluster.certificate_authority.0.data)
    load_config_file = false
    exec {
      api_version = "client.authentication.k8s.io/v1alpha1"
      command = "aws-iam-authenticator"
      args = ["token", "-i", var.region_cluster_name]
    }
  }
}