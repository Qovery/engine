provider "aws" {
  alias = "tfstates"
  access_key = "{{ aws_access_key_tfstates_account }}"
  secret_key = "{{ aws_secret_key_tfstates_account }}"
  region = "{{ aws_region_tfstates_account }}"
}

provider "digitalocean" {
  token = "{{ digitalocean_token }}"
}

provider "helm" {
  debug           = true
  kubernetes {
    host = digitalocean_kubernetes_cluster.kubernetes_cluster.endpoint
    client_certificate     = base64decode(digitalocean_kubernetes_cluster.kubernetes_cluster.kube_config.0.client_certificate)
    client_key             = base64decode(digitalocean_kubernetes_cluster.kubernetes_cluster.kube_config.0.client_key)
    cluster_ca_certificate = base64decode(digitalocean_kubernetes_cluster.kubernetes_cluster.kube_config.0.cluster_ca_certificate)
    load_config_file       = false
    token                  = digitalocean_kubernetes_cluster.kubernetes_cluster.kube_config.0.token
  }
}

terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version    = "~> 3.8.0"
    }

    digitalocean = {
      source = "digitalocean/digitalocean"
      version = "~> 1.22.1"
    }
    external = {
      source = "hashicorp/external"
      version = "~> 1.2"
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