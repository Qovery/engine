data "aws_caller_identity" "current" {}

locals {
  database_tf_config = <<TF_CONFIG
{
  "database_target_id": {%- if database_elasticache_parameter_group_name == 'default.redis5.0' or database_login == 'qoveryadmin' %} "${aws_elasticache_cluster.elasticache_cluster.id}" {%- else %} "${aws_elasticache_replication_group.elasticache_cluster.id}" {%- endif %}, }" {%- endif %},
  "database_target_hostname": {%- if database_elasticache_parameter_group_name == 'default.redis5.0' or database_login == 'qoveryadmin' %} "${aws_elasticache_cluster.elasticache_cluster.cache_nodes.0.address}" {%- else %} {%- if database_elasticache_instances_number > 1 %} "${aws_elasticache_replication_group.elasticache_cluster.configuration_endpoint_address}" {%- else %} "${aws_elasticache_replication_group.elasticache_cluster.primary_endpoint_address} {%- endif %} {%- endif %},
  "database_target_fqdn_id": "{{ fqdn_id }}",
  "database_target_fqdn": "{{ fqdn }}"
}
TF_CONFIG
}

resource "local_file" "database_tf_config" {
  filename = "database-tf-config.json"
  content = local.database_tf_config
  file_permission = "0600"
}
