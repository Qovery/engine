terraform {
  required_providers {
    scaleway = {
      source = "scaleway/scaleway"
    }
  }
  required_version = ">= 0.13"
}


# TODO: use explicit values once tests are over, for the time being, values are injected via ENV
# CF: https://registry.terraform.io/providers/scaleway/scaleway/latest/docs#authentication
provider "scaleway" {}
# provider "scaleway" {
#   access_key      = "{{ scw_access_key }}"
#   secret_key      = "{{ scw_secret_key }}"
#   project_id	    = "{{ scw_default_project_id }}"
#   zone            = "{{ scw_default_zone }}"
#   region          = "{{ scw_default_region }}"
# }

provider "kubernetes" {
  load_config_file = "false"

  host  = null_resource.kubeconfig.triggers.host
  token = null_resource.kubeconfig.triggers.token
  cluster_ca_certificate = base64decode(
    null_resource.kubeconfig.triggers.cluster_ca_certificate
  )
}
