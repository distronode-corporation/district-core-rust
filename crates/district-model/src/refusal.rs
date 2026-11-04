//! A route's own refusal body.

use serde::{Deserialize, Serialize};

/// `{success: false, error, code}`: a route refusing a request it understood,
/// with the English sentence to show and the machine-readable code the
/// localization tables key on. The API client reads failures leniently (see
/// `district-api`'s `ErrorDetail`); this type is the strict shape, for the
/// contract tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct RefusalResponse {
    /// `false`.
    #[serde(default)]
    pub success: bool,
    /// The service's sentence, in English.
    pub error: String,
    /// The machine-readable code, when the route sends one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}
