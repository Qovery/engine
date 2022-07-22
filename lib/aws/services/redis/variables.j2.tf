# elasticache instance basics

variable "elasticache_identifier" {
  description = "Elasticache cluster name (Cluster identifier)"
  default = "{{ fqdn_id }}"
  type = string
}

variable "elasticache_version" {
  description = "Elasticache version"
  default = "{{ version }}"
  type = string
}

variable "parameter_group_name" {
  description = "Elasticache parameter group name"
  default = "{{ database_elasticache_parameter_group_name }}"
  type = string
}

variable "elasticache_instances_number" {
  description = "Elasticache instance numbers"
  default = {{ database_elasticache_instances_number }}
  type = number
}

variable "port" {
  description = "Elasticache instance port"
  default = {{ database_port }}
  type = number
}

variable "password" {
  description = "Admin password for the master DB user"
  default = "{{ database_password }}"
  type = string
}

variable "instance_class" {
  description = "Type of instance: https://docs.aws.amazon.com/AmazonElastiCache/latest/red-ug/CacheNodes.SupportedTypes.html"
  default = "{{ database_instance_type }}"
  type = string
}