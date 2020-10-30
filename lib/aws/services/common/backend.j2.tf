terraform {
  backend "kubernetes" {
    secret_suffix    = "{{ namespace }}-state"
    load_config_file = true
    config_path      = "{{ kubeconfig_path }}"
    namespace        = "{{ namespace }}"
    exec {
      api_version = "client.authentication.k8s.io/v1alpha1"
      command     = "aws-iam-authenticator"
      args = [
        "token",
        "-i",
        "qovery-{{eks_cluster_id}}"]
      env = {
        AWS_ACCESS_KEY_ID     = "{{ aws_access_key }}"
        AWS_SECRET_ACCESS_KEY = "{{ aws_secret_key }}"
        AWS_DEFAULT_REGION    = "{{ region }}"
      }
    }
  }
}