//! Structured overrides for the vendored database charts. Values remain data throughout rendering.
use crate::infrastructure::models::cloud_provider::{Kind, service::DatabaseType};
use serde_json::{Value, json};
use tera::Context;

fn extend(target: &mut Value, source: &Value) {
    if let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) {
        target.extend(source.clone());
    }
}

pub(super) fn values(context: &Context, provider: Kind, database: DatabaseType, bitnami: bool) -> Value {
    let c = context.clone().into_json();
    let aws = provider == Kind::Aws;
    let managed = provider != Kind::OnPremise;
    let s = |key: &str| c[key].as_str().map(str::to_owned).unwrap_or_else(|| c[key].to_string());
    let annotations = |key: &str| {
        c["annotations_group"][key]
            .as_object()
            .cloned()
            .map(Value::Object)
            .unwrap_or(json!({}))
    };
    let base_labels = json!({
        "envId": c["environment_id"], "qovery.com/service-id": c["long_id"],
        "qovery.com/service-type": "database", "qovery.com/environment-id": c["environment_long_id"],
        "qovery.com/project-id": c["project_long_id"]
    });
    let mut labels = base_labels.clone();
    if managed || !bitnami {
        extend(
            &mut labels,
            &json!({"databaseId":c["id"], "databaseLongId":c["long_id"], "envLongId":c["environment_long_id"], "projectLongId":c["project_long_id"]}),
        );
    }
    let mut legacy_labels = labels.clone();
    if managed {
        legacy_labels["app"] = c["sanitized_name"].clone();
    }
    let mut common_labels = if bitnami && matches!(database, DatabaseType::MySQL | DatabaseType::Redis) {
        legacy_labels.clone()
    } else {
        labels.clone()
    };
    if (!bitnami || managed) && !(bitnami && database == DatabaseType::MySQL) {
        extend(&mut common_labels, &c["labels_group"]["common"]);
    }
    let resources = json!({"requests":{"memory":c["ram_request_in_mib"],"cpu":c["cpu_request_in_milli"]},"limits":{"memory":c["ram_limit_in_mib"],"cpu":c["cpu_limit_in_milli"]}});
    let mut persistence = json!({"storageClass":c["database_disk_type"],"size":format!("{}Gi",s("database_disk_size_in_gib")),"annotations":{}});
    if !bitnami || managed {
        persistence["annotations"] = json!({"ownerId":c["owner_id"],"envId":c["environment_id"],"databaseId":c["id"],"databaseName":c["sanitized_name"]});
    }
    if bitnami {
        persistence["labels"] = legacy_labels.clone();
    }
    let mut service = json!({"name":c["service_name"],"type":if c["publicly_accessible"] == true {"LoadBalancer"} else {"ClusterIP"}});
    if c["publicly_accessible"] == true {
        let mut a = json!({});
        if aws {
            a = json!({"service.beta.kubernetes.io/aws-load-balancer-type":c["aws_load_balancer_type"],"service.beta.kubernetes.io/aws-load-balancer-scheme":"internet-facing"});
            if c["aws_load_balancer_type"] == "external" {
                extend(
                    &mut a,
                    &json!({"service.beta.kubernetes.io/aws-load-balancer-nlb-target-type":"ip","service.beta.kubernetes.io/aws-load-balancer-cross-zone-load-balancing-enabled":"true"}),
                );
            }
        } else if provider == Kind::Scw {
            a = json!({"service.beta.kubernetes.io/scw-loadbalancer-forward-port-algorithm":"leastconn","service.beta.kubernetes.io/scw-loadbalancer-protocol-http":"false","service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v1":"false","service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v2":"false","service.beta.kubernetes.io/scw-loadbalancer-health-check-type":"tcp","service.beta.kubernetes.io/scw-loadbalancer-use-hostname":"false"});
        }
        let dns =
            json!({"external-dns.alpha.kubernetes.io/hostname":c["fqdn"],"external-dns.alpha.kubernetes.io/ttl":"300"});
        if !aws {
            extend(&mut a, &dns);
        }
        if managed || (bitnami && database == DatabaseType::MongoDB) {
            for annotation in c["additional_annotations"].as_array().into_iter().flatten() {
                if let Some(key) = annotation["key"].as_str() {
                    a[key] = annotation["value"].clone();
                }
            }
        }
        if aws {
            extend(&mut a, &dns);
        }
        if managed || !bitnami || database == DatabaseType::MongoDB {
            extend(&mut a, &annotations("service"));
        }
        service["annotations"] = a;
    }
    let headless = json!({"annotations":annotations("service")});
    let expressions: Vec<_> = c["node_affinity"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key, value)| json!({"key":key,"operator":"In","values":[value]}))
        .collect();
    let tolerations: Vec<_> = c["toleration"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key, value)| json!({"key":key,"operator":"Exists","effect":value}))
        .collect();
    let mut scheduling = json!({"tolerations":tolerations});
    if !expressions.is_empty() {
        scheduling["affinity"] = json!({"nodeAffinity":{"requiredDuringSchedulingIgnoredDuringExecution":{"nodeSelectorTerms":[{"matchExpressions":expressions}]}}});
    }
    if bitnami {
        scheduling["nodeAffinityPreset"] =
            json!({"type":c["node_affinity_type"],"key":c["node_affinity_key"],"values":c["node_affinity_values"]});
    }
    let mut result = json!({"image":{"registry":c["registry_name"],"repository":c["repository_name"],"tag":c["version"]},"fullnameOverride":c["sanitized_name"],"commonLabels":common_labels});
    let mut auth = json!({"password":c["database_password"]});
    if database != DatabaseType::Redis {
        extend(
            &mut auth,
            &json!({"username":c["database_login"],"database":c["database_db_name"]}),
        );
    }
    match database {
        DatabaseType::MySQL => {
            auth["rootPassword"] = c["database_password"].clone();
            auth["username"] = json!("qovery");
            auth["database"] = c["sanitized_name"].clone();
        }
        DatabaseType::MongoDB => {
            auth["rootPassword"] = c["database_password"].clone();
            if !bitnami {
                auth["rootUsername"] = json!("root");
            }
        }
        DatabaseType::PostgreSQL if bitnami => auth["postgresPassword"] = c["database_password"].clone(),
        DatabaseType::Redis if bitnami => auth["enabled"] = json!(true),
        _ => {}
    }
    result["auth"] = auth;
    if !bitnami {
        service["port"] = c["database_port"].clone();
        service["headless"] = headless;
        extend(
            &mut result,
            &json!({"service":service,"persistence":persistence,"resources":resources,"podAnnotations":annotations("pods"),"statefulsetAnnotations":annotations("stateful_set")}),
        );
        extend(&mut result, &scheduling);
        if provider == Kind::Gcp && database == DatabaseType::Redis {
            result["podAnnotations"]["cluster-autoscaler.kubernetes.io/safe-to-evict"] = json!("false");
        }
        return result;
    }
    result["nameOverride"] = c["sanitized_name"].clone();
    result["rbac"] = json!({"create":true});
    let account = json!({"create":true,"name":c["sanitized_name"]});
    let shell = json!({"registry":c["registry_name"],"repository":c["repository_name_bitnami_shell"]});
    result["volumePermissions"] = json!({"enabled":true,"image":shell});
    let mut primary = json!({"resources":resources,"persistence":persistence,"service":service});
    if managed {
        primary["podAnnotations"] = annotations("pods");
    }
    if aws {
        extend(&mut primary, &scheduling);
    }
    match database {
        DatabaseType::PostgreSQL => {
            result["serviceAccount"] = account;
            result["audit"] = json!({"logHostname":true,"logConnectitrue":true,"logDisconnections":true});
            let env = json!([{"name":"POSTGRESQL_REPLICATION_USE_PASSFILE","value":"false"}]);
            primary["extraEnvVars"] = env.clone();
            primary["initdb"] = json!({"user":c["database_login"],"password":c["database_password"]});
            if managed {
                primary["annotations"] = annotations("stateful_set");
                if c["publicly_accessible"] == true {
                    primary["service"]["headless"] = headless;
                }
            }
            let mut replica = json!({"extraEnvVars":env,"podAnnotations":annotations("pods")});
            if aws {
                extend(&mut replica, &scheduling);
                let mut a = json!({"karpenter.sh/do-not-disrupt":"true","cluster-autoscaler.kubernetes.io/safe-to-evict":"false"});
                extend(&mut a, &annotations("pods"));
                replica["podAnnotations"] = a;
            }
            result["primary"] = primary;
            result["readReplicas"] = replica;
        }
        DatabaseType::MySQL => {
            result["nameOverride"] = json!(format!("{}-master", s("sanitized_name")));
            result["fullnameOverride"] = result["nameOverride"].clone();
            let mut pod_labels = legacy_labels;
            if managed {
                extend(&mut pod_labels, &c["labels_group"]["common"]);
                extend(
                    &mut primary["persistence"]["annotations"],
                    &json!({"qovery.com/service-id":c["long_id"],"qovery.com/service-type":"database","qovery.com/environment-id":c["environment_long_id"],"qovery.com/project-id":c["project_long_id"]}),
                );
                if c["publicly_accessible"] == true {
                    primary["service"]["headless"] = headless;
                }
            }
            if !managed {
                let mut pvc_annotations = base_labels;
                pvc_annotations.as_object_mut().unwrap().remove("envId");
                primary["persistence"]["annotations"] = pvc_annotations;
            }
            primary["podLabels"] = pod_labels;
            if aws {
                primary["pdb"] = json!({"create":false});
                result["readReplicas"] = scheduling;
            }
            result["primary"] = primary;
        }
        DatabaseType::Redis => {
            result["architecture"] = json!("standalone");
            primary["podLabels"] = legacy_labels;
            primary["serviceAccount"] = account;
            if provider == Kind::Gcp {
                primary["podAnnotations"]["cluster-autoscaler.kubernetes.io/safe-to-evict"] = json!("false");
            }
            result["master"] = primary;
            result["sysctlImage"] = shell;
            result["sysctlImage"]["enabled"] = json!(true);
            if managed {
                result["replica"] = json!({"podAnnotations":annotations("pods")});
                result["sentinel"] = json!({"service":{"headless":headless}});
            }
            if aws {
                result["pdb"] = json!({"create":false});
                extend(&mut result["replica"], &scheduling);
                // Preserve the legacy standalone overlay shape (replicas are disabled).
                if let Some(affinity) = result["replica"].as_object_mut().and_then(|v| v.remove("affinity")) {
                    result["replica"]["nodeAffinity"] = affinity["nodeAffinity"].clone();
                }
            }
        }
        DatabaseType::MongoDB => {
            result["useStatefulSet"] = json!(true);
            result["serviceAccount"] = account;
            result["volumePermissions"] =
                json!({"image":{"registry":c["registry_name"],"repository":c["repository_name_minideb"]}});
            extend(&mut result, &primary);
            result.as_object_mut().unwrap().remove("nodeAffinityPreset");
            result["service"].as_object_mut().unwrap().remove("name");
            result["service"]["nameOverride"] = c["service_name"].clone();
            result["service"]["ports"] = json!({"mongodb":s("database_port")});
            if managed {
                result["persistence"]["annotations"] = labels;
                result["annotations"] = annotations("stateful_set");
            }
            let major = s("version")
                .split('.')
                .next()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or_default();
            if major >= 6 {
                let command = if managed {
                    "db.adminCommand('ping')"
                } else {
                    "assert.eq(db.adminCommand('ping').ok, 1)"
                };
                let probe = json!({"exec":{"command":["mongosh","--eval",command]},"initialDelaySeconds":30,"periodSeconds":20,"timeoutSeconds":10,"successThreshold":1,"failureThreshold":6});
                result["customLivenessProbe"] = probe.clone();
                result["customReadinessProbe"] = probe;
            } else {
                let probe = json!({"enabled":true,"initialDelaySeconds":30,"periodSeconds":30,"timeoutSeconds":20,"successThreshold":1,"failureThreshold":6});
                result["livenessProbe"] = probe.clone();
                result["readinessProbe"] = probe;
            }
            if managed {
                for name in ["hidden", "arbiter"] {
                    result[name] = json!({"service":{"annotations":annotations("service")},"annotations":annotations("stateful_set"),"podAnnotations":annotations("pods")});
                    if aws {
                        extend(&mut result[name], &scheduling);
                    }
                }
                result["arbiter"]["service"]["headless"] = headless;
            }
            if aws {
                extend(&mut result, &scheduling);
            }
            if provider == Kind::Scw {
                for path in ["hidden", "arbiter"] {
                    result[path]["nodeAffinityPreset"] = scheduling["nodeAffinityPreset"].clone();
                }
                result["nodeAffinityPreset"] = scheduling["nodeAffinityPreset"].clone();
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn normalize(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.retain(|_, value| !value.is_null());
                for value in map.values_mut() {
                    normalize(value);
                }
                map.retain(|_, value| value.as_object().is_none_or(|v| !v.is_empty()));
            }
            Value::Array(values) => {
                for value in values {
                    normalize(value);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn provider_database_overlays_preserve_legacy_values() {
        let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/database_helm");
        let source: Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/database_helm/context.json")).unwrap();
        for (provider, name) in [
            (Kind::Aws, "aws"),
            (Kind::Azure, "azure"),
            (Kind::Gcp, "gcp"),
            (Kind::Scw, "scaleway"),
            (Kind::OnPremise, "self-managed"),
        ] {
            for (database, folder) in [
                (DatabaseType::PostgreSQL, "postgresql"),
                (DatabaseType::MySQL, "mysql"),
                (DatabaseType::MongoDB, "mongodb"),
                (DatabaseType::Redis, "redis"),
            ] {
                for bitnami in [false, true] {
                    for public in [false, true] {
                        let filename = format!(
                            "{name}-{folder}{}-{}.json",
                            if bitnami { "-bitnami" } else { "" },
                            if public { "public" } else { "private" }
                        );
                        let path = fixture_dir.join(&filename);
                        // Azure did not ship a PostgreSQL official-image overlay.
                        if !path.exists() {
                            assert!(provider == Kind::Azure && database == DatabaseType::PostgreSQL && !bitnami);
                            continue;
                        }
                        let mut context = source.clone();
                        context["publicly_accessible"] = json!(public);
                        if provider == Kind::OnPremise && database == DatabaseType::PostgreSQL && bitnami {
                            context["annotations_group"]["pods"] = json!({});
                        }
                        let mut actual =
                            values(&Context::from_value(context).unwrap(), provider.clone(), database, bitnami);
                        let mut expected: Value =
                            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
                        // The legacy Scaleway template accidentally indented the liveness probe under resources.
                        // Preserve the intended probe while producing a valid Kubernetes resources object.
                        if provider == Kind::Scw && database == DatabaseType::MongoDB && bitnami {
                            expected["customLivenessProbe"] = expected["customReadinessProbe"].clone();
                            expected["resources"]
                                .as_object_mut()
                                .unwrap()
                                .retain(|key, _| key == "requests" || key == "limits");
                        }
                        normalize(&mut actual);
                        normalize(&mut expected);
                        assert_eq!(actual, expected, "{filename}");
                    }
                }
            }
        }
    }
    #[test]
    fn database_charts_render_deployment_values_literally() {
        use base64::Engine;
        use base64::engine::general_purpose::STANDARD;
        use serde::Deserialize;
        use std::process::Command;
        let source: Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/database_helm/context.json")).unwrap();
        let payload = "quoted \" value\nInjected: true\n{{ fail \"must remain data\" }}";
        for (database, folder) in [
            (DatabaseType::PostgreSQL, "postgresql"),
            (DatabaseType::MySQL, "mysql"),
            (DatabaseType::MongoDB, "mongodb"),
            (DatabaseType::Redis, "redis"),
        ] {
            for bitnami in [false, true] {
                let mut context = source.clone();
                context["publicly_accessible"] = json!(true);
                context["database_password"] = json!(payload);
                context["database_login"] = json!(payload);
                context["database_db_name"] = json!(payload);
                context["annotations_group"] = json!({"service":{"test.example/literal":payload},"pods":{"test.example/literal":payload},"stateful_set":{"test.example/literal":payload}});
                context["labels_group"]["common"] = json!({"test.example/literal":payload});
                let override_values = values(&Context::from_value(context).unwrap(), Kind::Aws, database, bitnami);
                let tmp = tempfile::tempdir().unwrap();
                let override_path = tmp.path().join("values.json");
                std::fs::write(&override_path, serde_json::to_vec(&override_values).unwrap()).unwrap();
                let chart = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("lib/common/services/{folder}{}", if bitnami { "-bitnami" } else { "" }));
                let output = Command::new("helm")
                    .arg("template")
                    .arg("database-test")
                    .arg(chart)
                    .arg("-f")
                    .arg(override_path)
                    .output()
                    .expect("helm must be installed");
                assert!(
                    output.status.success(),
                    "{folder} bitnami={bitnami}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let rendered = String::from_utf8(output.stdout).unwrap();
                let docs: Vec<Value> = serde_yaml::Deserializer::from_str(&rendered)
                    .map(|doc| Value::deserialize(doc).unwrap())
                    .filter(|doc| !doc.is_null())
                    .collect();
                let service = docs
                    .iter()
                    .find(|doc| {
                        doc["kind"] == "Service" && doc["metadata"]["annotations"]["test.example/literal"] == payload
                    })
                    .expect("service annotation preserved literally");
                assert!(service.get("Injected").is_none());
                let secrets: Vec<_> = docs.iter().filter(|doc| doc["kind"] == "Secret").collect();
                assert!(!secrets.is_empty());
                assert!(
                    secrets
                        .iter()
                        .any(|secret| secret["data"]
                            .as_object()
                            .is_some_and(|data| data.values().any(|value| value
                                .as_str()
                                .is_some_and(|value| { value == STANDARD.encode(payload) })))),
                    "password preserved: {folder}"
                );
            }
        }
    }
}
