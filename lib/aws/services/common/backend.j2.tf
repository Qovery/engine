terraform {
  backend "kubernetes" {
    secret_suffix    = "{{ namespace }}-state"
    load_config_file = true
    config_path = "{{ kubeconfig_path }}"
    namespace = "{{ namespace }}"
  }
}