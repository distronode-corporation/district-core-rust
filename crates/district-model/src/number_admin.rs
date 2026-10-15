//! Managing the numbers a workspace holds, and the carrier account behind them:
//! configuring and releasing a number, the billed number lookup, which carriers
//! are connected, SIP trunks, the one-time passcode sender, and the A2P 10DLC and
//! toll-free registrations.
//!
//! Buying a number is not here and is not for this app: a number is a recurring
//! carrier charge, and it is bought on the web.
//!
//! The service records no response of these routes for the Android app, which
//! has no screen for them, so no contract fixture holds these shapes yet. Each
//! type says which route answers it and how the route builds the answer.
//!
//! The request types leave out the workspace: the API client adds it where each
//! route reads it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::scheduling_admin::nullish;

/// `POST /api/district/workspace/numbers/configure`: the number's calls and
/// messages pointed back at the workspace. The service answers nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct NumberConfigureResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
}

/// `POST /api/district/workspace/numbers/release`: the number is gone.
///
/// A success may carry [`warnings`](Self::warnings), which do not make it a
/// partial release: the number went back to the carrier, and a step after that
/// did not finish (a trunk still lists it, or its monthly charge could not be
/// ended). Show them; leaving them out would tell a member a charge stopped
/// that did not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct NumberReleaseResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The steps after the release that did not finish, in the service's words.
    /// Sent only when there are some.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// `GET /api/district/workspace/lookup`: what the carrier knows about one
/// number. Billed by the carrier for every call, an invalid number included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct NumberLookupResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The answer.
    pub info: NumberLookupInfo,
}

/// What the carrier knows about one number. The three details are sent as
/// `null`, never left out, when the carrier did not say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct NumberLookupInfo {
    /// Whether the carrier recognised the number. `false` still cost a lookup.
    pub valid: bool,
    /// The number as the carrier writes it, or as asked when it did not.
    pub phone_number: String,
    /// Its ISO 3166-1 alpha-2 country.
    #[serde(default)]
    pub country: Option<String>,
    /// Its line type: `mobile`, `landline`, `voip` and so on.
    #[serde(rename = "type", default)]
    pub line_type: Option<String>,
    /// The carrier that serves it.
    #[serde(default)]
    pub carrier: Option<String>,
}

/// `GET /api/district/workspace/provider/status`: which carriers the workspace
/// has credentials for, and whether each one accepts them.
///
/// There is no `success` flag. A workspace with no credentials at all answers
/// `{"connected": false, "provider": null}`; any other answers
/// [`providers`](Self::providers), keyed by carrier (`twilio`, `sinch`,
/// `telnyx`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatusResponse {
    /// Whether any carrier accepted its credentials.
    pub connected: bool,
    /// Each carrier with credentials, by its id. Absent when there are none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub providers: Option<BTreeMap<String, ProviderStatusEntry>>,
    /// Sent as `null` only with no credentials at all, and left out otherwise:
    /// `None` left out, `Some(None)` null. Never a carrier's name.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub provider: Option<Option<String>>,
}

impl ProviderStatusResponse {
    /// The carriers that accepted their credentials, in order of id.
    pub fn connected_providers(&self) -> Vec<&str> {
        self.providers_where(true)
    }

    /// The carriers that refused their own credentials, in order of id: a
    /// different state from "not connected", fixed by reconnecting.
    pub fn refused_providers(&self) -> Vec<&str> {
        self.providers_where(false)
    }

    fn providers_where(&self, connected: bool) -> Vec<&str> {
        self.providers
            .iter()
            .flatten()
            .filter(|(_, entry)| entry.connected == connected)
            .map(|(id, _)| id.as_str())
            .collect()
    }
}

/// One carrier's state. A carrier held by Distronode on the workspace's behalf
/// reports only that it is connected: its account's name, balance and numbers
/// are Distronode's, not the workspace's. Each detail is left out, not sent as
/// `null`, when the route has none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatusEntry {
    /// Whether the carrier accepted the credentials.
    pub connected: bool,
    /// `true` for a carrier Distronode holds on the workspace's behalf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed: Option<bool>,
    /// The carrier account's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_name: Option<String>,
    /// The carrier account's status, in the carrier's word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The prepaid balance, as the carrier writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<String>,
    /// The balance's currency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// How many numbers the account holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_count: Option<i64>,
    /// Why the carrier is not connected, in the service's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// `GET /api/district/workspace/sip`: the workspace's SIP trunks. A viewer may
/// read them. There is no route that removes one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SipTrunksResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The trunks.
    #[serde(default)]
    pub trunks: Vec<SipTrunk>,
}

