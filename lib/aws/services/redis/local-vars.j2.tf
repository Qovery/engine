locals {
  redis_database_tags = merge (var.database_tags, {
    database_identifier = var.elasticache_identifier
    creationDate = time_static.on_db_create.rfc3339
    {% if snapshot is defined and snapshot["snapshot_id"] %}meta_last_restored_from = var.snapshot_identifier{% endif %}
  })
}