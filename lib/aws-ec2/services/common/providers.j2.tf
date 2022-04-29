terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version    = "~> 3.36.0"
    }
    helm = {
      source = "hashicorp/helm"
      version = "~> 1.3.2"
    }
    local = {
      source = "hashicorp/local"
      version = "~> 1.4"
    }
    time = {
      source  = "hashicorp/time"
      version = "~> 0.3"
    }
  }
  required_version = ">= 0.14"
}

provider "aws" {
  profile    = "default"
  region     = "{{ region }}"
  access_key = "{{ aws_access_key }}"
  secret_key = "{{ aws_secret_key }}"
}

data aws_instance eks_cluster {
  name = "qovery-{{kubernetes_cluster_id}}"
}

provider "helm" {
  kubernetes {
    host = data.aws_instance.eks_cluster.endpoint
    cluster_ca_certificate = base64decode(data.aws_instance.eks_cluster.certificate_authority.0.data)
    load_config_file = false
    exec {
      api_version = "client.authentication.k8s.io/v1alpha1"
      command = "aws-iam-authenticator"
      args = ["token", "-i", "qovery-{{kubernetes_cluster_id}}"]
      env = {
        AWS_ACCESS_KEY_ID = "{{ aws_access_key }}"
        AWS_SECRET_ACCESS_KEY = "{{ aws_secret_key }}"
        AWS_DEFAULT_REGION = "{{ region }}"
      }
    }
  }
}

resource "time_static" "on_db_create" {}