/// `POST /api/district/workspace/sip`: the trunk created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SipTrunkCreateResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The trunk.
    pub trunk: SipTrunk,
}

/// One SIP trunk on the workspace's own carrier account: a SIP domain with the
/// addresses allowed to use it, billed monthly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SipTrunk {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its full domain, `<label>.sip.twilio.com`.
    pub domain: String,
    /// The IPv4 addresses and ranges allowed to use it. Can be empty.
    #[serde(default)]
    pub ip_access_control_list: Vec<String>,
    /// The carrier's id of the domain. `null` for a trunk recorded before the
    /// carrier's resources were created with it.
    #[serde(default)]
    pub domain_sid: Option<String>,
    /// The carrier's id of the address list, `null` likewise.
    #[serde(default)]
    pub ip_acl_sid: Option<String>,
    /// Its billing state, for example `active`.
    #[serde(default)]
    pub status: String,
}

impl SipTrunk {
    /// Whether the carrier holds nothing for this trunk: a record with no
    /// domain and no address list behind it.
    pub fn unprovisioned(&self) -> bool {
        self.domain_sid.is_none() && self.ip_acl_sid.is_none()
    }
}

/// The body of `POST /api/district/workspace/sip`, less the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SipTrunkDraft {
    /// The trunk's name. No `|`.
    pub name: String,
    /// The domain's label only, letters, digits and hyphens: the carrier adds
    /// `.sip.twilio.com`.
    pub domain: String,
    /// At least one IPv4 address or range (`203.0.113.7`, `203.0.113.0/24`).
    pub ip_access_control_list: Vec<String>,
}

/// `GET` and `POST /api/district/workspace/verify`: whether the workspace has a
/// one-time passcode sender on its carrier account.
///
/// Turning it on creates a sender the carrier bills for; turning it off stops
/// the service using it and leaves it on the carrier account.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct VerifyServiceResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// Whether the sender is in use.
    #[serde(default)]
    pub enabled: bool,
    /// The sender's id at the carrier, while it is in use. `null` from the read
    /// and left out of the answer to turning it off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_service_sid: Option<String>,
}

/// The body of `POST /api/district/workspace/a2p`, less the workspace: an A2P
/// 10DLC brand and campaign registration, which the carrier charges for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2pRegistrationDraft {
    /// The business's legal name.
    pub business_name: String,
    /// Its employer identification or company number. Kept for the carrier's
    /// business profile, not sent to the registration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ein: Option<String>,
    /// Its website.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    /// Its industry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical: Option<String>,
    /// One of [`A2P_CAMPAIGN_TYPES`]. It decides the brand and the price:
    /// `LOW_VOLUME` is a sole proprietor's brand, anything else a standard one.
    pub campaign_type: String,
    /// What the business will send.
    pub campaign_description: String,
    /// An example message.
    #[serde(rename = "sampleMessage1")]
    pub sample_message_1: String,
    /// Another example message.
    #[serde(rename = "sampleMessage2")]
    pub sample_message_2: String,
}

/// The campaign types the service's registration recognises. Anything else it
/// files as a mixed campaign rather than refusing.
pub const A2P_CAMPAIGN_TYPES: [&str; 4] = ["CUSTOMER_CARE", "LOW_VOLUME", "MARKETING", "2FA"];

/// `POST /api/district/workspace/a2p`: what the carrier said when the
/// registration was filed. There is no route that reads it again, so this is
/// the only report the registration gives.
///
/// The usual first answer is not this at all but a refusal with the code
/// `TRUST_BUNDLES_NOT_APPROVED`: the carrier's business profile is a manual,
/// once per account step that has not been done. The business details are kept
/// even then.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct A2pRegistrationResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The campaign's status at the carrier, `IN_PROGRESS` when it named none.
    pub status: String,
    /// The brand's id at the carrier.
    pub brand_sid: String,
    /// The messaging service's id at the carrier.
    pub messaging_service_sid: String,
    /// The campaign's id at the carrier.
    pub campaign_sid: String,
}

/// The body of `POST /api/district/workspace/tfv`, less the workspace: a
/// toll-free number's verification, which a carrier reviews by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TollFreeVerificationDraft {
    /// The toll-free number, one the workspace holds on its own carrier
    /// account.
    pub phone_number: String,
    /// The business's name.
    pub business_name: String,
    /// Its website.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    /// One of [`TOLL_FREE_USE_CASES`]. Anything else is filed as "other".
    pub use_case: String,
    /// An example message. The service writes one of its own when left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_content: Option<String>,
    /// How people agree to receive messages. The service writes a sentence of
    /// its own when left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opt_in_flow: Option<String>,
    /// Public links to the business's own opt-in form or a screenshot of it,
    /// which the carrier's reviewer opens. At least one; the service refuses
    /// none.
    pub opt_in_image_urls: Vec<String>,
}

