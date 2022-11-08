locals {
  database_tf_config = <<TF_CONFIG
{
  {%- if database_elasticache_parameter_group_name == 'default.redis5.0' or database_login == 'qoveryadmin' %}
  "database_target_id": "${aws_elasticache_cluster.elasticache_cluster.id}",
  "database_target_hostname": "${aws_elasticache_cluster.elasticache_cluster.cache_nodes.0.address}",
  {%- else %}
  "database_target_id": "${aws_elasticache_replication_group.elasticache_cluster.id}",
  {%- if database_elasticache_instances_number > 1 %}
  "database_target_hostname": "${aws_elasticache_replication_group.elasticache_cluster.configuration_endpoint_address}",
  {%- else %}
  "database_target_hostname": "${aws_elasticache_replication_group.elasticache_cluster.primary_endpoint_address}",
  {%- endif %}
  {%- endif %}
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
