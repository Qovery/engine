use crate::cloud_provider::kubernetes::{Kind, Kubernetes};
use crate::cloud_provider::models::{KubernetesClusterAction, NodeGroups, NodeGroupsWithDesiredState};
use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use crate::infrastructure_action::eks::sdk::QoveryAwsSdkConfigEks;
use crate::runtime::block_on;
use aws_sdk_eks::operation::describe_nodegroup::DescribeNodegroupOutput;
use aws_types::SdkConfig;
use rusoto_eks::{DescribeNodegroupRequest, Eks, EksClient, ListNodegroupsRequest, NodegroupScalingConfig};
use thiserror::Error;

#[derive(Debug, PartialEq, Eq, Error)]
pub enum NodeGroupToRemoveFailure {
    #[error("No cluster found")]
    ClusterNotFound,
    #[error("No nodegroup found for this cluster")]
    NodeGroupNotFound,
    #[error("At lease one nodegroup must be active, no one can be deleted")]
    OneNodeGroupMustBeActiveAtLeast,
}

#[derive(PartialEq, Eq)]
pub enum NodeGroupsDeletionType {
    All,
    FailedOnly,
}

fn select_nodegroups_autoscaling_group_behavior(
    action: KubernetesClusterAction,
    nodegroup: &NodeGroups,
) -> NodeGroupsWithDesiredState {
    let nodegroup_desired_state = |x| {
        // desired nodes can't be lower than min nodes
        if x < nodegroup.min_nodes {
            (true, nodegroup.min_nodes)
            // desired nodes can't be higher than max nodes
        } else if x > nodegroup.max_nodes {
            (true, nodegroup.max_nodes)
        } else {
            (false, x)
        }
    };

    match action {
        KubernetesClusterAction::Bootstrap => {
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, nodegroup.min_nodes, true)
        }
        KubernetesClusterAction::Update(current_nodes) | KubernetesClusterAction::Upgrade(current_nodes) => {
            let (upgrade_required, desired_state) = match current_nodes {
                Some(x) => nodegroup_desired_state(x),
                // if nothing is given, it's may be because the nodegroup has been deleted manually, so if we need to set it otherwise we won't be able to create a new nodegroup
                None => (true, nodegroup.max_nodes),
            };
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, desired_state, upgrade_required)
        }
        KubernetesClusterAction::Pause | KubernetesClusterAction::Delete => {
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, nodegroup.min_nodes, false)
        }
        KubernetesClusterAction::Resume(current_nodes) => {
            // we always want to set the desired sate here to optimize the speed to return to the best situation
            // TODO: (pmavro) save state on pause and reread it on resume
            let resume_nodes_number = match current_nodes {
                Some(x) => nodegroup_desired_state(x).1,
                None => nodegroup.min_nodes,
            };
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, resume_nodes_number, true)
        }
        KubernetesClusterAction::CleanKarpenterMigration => {
            NodeGroupsWithDesiredState::new_from_node_groups(nodegroup, 0, false)
        }
    }
}

/// Returns a tuple of (update_desired_node: bool, desired_nodes_count: i32).
pub fn should_update_desired_nodes(
    event_details: EventDetails,
    kubernetes: &dyn Kubernetes,
    action: KubernetesClusterAction,
    node_groups: &[NodeGroups],
    aws_eks_client: Option<EksClient>,
) -> Result<Vec<NodeGroupsWithDesiredState>, Box<EngineError>> {
    let get_autoscaling_config =
        |node_group: &NodeGroups, eks_client: EksClient| -> Result<Option<i32>, Box<EngineError>> {
            let current_nodes = get_nodegroup_autoscaling_config_from_aws(
                event_details.clone(),
                kubernetes,
                node_group.clone(),
                eks_client,
            )?;
            match current_nodes {
                Some(x) => match x.desired_size {
                    Some(n) => Ok(Some(n as i32)),
                    None => Ok(None),
                },
                None => Ok(None),
            }
        };
    let mut node_groups_with_size = Vec::with_capacity(node_groups.len());

    for node_group in node_groups {
        let eks_client = match aws_eks_client.clone() {
            Some(x) => x,
            None => {
                // if no clients, we're in bootstrap mode
                select_nodegroups_autoscaling_group_behavior(action, node_group);
                continue;
            }
        };
        let node_group_with_desired_state = match action {
            KubernetesClusterAction::Bootstrap | KubernetesClusterAction::Pause | KubernetesClusterAction::Delete => {
                select_nodegroups_autoscaling_group_behavior(action, node_group)
            }
            KubernetesClusterAction::Update(_) => {
                let current_nodes = get_autoscaling_config(node_group, eks_client)?;
                select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(current_nodes), node_group)
            }
            KubernetesClusterAction::Upgrade(_) => {
                let current_nodes = get_autoscaling_config(node_group, eks_client)?;
                select_nodegroups_autoscaling_group_behavior(
                    KubernetesClusterAction::Upgrade(current_nodes),
                    node_group,
                )
            }
            KubernetesClusterAction::Resume(_) => {
                let current_nodes = get_autoscaling_config(node_group, eks_client)?;
                select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Resume(current_nodes), node_group)
            }
            KubernetesClusterAction::CleanKarpenterMigration => {
                select_nodegroups_autoscaling_group_behavior(action, node_group)
            }
        };
        node_groups_with_size.push(node_group_with_desired_state)
    }

    Ok(node_groups_with_size)
}

