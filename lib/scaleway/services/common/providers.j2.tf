terraform {
  required_providers {
    scaleway = {
      source = "scaleway/scaleway"
      version = "~> 2.1.0"
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
  required_version = ">= 0.13"
}

provider "scaleway" {
  access_key      = "{{ scaleway_access_key }}"
  secret_key      = "{{ scaleway_secret_key }}"
  project_id	  = "{{ scaleway_project_id }}"
  zone            = "{{ zone }}"
  region          = "{{ region }}"
}

data "scaleway_k8s_cluster" "kubernetes_cluster" {
  name = "qovery-{{kubernetes_cluster_id}}"
}

provider "helm" {
  kubernetes {
    host = data.scaleway_k8s_cluster.kubernetes_cluster.apiserver_url
    cluster_ca_certificate = base64decode(data.scaleway_k8s_cluster.kubernetes_cluster.kubeconfig.cluster_ca_certificate)
    load_config_file = false
  }
}
