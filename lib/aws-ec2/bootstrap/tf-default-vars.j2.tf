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

variable "organization_long_id" {
  description = "Qovery Organization long ID"
  default     = "{{ organization_long_id }}"
  type        = string
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
    {% if eks_worker_nodes[0].instance_architecture == "ARM64" -%}
    "name" = "debian-10-arm64*"
    {%- else -%}
    "name" = "debian-10-amd64*"
    {%- endif %}
    "owners" = "136693071363"
  }
  type = map(string)
}

variable "ec2_instance" {
  description = "EC2 instance configuration"
  default = {
    "instance_type" = "{{ eks_worker_nodes[0].instance_type }}"
    "disk_size_in_gb" = "{{ eks_worker_nodes[0].disk_size_in_gib }}"
    "user_data_logs_path" = "/var/log/user_data.log" # install error logs location
    "volume_device_name" = "/dev/sdf"
  }
  type = map(string)
}

variable "k3s_config" {
  description = "K3s configuration"
  default = {
    "version" = "{{ k3s_version }}"
    "channel" = "stable"
    "exposed_port" = "{{ ec2_port }}"
    "exec" = "--disable=traefik --disable=metrics-server" # remove when migration is done
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

variable "ec2_metadata_imds_version" {
  description = "Set the imds version"
  default = "{{ ec2_metadata_imds_version }}"
  type = string
}

variable "ec2_k8s_versions" {
  description = "Kubernetes version"
  default = {
    "masters": "{{ ec2_masters_version }}",
    "workers": "{{ ec2_workers_version }}",
  }
  type = map(string)
}

variable "kubernetes_cluster_long_id" {
  description = "Kubernetes cluster long id"
  default     = "{{ kubernetes_cluster_long_id }}"
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

# S3 bucket name

variable "s3_bucket_kubeconfig" {
  description = "S3 bucket containing kubeconfigs"
  default = "{{ s3_kubeconfig_bucket }}"
  type = string
}

variable "enable_vpc_flow_logs" {
  description = "Enable VPC flow logs"
  default = {{ aws_enable_vpc_flow_logs }}
  type = bool
}

variable "vpc_flow_logs_retention_days" {
  description = "Set VPC flow logs retention in days"
  default = {{ vpc_flow_logs_retention_days }}
  type = number
}

variable "s3_flow_logs_bucket_name" {
  description = "S3 bucket containing flow logs"
  default = "{{ s3_flow_logs_bucket_name }}"
  type = string
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

variable "database_postgresql_allowed_cidrs" {
  description = "PostgreSQL allowed CIDR Block"
  default = {{ database_postgresql_allowed_cidrs }}
  type = list(string)
}

variable "database_mysql_allowed_cidrs" {
  description = "MySQL allowed CIDR Block"
  default = {{ database_mysql_allowed_cidrs }}
  type = list(string)
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

variable "database_mongodb_allowed_cidrs" {
  description = "MongoDB allowed CIDR Block"
  default = {{ database_mongodb_allowed_cidrs }}
  type = list(string)
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

variable "database_redis_allowed_cidrs" {
  description = "Redis allowed CIDR Block"
  default = {{ database_redis_allowed_cidrs }}
  type = list(string)
}

{%- if resource_expiration_in_seconds > -1 %}
# Pleco ttl
variable "resource_expiration_in_seconds" {
  description = "Resource expiration in seconds"
  default = {{ resource_expiration_in_seconds }}
  type = number
}
{% endif %}