/// Returns the scaling config of a node_group by node_group_name.
pub fn get_nodegroup_autoscaling_config_from_aws(
    event_details: EventDetails,
    kubernetes: &dyn Kubernetes,
    node_group: NodeGroups,
    eks_client: EksClient,
) -> Result<Option<NodegroupScalingConfig>, Box<EngineError>> {
    // In case of EC2, there is no need to care about auto scaling
    if kubernetes.kind() == Kind::Ec2 {
        return Ok(None);
    }

    let eks_node_groups = match block_on(eks_client.list_nodegroups(ListNodegroupsRequest {
        cluster_name: kubernetes.cluster_name(),
        ..Default::default()
    })) {
        Ok(res) => match res.nodegroups {
            // This could be empty on paused clusters, we should not return an error for this
            None => return Ok(None),
            Some(x) => x,
        },
        Err(e) => {
            return Err(Box::new(EngineError::new_nodegroup_list_error(
                event_details,
                CommandError::new(
                    e.to_string(),
                    Some("Error while trying to get node groups from eks".to_string()),
                    None,
                ),
            )));
        }
    };

    // Find eks_node_group that matches the node_group.name passed in parameters
    let mut scaling_config: Option<NodegroupScalingConfig> = None;
    for eks_node_group_name in eks_node_groups {
        // warn: can't filter the state of the autoscaling group with this lib. We should filter on running (and not deleting/creating)
        let eks_node_group = match block_on(eks_client.describe_nodegroup(DescribeNodegroupRequest {
            cluster_name: kubernetes.cluster_name(),
            nodegroup_name: eks_node_group_name.clone(),
        })) {
            Ok(res) => match res.nodegroup {
                None => {
                    return Err(Box::new(EngineError::new_missing_nodegroup_information_error(
                        event_details,
                        eks_node_group_name,
                    )));
                }
                Some(x) => x,
            },
            Err(error) => {
                return Err(Box::new(EngineError::new_cluster_worker_node_not_found(
                    event_details,
                    Some(CommandError::new(
                        "Error while trying to get node groups from AWS".to_string(),
                        Some(error.to_string()),
                        None,
                    )),
                )));
            }
        };
        // ignore if group of nodes is not managed by Qovery
        match eks_node_group.tags {
            None => continue,
            Some(tags) => match tags.get("QoveryNodeGroupName") {
                None => continue,
                Some(tag) => {
                    if tag == &node_group.name {
                        scaling_config = eks_node_group.scaling_config;
                        break;
                    }
                }
            },
        }
    }

    Ok(scaling_config)
}

pub fn node_group_is_running(
    kubernetes: &dyn Kubernetes,
    event_details: &EventDetails,
    node_group: &NodeGroups,
    eks_client: Option<EksClient>,
) -> Result<Option<i32>, Box<EngineError>> {
    let client = match eks_client {
        Some(client) => client,
        None => return Ok(None),
    };

    let current_nodes =
        get_nodegroup_autoscaling_config_from_aws(event_details.clone(), kubernetes, node_group.clone(), client)?;
    match current_nodes {
        Some(config) => match config.desired_size {
            Some(n) => Ok(Some(n as i32)),
            None => Ok(None),
        },
        None => Ok(None),
    }
}

