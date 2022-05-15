# Qovery

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

# AWS specific

variable "aws_availability_zones" {
  description = "AWS availability zones"
  default = {{ aws_availability_zones }}
  type = list(string)
}

variable "vpc_cidr_block" {
  description = "VPC CIDR block"
  default = "{{ vpc_cidr_block }}"
  type = string
}

# ec2

variable "ec2_image_info" {
  description = "EC2 image information"
  default = {
    "name" = "debian-10-amd64*"
    "owners" = "136693071363"
  }
  type = map(string)
}

variable "ec2_instance" {
  description = "EC2 instance configuration"
  default = {
    "instance_type" = "{{ eks_worker_nodes[0].instance_type }}"
    "disk_size_in_gb" = "{{ eks_worker_nodes[0].disk_size_in_gib }}"
    "user_data_logs_path" = "/var/log/user-data.log" # install error logs location
  }
  type = map(string)
}

variable "k3s_config" {
  description = "K3s configuration"
  default = {
    "version" = "v1.20.15+k3s1"
    "channel" = "stable"
    "exec" = "--disable=traefik --disable=metrics-server"
  }
  type = map(string)
}

variable "ec2_subnets_zone_a_private" {
  description = "EC2 private subnets Zone A"
  default = {{ ec2_zone_a_subnet_blocks_private }}
  type = list(string)
}

variable "ec2_subnets_zone_b_private" {
  description = "EC2 private subnets Zone B"
  default = {{ ec2_zone_b_subnet_blocks_private }}
  type = list(string)
}

variable "ec2_subnets_zone_c_private" {
  description = "EC2 private subnets Zone C"
  default = {{ ec2_zone_c_subnet_blocks_private }}
  type = list(string)
}

{% if vpc_qovery_network_mode == "WithNatGateways" %}
variable "ec2_subnets_zone_a_public" {
  description = "EC2 public subnets Zone A"
  default = {{ ec2_zone_a_subnet_blocks_public }}
  type = list(string)
}

variable "ec2_subnets_zone_b_public" {
  description = "EC2 public subnets Zone B"
  default = {{ ec2_zone_b_subnet_blocks_public }}
  type = list(string)
}

variable "ec2_subnets_zone_c_public" {
  description = "EC2 public subnets Zone C"
  default = {{ ec2_zone_c_subnet_blocks_public }}
  type = list(string)
}
{% endif %}

variable "ec2_cidr_subnet" {
  description = "EC2 CIDR (x.x.x.x/CIDR)"
  default     = {{ ec2_cidr_subnet }}
  type        = number
}

variable "ec2_k8s_versions" {
  description = "Kubernetes version"
  default = {
    "masters": "{{ ec2_masters_version }}",
    "workers": "{{ ec2_workers_version }}",
  }
  type = map(string)
}

variable "kubernetes_full_cluster_id" {
  description = "Kubernetes full cluster id"
  default     = "{{ kubernetes_full_cluster_id }}"
  type        = string
}

variable "kubernetes_cluster_id" {
  description = "Kubernetes cluster id"
  default     = "{{ kubernetes_cluster_id }}"
  type        = string
}

variable "kubernetes_cluster_name" {
  description = "Kubernetes cluster name"
  default     = "qovery-{{ kubernetes_cluster_id }}"
  type        = string
}

variable "ec2_access_cidr_blocks" {
  description = "Kubernetes CIDR Block"
  default     = {{ ec2_access_cidr_blocks }}
  type        = list(string)
}

# S3 bucket name

variable "s3_bucket_kubeconfig" {
  description = "S3 bucket containing kubeconfigs"
  default = "{{ s3_kubeconfig_bucket }}"
  type = string
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

# Elasticache

variable "elasticache_subnets_zone_a" {
  description = "Elasticache subnets Zone A"
  default = {{ elasticache_zone_a_subnet_blocks }}
  type = list(string)
}

variable "elasticache_subnets_zone_b" {
  description = "Elasticache subnets Zone B"
  default = {{ elasticache_zone_b_subnet_blocks }}
  type = list(string)
}

variable "elasticache_subnets_zone_c" {
  description = "Elasticache subnets Zone C"
  default = {{ elasticache_zone_c_subnet_blocks }}
  type = list(string)
}

variable "elasticache_cidr_subnet" {
  description = "Elasticache CIDR (x.x.x.x/CIDR)"
  default     = {{ elasticache_cidr_subnet }}
  type        = number
}

# Elasticsearch

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

# Qovery features

variable "log_history_enabled" {
  description = "Enable log history"
  default = {{ log_history_enabled }}
  type = bool
}

variable "metrics_history_enabled" {
  description = "Enable metrics history"
  default = {{ metrics_history_enabled }}
  type = bool
}

{%- if resource_expiration_in_seconds is defined %}
# Pleco ttl
variable "resource_expiration_in_seconds" {
  description = "Resource expiration in seconds"
  default = {{ resource_expiration_in_seconds }}
  type = number
}
{% endif %}