/// The use cases the service's toll-free verification recognises.
pub const TOLL_FREE_USE_CASES: [&str; 4] = [
    "Customer Support",
    "Marketing",
    "Notifications",
    "2FA / OTP",
];

/// `POST /api/district/workspace/tfv`: what the carrier said when the
/// verification was filed. There is no route that reads it again, and the
/// review that follows takes days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct TollFreeVerificationResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The verification's status at the carrier.
    pub status: String,
    /// The verification's id at the carrier, to quote to support.
    pub tfv_sid: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
        serde_json::from_value(value).expect("decodes")
    }

    /// Both answers of the carrier status route, the one with no credentials
    /// and the one with carriers, decode into one type.
    #[test]
    fn the_carrier_status_has_two_shapes_and_one_type() {
        let none: ProviderStatusResponse = decode(json!({"connected": false, "provider": null}));
        assert!(!none.connected);
        assert!(none.providers.is_none());
        assert!(none.connected_providers().is_empty());
        let some: ProviderStatusResponse = decode(json!({
            "connected": true,
            "providers": {
                "twilio": {"connected": true, "accountName": "Main", "status": "active",
                           "balance": "12.50", "currency": "USD", "numberCount": 3},
                "telnyx": {"connected": false, "error": "Invalid credentials"},
                "sinch": {"connected": true, "managed": true}
            }
        }));
        assert_eq!(some.connected_providers(), ["sinch", "twilio"]);
        assert_eq!(some.refused_providers(), ["telnyx"]);
        assert!(serde_json::from_value::<ProviderStatusResponse>(json!({})).is_err());
    }

    /// A lookup's details are `null` when the carrier did not say, and stay so
    /// when written again.
    #[test]
    fn a_lookups_unknown_details_are_null() {
        let invalid = json!({"success": true, "info": {
            "valid": false, "phoneNumber": "+12125550142",
            "country": null, "type": null, "carrier": null
        }});
        let decoded: NumberLookupResponse = decode(invalid.clone());
        assert!(!decoded.info.valid);
        assert_eq!(serde_json::to_value(&decoded).unwrap(), invalid);
    }

    #[test]
    fn a_trunk_without_carrier_ids_is_unprovisioned() {
        let trunk: SipTrunk = decode(json!({
            "id": "item_1", "name": "Office", "domain": "office.sip.twilio.com",
            "ipAccessControlList": [], "domainSid": null, "ipAclSid": null, "status": "active"
        }));
        assert!(trunk.unprovisioned());
        let provisioned = SipTrunk {
            domain_sid: Some("SD1".to_owned()),
            ..trunk
        };
        assert!(!provisioned.unprovisioned());
    }

    #[test]
    fn a_release_says_what_did_not_finish() {
        let clean: NumberReleaseResponse = decode(json!({"success": true}));
        assert!(clean.warnings.is_empty());
        let warned: NumberReleaseResponse = decode(
            json!({"success": true, "warnings": ["The monthly charge could not be ended."]}),
        );
        assert_eq!(warned.warnings.len(), 1);
    }

    /// The request bodies use the route's names, and leave a blank optional
    /// out rather than sending it.
    #[test]
    fn the_registrations_are_sent_in_the_routes_words() {
        let a2p = A2pRegistrationDraft {
            business_name: "Acme".to_owned(),
            ein: None,
            website: Some("https://example.com".to_owned()),
            vertical: None,
            campaign_type: A2P_CAMPAIGN_TYPES[0].to_owned(),
            campaign_description: "Appointment reminders".to_owned(),
            sample_message_1: "One".to_owned(),
            sample_message_2: "Two".to_owned(),
        };
        assert_eq!(
            serde_json::to_value(&a2p).unwrap(),
            json!({
                "businessName": "Acme", "website": "https://example.com",
                "campaignType": "CUSTOMER_CARE", "campaignDescription": "Appointment reminders",
                "sampleMessage1": "One", "sampleMessage2": "Two"
            })
        );
        let tfv = TollFreeVerificationDraft {
            phone_number: "+18005550142".to_owned(),
            business_name: "Acme".to_owned(),
            website: None,
            use_case: TOLL_FREE_USE_CASES[0].to_owned(),
            message_content: None,
            opt_in_flow: None,
            opt_in_image_urls: vec!["https://example.com/opt-in.png".to_owned()],
        };
        assert_eq!(
            serde_json::to_value(&tfv).unwrap(),
            json!({
                "phoneNumber": "+18005550142", "businessName": "Acme",
                "useCase": "Customer Support",
                "optInImageUrls": ["https://example.com/opt-in.png"]
            })
        );
    }
}