pub async fn delete_eks_nodegroups(
    aws_conn: SdkConfig,
    cluster_name: String,
    is_first_install: bool,
    nodegroup_delete_selection: NodeGroupsDeletionType,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    let clusters = match aws_conn.list_clusters().await {
        Ok(x) => x,
        Err(e) => {
            return Err(Box::new(EngineError::new_cannot_list_clusters_error(
                event_details.clone(),
                CommandError::new("Couldn't list clusters from AWS".to_string(), Some(e.to_string()), None),
            )));
        }
    };

    if !clusters.clusters().iter().any(|x| x == &cluster_name) {
        return Err(Box::new(EngineError::new_cannot_get_cluster_error(
            event_details.clone(),
            CommandError::new_from_safe_message(NodeGroupToRemoveFailure::ClusterNotFound.to_string()),
        )));
    };

    let all_cluster_nodegroups = match aws_conn.list_all_eks_nodegroups(cluster_name.clone()).await {
        Ok(x) => x,
        Err(e) => {
            return Err(Box::new(EngineError::new_nodegroup_list_error(
                event_details,
                CommandError::new_from_safe_message(e.to_string()),
            )));
        }
    };

    let all_cluster_nodegroups_described = match aws_conn
        .describe_nodegroups(cluster_name.clone(), all_cluster_nodegroups)
        .await
    {
        Ok(x) => x,
        Err(e) => {
            return Err(Box::new(EngineError::new_missing_nodegroup_information_error(
                event_details,
                e.to_string(),
            )));
        }
    };

    // If it is the first installation of the cluster, we dont want to keep any nodegroup.
    // So just delete everything
    let nodegroups_to_delete = if is_first_install || nodegroup_delete_selection == NodeGroupsDeletionType::All {
        info!("Deleting all nodegroups of this cluster as it is the first installation.");
        all_cluster_nodegroups_described
    } else {
        match check_failed_nodegroups_to_remove(all_cluster_nodegroups_described.clone()) {
            Ok(x) => x,
            Err(e) => {
                // print AWS nodegroup errors to the customer (useful when quota is reached)
                if e == NodeGroupToRemoveFailure::OneNodeGroupMustBeActiveAtLeast {
                    let nodegroup_health_message = all_cluster_nodegroups_described
                        .iter()
                        .map(|n| match n.nodegroup() {
                            Some(nodegroup) => {
                                let nodegroup_name = nodegroup.nodegroup_name().unwrap_or("unknown_nodegroup_name");
                                let nodegroup_status = match nodegroup.health() {
                                    Some(x) =>
                                        x
                                            .issues()
                                            .iter()
                                            .map(|x| format!("{:?}: {}", x.code(), x.message().unwrap_or("no AWS specific message given, please contact Qovery and AWS support regarding this nodegroup issue")))
                                            .collect::<Vec<String>>()
                                            .join(", "),
                                    None => "can't get nodegroup status from cloud provider".to_string(),
                                };
                                format!("Nodegroup {nodegroup_name} health is: {nodegroup_status}")
                            }
                            None => "".to_string(),
                        })
                        .collect::<Vec<String>>()
                        .join("\n");

                    return Err(Box::new(EngineError::new_nodegroup_delete_any_nodegroup_error(
                        event_details,
                        nodegroup_health_message,
                    )));
                };

                return Err(Box::new(EngineError::new_nodegroup_delete_error(
                    event_details,
                    None,
                    e.to_string(),
                )));
            }
        }
    };

    for nodegroup in nodegroups_to_delete {
        let nodegroup_name = match nodegroup.nodegroup() {
            Some(x) => x.nodegroup_name().unwrap_or("unknown_nodegroup_name"),
            None => {
                return Err(Box::new(EngineError::new_missing_nodegroup_information_error(
                    event_details,
                    format!("{nodegroup:?}"),
                )));
            }
        };

        if let Err(e) = aws_conn
            .delete_nodegroup(cluster_name.clone(), nodegroup_name.to_string())
            .await
        {
            return Err(Box::new(EngineError::new_nodegroup_delete_error(
                event_details,
                Some(nodegroup_name.to_string()),
                e.to_string(),
            )));
        }
    }

    Ok(())
}

