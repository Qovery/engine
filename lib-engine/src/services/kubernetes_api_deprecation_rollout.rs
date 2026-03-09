use uuid::Uuid;

// Temporary rollout gate: only clusters in this list use Pluto.
// Remove this allowlist once Pluto rollout is complete.
const PLUTO_CLUSTER_ALLOWLIST: &[&str] = &[
    "be9e22b0-d05a-4330-b5b5-547667d380fd", // Qovery AWS Test
    "f6b6e9ca-8c26-4c9d-88c5-aca5076f82d5",
];

pub fn is_pluto_enabled_for_cluster(cluster_id: &Uuid) -> bool {
    is_pluto_enabled_for_cluster_with_allowlist(cluster_id, PLUTO_CLUSTER_ALLOWLIST)
}

fn is_pluto_enabled_for_cluster_with_allowlist(cluster_id: &Uuid, allowlist: &[&str]) -> bool {
    allowlist.iter().any(|id| match Uuid::parse_str(id) {
        Ok(candidate) => candidate == *cluster_id,
        Err(err) => {
            warn!("Ignoring invalid UUID in PLUTO_CLUSTER_ALLOWLIST: `{}` ({})", id, err);
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_pluto_enabled_for_allowlisted_cluster() {
        let cluster_id = Uuid::parse_str("be9e22b0-d05a-4330-b5b5-547667d380fd").expect("UUID should be valid");
        assert!(is_pluto_enabled_for_cluster(&cluster_id));
    }

    #[test]
    fn test_is_pluto_disabled_for_non_allowlisted_cluster() {
        let cluster_id = Uuid::parse_str("22222222-2222-2222-2222-222222222222").expect("UUID should be valid");
        assert!(!is_pluto_enabled_for_cluster(&cluster_id));
    }

    #[test]
    fn test_is_pluto_enabled_ignores_invalid_allowlist_entries() {
        let cluster_id = Uuid::parse_str("be9e22b0-d05a-4330-b5b5-547667d380fd").expect("UUID should be valid");

        assert!(is_pluto_enabled_for_cluster_with_allowlist(
            &cluster_id,
            &["not-a-uuid", "be9e22b0-d05a-4330-b5b5-547667d380fd"]
        ));
    }
}
