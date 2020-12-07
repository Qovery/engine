terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version    = "~> 3.8.0"
    }
    external = {
      source = "hashicorp/external"
      version = "~> 1.2"
    }
    kubernetes = {
      source = "hashicorp/kubernetes"
      version = "~> 1.13.3"
    }
    helm = {
      source = "hashicorp/helm"
      version = "~> 1.3.2"
    }
    local = {
      source = "hashicorp/local"
      version = "~> 1.4"
    }
    null = {
      source = "hashicorp/null"
      version = "~> 2.1"
    }
    random = {
      source = "hashicorp/random"
      version = "~> 2.3"
    }
  }
  required_version = ">= 0.13"
}

provider "aws" {
  profile    = "default"
  access_key = "{{ aws_access_key }}"
  secret_key = "{{ aws_secret_key }}"
  region     = "{{ aws_region }}"
}

provider "aws" {
  alias = "tfstates"
  access_key = "{{ aws_access_key_tfstates_account }}"
  secret_key = "{{ aws_secret_key_tfstates_account }}"
  region = "{{ aws_region_tfstates_account }}"
}

provider "kubernetes" {
  host = aws_eks_cluster.eks_cluster.endpoint
  cluster_ca_certificate = base64decode(aws_eks_cluster.eks_cluster.certificate_authority.0.data)
  load_config_file = false
  exec {
    api_version = "client.authentication.k8s.io/v1alpha1"
    command = "aws-iam-authenticator"
    args = ["token", "-i", aws_eks_cluster.eks_cluster.name]
    env = {
      AWS_ACCESS_KEY_ID = "{{ aws_access_key }}"
      AWS_SECRET_ACCESS_KEY = "{{ aws_secret_key }}"
      AWS_DEFAULT_REGION = "{{ aws_region }}"
    }
  }
}

provider "helm" {
  kubernetes {
    host = aws_eks_cluster.eks_cluster.endpoint
    cluster_ca_certificate = base64decode(aws_eks_cluster.eks_cluster.certificate_authority.0.data)
    load_config_file = false
    exec {
      api_version = "client.authentication.k8s.io/v1alpha1"
      command = "aws-iam-authenticator"
      args = ["token", "-i", aws_eks_cluster.eks_cluster.name]
      env = {
        AWS_ACCESS_KEY_ID = "{{ aws_access_key }}"
        AWS_SECRET_ACCESS_KEY = "{{ aws_secret_key }}"
        AWS_DEFAULT_REGION = "{{ aws_region }}"
      }
    }
  }
}