fn check_failed_nodegroups_to_remove(
    nodegroups: Vec<DescribeNodegroupOutput>,
) -> Result<Vec<DescribeNodegroupOutput>, NodeGroupToRemoveFailure> {
    let mut failed_nodegroups_to_remove = Vec::new();

    for nodegroup in nodegroups.iter() {
        match nodegroup.nodegroup() {
            Some(ng) => match ng.status() {
                Some(s) => match s {
                    aws_sdk_eks::types::NodegroupStatus::CreateFailed => {
                        failed_nodegroups_to_remove.push(nodegroup.clone())
                    }
                    aws_sdk_eks::types::NodegroupStatus::DeleteFailed => {
                        failed_nodegroups_to_remove.push(nodegroup.clone())
                    }
                    aws_sdk_eks::types::NodegroupStatus::Degraded => {
                        failed_nodegroups_to_remove.push(nodegroup.clone())
                    }
                    _ => {
                        info!(
                            "Nodegroup {} is in state {:?}, it will not be deleted",
                            ng.nodegroup_name().unwrap_or("unknown name"),
                            s
                        );
                        continue;
                    }
                },
                None => continue,
            },
            None => return Err(NodeGroupToRemoveFailure::NodeGroupNotFound),
        }
    }

    // ensure we don't remove all nodegroups (even failed ones) to avoid blackout
    if failed_nodegroups_to_remove.len() == nodegroups.len() && !nodegroups.is_empty() {
        return Err(NodeGroupToRemoveFailure::OneNodeGroupMustBeActiveAtLeast);
    }

    Ok(failed_nodegroups_to_remove)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_provider::models::{
        CpuArchitecture, KubernetesClusterAction, NodeGroups, NodeGroupsWithDesiredState,
    };
    use aws_sdk_eks::operation::describe_nodegroup::DescribeNodegroupOutput;
    use aws_sdk_eks::types::{Nodegroup, NodegroupStatus};

    use super::check_failed_nodegroups_to_remove;

    impl NodeGroupsWithDesiredState {
        fn new(
            name: String,
            id: Option<String>,
            min_nodes: i32,
            max_nodes: i32,
            desired_size: i32,
            enable_desired_size: bool,
            instance_type: String,
            disk_size_in_gib: i32,
        ) -> NodeGroupsWithDesiredState {
            NodeGroupsWithDesiredState {
                name,
                id,
                min_nodes,
                max_nodes,
                desired_size,
                enable_desired_size,
                instance_type,
                disk_size_in_gib,
                instance_architecture: CpuArchitecture::AMD64,
            }
        }
    }

    #[test]
    fn test_nodegroup_failure_deletion() {
        let nodegroup_ok = Nodegroup::builder()
            .set_nodegroup_name(Some("nodegroup_ok".to_string()))
            .set_status(Some(NodegroupStatus::Active))
            .build();
        let nodegroup_create_failed = Nodegroup::builder()
            .set_nodegroup_name(Some("nodegroup_create_failed".to_string()))
            .set_status(Some(NodegroupStatus::CreateFailed))
            .build();

        // 2 nodegroups, 2 ok => nothing to delete
        let ngs = vec![
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_ok.clone())
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_ok.clone())
                .build(),
        ];
        assert_eq!(check_failed_nodegroups_to_remove(ngs).unwrap().len(), 0);

        // 2 nodegroups, 1 ok, 1 create failed => 1 to delete
        let ngs = vec![
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_ok.clone())
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_create_failed.clone())
                .build(),
        ];
        let failed_ngs = check_failed_nodegroups_to_remove(ngs).unwrap();
        assert_eq!(failed_ngs.len(), 1);
        assert_eq!(
            failed_ngs[0].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_create_failed"
        );

        // 2 nodegroups, 2 failed => nothing to do, too critical to be deleted. Manual intervention required
        let ngs = vec![
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_create_failed.clone())
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_create_failed.clone())
                .build(),
        ];
        assert_eq!(
            check_failed_nodegroups_to_remove(ngs).unwrap_err(),
            NodeGroupToRemoveFailure::OneNodeGroupMustBeActiveAtLeast
        );

        // 1 nodegroup, 1 failed => nothing to do, too critical to be deleted. Manual intervention required
        let ngs = vec![DescribeNodegroupOutput::builder()
            .nodegroup(nodegroup_create_failed.clone())
            .build()];
        assert_eq!(
            check_failed_nodegroups_to_remove(ngs).unwrap_err(),
            NodeGroupToRemoveFailure::OneNodeGroupMustBeActiveAtLeast
        );

        // no nodegroups => ok
        let ngs = vec![];
        assert_eq!(check_failed_nodegroups_to_remove(ngs).unwrap().len(), 0);

        // x nodegroups, 1 ok, 2 create failed, 1 delete failure, others in other states => 4 to delete
        let ngs = vec![
            DescribeNodegroupOutput::builder().nodegroup(nodegroup_ok).build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(nodegroup_create_failed)
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::CreateFailed)))
                        .set_status(Some(NodegroupStatus::CreateFailed))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Deleting)))
                        .set_status(Some(NodegroupStatus::Deleting))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Creating)))
                        .set_status(Some(NodegroupStatus::Creating))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Degraded)))
                        .set_status(Some(NodegroupStatus::Degraded))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::DeleteFailed)))
                        .set_status(Some(NodegroupStatus::DeleteFailed))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Deleting)))
                        .set_status(Some(NodegroupStatus::Deleting))
                        .build(),
                )
                .build(),
            DescribeNodegroupOutput::builder()
                .nodegroup(
                    Nodegroup::builder()
                        .set_nodegroup_name(Some(format!("nodegroup_{:?}", NodegroupStatus::Updating)))
                        .set_status(Some(NodegroupStatus::Updating))
                        .build(),
                )
                .build(),
        ];
        let failed_ngs = check_failed_nodegroups_to_remove(ngs).unwrap();
        assert_eq!(failed_ngs.len(), 4);
        assert_eq!(
            failed_ngs[0].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_create_failed"
        );
        assert_eq!(
            failed_ngs[1].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_CreateFailed"
        );
        assert_eq!(
            failed_ngs[2].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_Degraded"
        );
        assert_eq!(
            failed_ngs[3].nodegroup().unwrap().nodegroup_name().unwrap(),
            "nodegroup_DeleteFailed"
        );
    }

    #[test]
    fn test_nodegroup_autoscaling_group() {
        let nodegroup_with_ds = |desired_nodes, enable_desired_nodes| {
            NodeGroupsWithDesiredState::new(
                "nodegroup".to_string(),
                None,
                3,
                10,
                desired_nodes,
                enable_desired_nodes,
                "t1000.xlarge".to_string(),
                20,
            )
        };
        let nodegroup = NodeGroups::new(
            "nodegroup".to_string(),
            3,
            10,
            "t1000.xlarge".to_string(),
            20,
            CpuArchitecture::AMD64,
        )
        .unwrap();

        // bootstrap
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Bootstrap, &nodegroup),
            nodegroup_with_ds(3, true) // need true because it's required from AWS to set desired node when initializing the autoscaler
        );
        // pause
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Pause, &nodegroup),
            nodegroup_with_ds(3, false)
        );
        // delete
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Delete, &nodegroup),
            nodegroup_with_ds(3, false)
        );
        // resume
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Resume(Some(5)), &nodegroup),
            nodegroup_with_ds(5, true)
        );
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Resume(None), &nodegroup),
            // if no info is given during resume, we should take the max and let the autoscaler reduce afterwards
            // but by setting it to the max, some users with have to ask support to raise limits
            // also useful when a customer wants to try Qovery, and do not need to ask AWS support in the early phase
            nodegroup_with_ds(3, true)
        );
        // update (we never have to change desired state during an update because the autoscaler manages it already)
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(Some(6)), &nodegroup),
            nodegroup_with_ds(6, false)
        );
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(None), &nodegroup),
            nodegroup_with_ds(10, true) // max node is set just in case there is an issue with the AWS autoscaler to retrieve info, but should not be applied
        );
        // upgrade (we never have to change desired state during an update because the autoscaler manages it already)
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Upgrade(Some(7)), &nodegroup),
            nodegroup_with_ds(7, false)
        );
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(None), &nodegroup),
            nodegroup_with_ds(10, true) // max node is set just in case there is an issue with the AWS autoscaler to retrieve info, but should not be applied
        );

        // test autocorrection of silly stuffs
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(Some(1)), &nodegroup),
            nodegroup_with_ds(3, true) // set to minimum if desired is below min
        );
        assert_eq!(
            select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(Some(1000)), &nodegroup),
            nodegroup_with_ds(10, true) // set to max if desired is above max
        );
    }
}
