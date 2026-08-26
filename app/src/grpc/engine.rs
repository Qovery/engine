#![allow(clippy::enum_variant_names)]
// tonic generates `Result<_, tonic::Status>` everywhere and `Status` is 176 bytes, over the
// `result_large_err` threshold. Nothing to fix on our side, the type comes from tonic.
#![allow(clippy::result_large_err)]

use tonic::include_proto;

include_proto!("engine");
