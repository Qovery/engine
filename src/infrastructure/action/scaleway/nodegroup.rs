use crate::errors::CommandError;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::infrastructure::models::kubernetes::scaleway::kapsule::{Kapsule, ScwNodeGroupErrors};
use crate::infrastructure::models::kubernetes::scaleway::node::ScwNodeGroup;
use crate::runtime::block_on;
use scaleway_api_rs::models::ScalewayK8sV1Cluster;

pub(super) fn get_existing_sanitized_node_groups(
    cluster: &Kapsule,
    cluster_info: ScalewayK8sV1Cluster,
) -> Result<Vec<ScwNodeGroup>, ScwNodeGroupErrors> {
    let error_cluster_id = "expected cluster id for this Scaleway cluster".to_string();
    let cluster_id = match cluster_info.id {
        None => {
            return Err(ScwNodeGroupErrors::NodeGroupValidationError(
                CommandError::new_from_safe_message(error_cluster_id),
            ));
        }
        Some(x) => x,
    };

    let pools = match block_on(scaleway_api_rs::apis::pools_api::list_pools(
        &cluster.get_configuration(),
        cluster.region(),
        cluster_id.as_str(),
        None,
        None,
        None,
        None,
        None,
    )) {
        Ok(x) => x,
        Err(e) => {
            return Err(ScwNodeGroupErrors::CloudProviderApiError(CommandError::new(
                format!("Error while trying to get SCW pool info from cluster {}.", &cluster_id),
                Some(e.to_string()),
                None,
            )));
        }
    };

    // ensure pool are present
    if pools.pools.is_none() {
        return Err(ScwNodeGroupErrors::NoNodePoolFound(CommandError::new_from_safe_message(
            format!(
                "Error, no SCW pool found from the SCW API for cluster {}/{}",
                &cluster_id,
                &cluster_info.name.unwrap_or_else(|| "unknown cluster".to_string())
            ),
        )));
    }

    // create sanitized nodegroup pools
    let mut nodegroup_pool: Vec<ScwNodeGroup> = Vec::with_capacity(pools.total_count.unwrap_or(0f32) as usize);
    for ng in pools.pools.unwrap() {
        if ng.id.is_none() {
            return Err(ScwNodeGroupErrors::NodeGroupValidationError(
                CommandError::new_from_safe_message(format!(
                    "Error while trying to validate SCW pool ID from cluster {}",
                    &cluster_id
                )),
            ));
        }
        let ng_sanitized = get_node_group_info(cluster, ng.id.unwrap().as_str())?;
        nodegroup_pool.push(ng_sanitized)
    }

    Ok(nodegroup_pool)
}

pub(super) fn get_node_group_info(cluster: &Kapsule, pool_id: &str) -> Result<ScwNodeGroup, ScwNodeGroupErrors> {
    let pool = match block_on(scaleway_api_rs::apis::pools_api::get_pool(
        &cluster.get_configuration(),
        cluster.region(),
        pool_id,
    )) {
        Ok(x) => x,
        Err(e) => {
            return Err(match e {
                scaleway_api_rs::apis::Error::ResponseError(x) => {
                    let msg_with_error =
                        format!("Error code while getting node group: {}, API message: {} ", x.status, x.content);
                    match x.status.as_u16() {
                        // TODO(ENG-1453): To be tested against StatusCode::NOT_FOUND once SCW will be bumped (it uses an old http version clashing with new one)
                        404_u16 /*StatusCode::NOT_FOUND*/ => ScwNodeGroupErrors::NoNodePoolFound(CommandError::new(
                            "No node pool found".to_string(),
                            Some(msg_with_error),
                            None,
                        )),
                        _ => ScwNodeGroupErrors::CloudProviderApiError(CommandError::new(
                            "Scaleway API error while trying to get node group".to_string(),
                            Some(msg_with_error),
                            None,
                        )),
                    }
                }
                _ => ScwNodeGroupErrors::NodeGroupValidationError(CommandError::new(
                    "This Scaleway API error is not supported in the engine, please add it to better support it"
                        .to_string(),
                    Some(e.to_string()),
                    None,
                )),
            });
        }
    };

    // ensure there is no missing info
    check_missing_nodegroup_info(&pool.name, "name")?;
    check_missing_nodegroup_info(&pool.min_size, "min_size")?;
    check_missing_nodegroup_info(&pool.max_size, "max_size")?;
    check_missing_nodegroup_info(&pool.status, "status")?;

    match ScwNodeGroup::new(
        pool.id,
        pool.name.unwrap(),
        pool.min_size.unwrap() as i32,
        pool.max_size.unwrap() as i32,
        pool.node_type,
        pool.size as i32,
        pool.status.unwrap(),
    ) {
        Ok(x) => Ok(x),
        Err(e) => Err(ScwNodeGroupErrors::NodeGroupValidationError(e)),
    }
}

fn check_missing_nodegroup_info<T>(item: &Option<T>, name: &str) -> Result<(), ScwNodeGroupErrors> {
    if item.is_none() {
        return Err(ScwNodeGroupErrors::MissingNodePoolInfo(name.to_string()));
    };

    Ok(())
}
