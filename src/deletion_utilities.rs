// this fn should implements the algorythm describe here: https://qovery.atlassian.net/secure/RapidBoard.jspa?rapidView=10&modal=detail&selectedIssue=DEV-283
pub fn get_firsts_namespaces_to_delete(namespaces: Vec<&str>) -> Vec<&str> {
    // from all namesapce remove managed and never delete namespaces
    let minus_managed = minus_namespaces(namespaces, get_qovery_managed_namespaces());
    let minus_qovery_managed_and_never_delete =
        minus_namespaces(minus_managed, get_never_delete_namespaces());
    minus_qovery_managed_and_never_delete
}

fn minus_namespaces<'a>(all: Vec<&'a str>, to_remove_namespaces: Vec<&str>) -> Vec<&'a str> {
    let reduced = all
        .into_iter()
        .filter(|item| !to_remove_namespaces.contains(item))
        .collect();
    return reduced;
}

// TODO: use label instead
// TODO: create enum: deletion_rule [system, qovery,..]
pub fn get_qovery_managed_namespaces() -> Vec<&'static str> {
    let mut qovery_managed_namespaces = Vec::with_capacity(5);
    qovery_managed_namespaces.push("logging");
    qovery_managed_namespaces.push("nginx-ingress");
    qovery_managed_namespaces.push("qovery");
    qovery_managed_namespaces.push("cert-manager");
    qovery_managed_namespaces.push("prometheus");
    return qovery_managed_namespaces;
}

// TODO: use label instead
fn get_never_delete_namespaces() -> Vec<&'static str> {
    let mut kubernetes_never_delete_namespaces = Vec::with_capacity(4);
    kubernetes_never_delete_namespaces.push("default");
    kubernetes_never_delete_namespaces.push("kube-node-lease");
    kubernetes_never_delete_namespaces.push("kube-public");
    kubernetes_never_delete_namespaces.push("kube-system");
    return kubernetes_never_delete_namespaces;
}
