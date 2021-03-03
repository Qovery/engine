# Qovery

variable "cloud_provider" {
  description = "Cloud provider name"
  default = "do"
  type = string
}

variable "region" {
  description = "AWS region to store terraform state and lock"
  default     = "{{ do_region }}"
  type        = string
}

variable "organization_id" {
  description = "Qovery Organization ID"
  default     = "{{ organization_id }}"
  type        = string
}

variable "qovery_nats_url" {
  description = "URL of qovery nats server"
  default = "{{ qovery_nats_url }}"
  type = string
}

variable "qovery_nats_user" {
  description = "user of qovery nats server"
  default = "{{ qovery_nats_user }}"
  type = string
}

variable "qovery_nats_password" {
  description = "password of qovery nats server"
  default = "{{ qovery_nats_password }}"
  type = string
}

variable "test_cluster" {
  description = "Is this a test cluster?"
  default = "{{ test_cluster }}"
  type = string
}

# Digital Ocean Specific

variable "vpc_name" {
  description = "VPC name, should be unique"
  default = "{{ vpc_name }}"
  type = string
}

# Kubernetes

variable "cidr_block" {
  description = "CIDR block for VPC segmentation"
  default = "{{ vpc_cidr_block }}"
  type = string
}

variable "kubernetes_cluster_id" {
  description = "Kubernetes cluster name"
  default     = "{{ doks_cluster_id }}"
  type        = string
}

variable "kubernetes_cluster_name" {
  description = "Kubernetes cluster name"
  default     = "qovery-{{ doks_master_name }}"
  type        = string
}

variable "doks_version" {
  description = "Kubernetes cluster version"
  default = "{{ doks_version }}"
  type    = string
}

# kubernetes WORKER second cluster

variable "doks_pool_name" {
  default = "{{ doks_master_name }}"
  type    = string
}

variable "doks_pool_autoscale" {
  description = "Enable built-in cluster autoscaler"
  default = true
  type    = bool
}

# Space bucket

variable "space_bucket_kubeconfig" {
  description = "Space bucket containing kubeconfigs"
  default = "{{ space_bucket_kubeconfig }}"
  type = string
}

variable "space_access_id" {
  description = "credentials space access key"
  default = "{{ spaces_access_id }}"
  type = string
}

variable "space_secret_key" {
  description = "credentials space access key"
  default = "{{ spaces_secret_key }}"
  type = string
}

# Engine info

variable "qovery_engine_info" {
  description = "Qovery engine info"
  default = {
    "token" = "{{ engine_version_controller_token }}"
    "api_fqdn" = "{{ qovery_api_url }}"
  }
  type = map(string)
}

variable "qovery_engine_replicas" {
  description = "This variable is used to get random ID generated for the engine"
  default = "2"
  type = number
}

# Agent info

variable "qovery_agent_info" {
  description = "Qovery agent info"
  default = {
    "token" = "{{ agent_version_controller_token }}"
    "api_fqdn" = "{{ qovery_api_url }}"
  }
  type = map(string)
}

variable "qovery_agent_replicas" {
  description = "This variable is used to get random ID generated for the agent"
  default = "1"
  type = number
}

# Helm alert manager discord

variable "discord_api_key" {
  description = "discord url with token for used for alerting"
  default = "{{ discord_api_key }}"
  type = string
}
