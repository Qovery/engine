// this fn should implements the algorythm describe here: https://qovery.atlassian.net/secure/RapidBoard.jspa?rapidView=10&modal=detail&selectedIssue=DEV-283
pub fn get_firsts_namespaces_to_delete(namespaces: Vec<&str>) -> Vec<&str> {
    // from all namesapce remove managed and never delete namespaces
    let minus_managed = minus_namespaces(namespaces, get_qovery_managed_namespaces());
    let minus_qovery_managed_and_never_delete = minus_namespaces(minus_managed, get_never_delete_namespaces());
    minus_qovery_managed_and_never_delete
}

fn minus_namespaces<'a>(all: Vec<&'a str>, to_remove_namespaces: Vec<&str>) -> Vec<&'a str> {
    let reduced = all
        .into_iter()
        .filter(|item| !to_remove_namespaces.contains(item))
        .collect();
    return reduced;
}

pub fn get_qovery_managed_namespaces() -> Vec<&'static str> {
    // the order is very important because of dependencies
    vec!["logging", "nginx-ingress", "qovery", "cert-manager", "prometheus"]
}

fn get_never_delete_namespaces() -> Vec<&'static str> {
    vec!["default", "kube-node-lease", "kube-public", "kube-system"]
}
