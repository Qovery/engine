use crate::cloud_provider::{Edge, Kind as KindModel};
use serde_derive::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Aws,
    Do,
    Scw,
    Edge(Edge),
}

impl From<KindModel> for Kind {
    fn from(kind: KindModel) -> Self {
        match kind {
            KindModel::Aws => Kind::Aws,
            KindModel::Do => Kind::Do,
            KindModel::Scw => Kind::Scw,
            KindModel::Edge(Edge::Aws) => Kind::Edge(Edge::Aws),
        }
    }
}
