locals {
  mongodb_database_tags = merge (var.database_tags, {
    database_identifier = var.documentdb_identifier
    creationDate = time_static.on_db_create.rfc3339
    {% for key, value in labels_group.propagated_to_cloud_provider %}
    {{ key }} = "{{ value }}"
    {% endfor %}
  })
}