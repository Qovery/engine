terraform {
  backend "kubernetes" {
    secret_suffix    = "state"
    load_config_file = true
    config_path = "{{ kubeconfig_path }}"
  }
}