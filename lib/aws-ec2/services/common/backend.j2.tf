terraform {
  backend "kubernetes" {
    secret_suffix    = "{{ tfstate_suffix_name }}"
    load_config_file = true
    config_path      = "{{ kubeconfig_path }}"
    namespace        = "{{ namespace }}"
  }
}
