# AWS specific
variable "cloud_provider" {
  description = "Cloud provider name"
  default = "aws"
  type = string
}

variable "region" {
  description = "AWS region to store terraform state and lock"
  default     = "{{ aws_region }}"
  type        = string
}

variable "organization_id" {
  description = "Qovery Organization ID"
  default     = "{{ organization_id }}"
  type        = string
}

variable "vpc_cidr_block" {
  description = "VPC CIDR block"
  default = "{{ vpc_cidr_block }}"
  type = string
}

variable "test_cluster" {
  description = "Is this a test cluster?"
  default = "false"
  type = string
}

# Kubernetes

variable "eks_subnets_zone_a" {
  description = "EKS subnets Zone A"
  default = {{ eks_zone_a_subnet_blocks }}
  type = list(string)
}

variable "eks_subnets_zone_b" {
  description = "EKS subnets Zone B"
  default = {{ eks_zone_b_subnet_blocks }}
  type = list(string)
}

variable "eks_subnets_zone_c" {
  description = "EKS subnets Zone C"
  default = {{ eks_zone_c_subnet_blocks }}
  type = list(string)
}

variable "eks_cidr_subnet" {
  description = "EKS CIDR (x.x.x.x/CIDR)"
  default     = {{ eks_cidr_subnet }}
  type        = number
}

variable "eks_k8s_versions" {
  description = "Kubernetes version"
  default = {
    "masters": "{{ eks_masters_version }}",
    "workers": "{{ eks_workers_version }}",
  }
  type = map(string)
}

variable "eks_cluster_id" {
  description = "Kubernetes cluster name with region"
  default     = "{{ eks_cluster_id }}"
  type        = string
}

variable "eks_cluster_name" {
  description = "Kubernetes cluster name"
  default     = "qovery-{{ eks_cluster_name }}"
  type        = string
}

variable "eks_access_cidr_blocks" {
  description = "Kubernetes cluster name"
  default     = {{ eks_access_cidr_blocks }}
  type        = list(string)
}

variable "eks_cloudwatch_log_group" {
  description = "AWS cloudwatch log group for EKS"
  default = "qovery-{{ eks_cloudwatch_log_group }}"
  type = string
}

# S3 bucket name

variable "s3_bucket_kubeconfig" {
  description = "S3 bucket with kubeconfigs"
  default = "{{ s3_kubeconfig_bucket }}"
  type = string
}

variable "s3_bucket_qengine_resources" {
  description = "S3 bucket containing qengine resources (libs)"
  default = "prod-qengine-resources"
  type = string
}

# EC2 SSH default SSH key

variable "ec2_ssh_default_key" {
  description = "Default SSH key"
  default = {
    "key_name" = "qovery-{{ eks_cluster_id }}"
    "public_key" = "{{ qovery_ssh_key }}"
  }
  type = map(string)
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

# Agent info

variable "qovery_agent_replicas" {
  description = "This variable is used to get random ID generated for the agent"
  default = "1"
  type = number
}

# RDS

variable "rds_subnets_zone_a" {
  description = "RDS subnets Zone A"
  default = {{ rds_zone_a_subnet_blocks }}
  type = list(string)
}

variable "rds_subnets_zone_b" {
  description = "RDS subnets Zone B"
  default = {{ rds_zone_b_subnet_blocks }}
  type = list(string)
}

variable "rds_subnets_zone_c" {
  description = "RDS subnets Zone C"
  default = {{ rds_zone_c_subnet_blocks }}
  type = list(string)
}

variable "rds_cidr_subnet" {
  description = "RDS CIDR (x.x.x.x/CIDR)"
  default     = {{ rds_cidr_subnet }}
  type        = number
}

# DocumentDB

variable "documentdb_subnets_zone_a" {
  description = "DocumentDB subnets Zone A"
  default = {{ documentdb_zone_a_subnet_blocks }}
  type = list(string)
}

variable "documentdb_subnets_zone_b" {
  description = "DocumentDB subnets Zone B"
  default = {{ documentdb_zone_b_subnet_blocks }}
  type = list(string)
}

variable "documentdb_subnets_zone_c" {
  description = "DocumentDB subnets Zone C"
  default = {{ documentdb_zone_c_subnet_blocks }}
  type = list(string)
}

variable "documentdb_cidr_subnet" {
  description = "DocumentDB CIDR (x.x.x.x/CIDR)"
  default     = {{ documentdb_cidr_subnet }}
  type        = number
}

# Elasticsearch

variable "enable_elastic_search" {
  default = false
  type = bool
  description = "option that create elasticsearch stack for logs, logs could use loki as well"
}
# Must start with a lowercase alphabet and be at least 3 and no more than 28 characters long.
# Valid characters are a-z (lowercase letters), 0-9, and - (hyphen).
variable "elasticsearch_q_logs_domain_name" {
  description = "ES domain name"
  default = "qovery-{{ eks_cluster_id }}"
  type = string
}

variable "elasticsearch_node_number" {
  description = "Number of Elasticsearch nodes"
  default = 3
  type = number
}

variable "elasticsearch_volume_size" {
  description = "Disk size per node"
  default = 50
  type = number
}

variable "elasticsearch_subnets_zone_a" {
  description = "Elasticsearch subnets Zone A"
  default = {{ elasticsearch_zone_a_subnet_blocks }}
  type = list(string)
}

variable "elasticsearch_subnets_zone_b" {
  description = "Elasticsearch subnets Zone B"
  default = {{ elasticsearch_zone_b_subnet_blocks }}
  type = list(string)
}

variable "elasticsearch_subnets_zone_c" {
  description = "Elasticsearch subnets Zone C"
  default = {{ elasticsearch_zone_c_subnet_blocks }}
  type = list(string)
}

variable "elasticsearch_cidr_subnet" {
  description = "Elasticsearch CIDR (x.x.x.x/CIDR)"
  default     = {{ elasticsearch_cidr_subnet }}
  type        = number
}

# Helm alert manager discord
variable "discord_api_key" {
  description = "discord url with token for used for alerting"
  default = "{{ discord_api_key }}"
  type = string
}

# Helm qovery agent
variable "qovery_nats_url" {
  description = "URL of qovery nats server"
  default = "{{ qovery_nats_url }}"
  type = string
}