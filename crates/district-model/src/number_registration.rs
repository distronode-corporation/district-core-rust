//! A phone number's regulatory registration: what a country's regulator asks
//! for before a number there can be bought, the workspace's filings (the
//! carrier calls one a "bundle"), the documents on a filing, and filing it.
//!
//! The service records no response of these routes as a contract fixture yet,
//! so each type says which route answers it and how the route builds the
//! answer, as the Android app's `NumberProvisioningResponses.kt` reads them.
//! Every timestamp is an ISO-8601 string.
//!
//! A filing's `status`, its `numberType` and `endUserType`, and a
//! requirement's `kind` are wire values: a screen labels them through the wire
//! vocabulary (`numberRegistrationStatus`, `numberRegistrationNumberType`,
//! `numberRegistrationEndUserType`, `numberRequirementKind`), never as sent.
//!
//! The request types leave out the workspace: the API client adds it, in the
//! query, on every one of these routes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The status of a filing that is still the workspace's to edit, add documents
/// to and file. Every other status is with the carrier.
pub const REGISTRATION_DRAFT: &str = "draft";

/// The two kinds of requirement a regulation lists.
pub const REQUIREMENT_END_USER: &str = "end_user";
/// A requirement met by a document.
pub const REQUIREMENT_SUPPORTING_DOCUMENT: &str = "supporting_document";

/// The carrier's regulatory number types, as the registration routes take and
/// answer them: `toll free` with its space, unlike the owned numbers' own
/// `tollFree`. The first is the routes' default.
pub const REGISTRATION_NUMBER_TYPES: [&str; 4] = ["local", "mobile", "national", "toll free"];

/// Who a filing is for. The routes read anything but `individual` as
/// `business`, their default.
pub const REGISTRATION_END_USER_TYPES: [&str; 2] = ["business", "individual"];

/// `GET /api/district/workspace/numbers/requirements`: what a country's
/// regulator asks for one number type and end-user type.
///
/// [`requirements`](Self::requirements) is `null` when the country publishes no
/// regulation for that number type: nothing needs filing. A failed lookup is an
/// error, never this.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct NumberRequirementsResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The country asked about, in capitals.
    pub country: String,
    /// The number type asked about (`local` when none was).
    pub number_type: String,
    /// Whether numbers in the country can be bought through Distronode at all,
    /// a registration or not.
    pub purchasable: bool,
    /// The regulation, or `None` when none applies.
    #[serde(default)]
    pub requirements: Option<CountryRequirements>,
}

/// The regulation of one country, number type and end-user type.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct CountryRequirements {
    /// The country.
    pub iso_country: String,
    /// The number type: a `numberRegistrationNumberType` value.
    pub number_type: String,
    /// The end-user type: a `numberRegistrationEndUserType` value.
    pub end_user_type: String,
    /// The regulation's id at the carrier.
    pub regulation_sid: String,
    /// The regulation's name, as the carrier writes it.
    pub friendly_name: String,
    /// What it asks for.
    #[serde(default)]
    pub requirements: Vec<RegulatoryRequirement>,
}

/// One thing a regulation asks for: details about the end user, or a
/// document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct RegulatoryRequirement {
    /// [`REQUIREMENT_END_USER`] or [`REQUIREMENT_SUPPORTING_DOCUMENT`]: a
    /// `numberRequirementKind` value.
    pub kind: String,
    /// Its name, as the carrier writes it.
    pub name: String,
    /// The name a document upload names it by.
    pub requirement_name: String,
    /// The carrier's type for it.
    #[serde(rename = "type")]
    pub requirement_type: String,
    /// The field names it asks for: for an end-user requirement, the keys of a
    /// filing's end-user attributes.
    #[serde(default)]
    pub fields: Vec<String>,
    /// For a document requirement, the documents that meet it: any one does.
    /// Left out for an end-user requirement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_documents: Option<Vec<AcceptedDocument>>,
}

impl RegulatoryRequirement {
    /// Whether a document meets it, rather than details about the end user.
    pub fn is_document(&self) -> bool {
        self.kind == REQUIREMENT_SUPPORTING_DOCUMENT
    }
}

/// A document a regulation accepts for one requirement.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct AcceptedDocument {
    /// Its name, as the carrier writes it.
    pub name: String,
    /// The carrier's type for it.
    #[serde(rename = "type")]
    pub document_type: String,
    /// The fields the document itself must carry.
    #[serde(default)]
    pub fields: Vec<String>,
}

/// `GET /api/district/workspace/numbers/registrations`: the workspace's
/// filings, newest first, and the countries open to it.
///
/// The service reads a filing under review again from the carrier when this is
/// asked (no more than once per filing), so a status moves only when the list
/// is read. A carrier that does not answer leaves the stored status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct NumberRegistrationsResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The filings, newest first.
    #[serde(default)]
    pub registrations: Vec<NumberRegistration>,
    /// The countries this workspace's own approved filings have opened.
    #[serde(default)]
    pub approved_countries: Vec<String>,
    /// The countries Distronode's own filings cover: purchasable without one.
    #[serde(default)]
    pub platform_countries: Vec<String>,
}

/// One filing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct NumberRegistration {
    /// The filing's id: the `bundleId` of the document and submit routes.
    pub id: String,
    /// Its country.
    pub iso_country: String,
    /// Its number type: a `numberRegistrationNumberType` value.
    pub number_type: String,
    /// Who it is for: a `numberRegistrationEndUserType` value.
    pub end_user_type: String,
    /// The name it has in the carrier's console, or `null`.
    #[serde(default)]
    pub friendly_name: Option<String>,
    /// A `numberRegistrationStatus` value: `draft`, `pending-review`,
    /// `in-review`, `twilio-approved` or `twilio-rejected`. The carrier can add
    /// one without notice. Only [`REGISTRATION_DRAFT`] may be edited or filed.
    pub status: String,
    /// What the carrier objected to, in the service's sentences. Empty unless
    /// it refused the filing.
    #[serde(default)]
    pub rejection_reasons: Vec<String>,
    /// When it was filed, or `null`.
    #[serde(default)]
    pub submitted_at: Option<String>,
    /// When the carrier decided, or `null`.
    #[serde(default)]
    pub reviewed_at: Option<String>,
    /// When it was started.
    pub created_at: String,
    /// Its documents.
    #[serde(default)]
    pub documents: Vec<RegistrationDocument>,
}

impl NumberRegistration {
    /// Whether it is still the workspace's to change and file.
    pub fn is_draft(&self) -> bool {
        self.status == REGISTRATION_DRAFT
    }
}

/// One document on a filing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct RegistrationDocument {
    /// The document's id, which a removal names.
    pub id: String,
    /// The requirement it meets.
    pub requirement_name: String,
    /// Its type: PDF, PNG or JPEG.
    pub mime_type: String,
    /// Its size in bytes.
    pub size_bytes: u64,
    /// Whether the service holds the file. `false` is a row recorded before a
    /// file arrived.
    pub stored: bool,
    /// Whether the carrier holds a copy: only after the filing was filed.
    pub submitted: bool,
    /// When it last changed.
    pub updated_at: String,
}

/// `POST /api/district/workspace/numbers/registrations`: a new draft filing.
/// Nothing reaches the carrier until it is filed.
///
/// [`number_type`](Self::number_type) and
/// [`end_user_type`](Self::end_user_type) are left out when `None`, so the
/// route's defaults (`local`, `business`) apply.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NumberRegistrationDraft {
    /// A two-letter country code.
    pub iso_country: String,
    /// One of [`REGISTRATION_NUMBER_TYPES`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_type: Option<String>,
    /// One of [`REGISTRATION_END_USER_TYPES`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_user_type: Option<String>,
    /// The name it gets in the carrier's console, at most 120 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friendly_name: Option<String>,
}

/// The answer to starting a filing, with a 201.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct NumberRegistrationCreateResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The draft.
    pub registration: NumberRegistration,
}

/// One document to put on a draft filing, held in memory. The route takes a
/// PDF, PNG or JPEG of at most 10 MiB, checked by its contents, and replaces
/// the document the requirement already has.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RegistrationDocumentUpload {
    /// The filing.
    pub bundle_id: String,
    /// The requirement it meets: a requirement's
    /// [`requirement_name`](RegulatoryRequirement::requirement_name).
    pub requirement_name: String,
    /// The file's name.
    pub file_name: String,
    /// The file's type.
    pub mime_type: String,
    /// The file.
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for RegistrationDocumentUpload {
    /// The file's bytes are an identity document: they are counted, never
    /// printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistrationDocumentUpload")
            .field("bundle_id", &self.bundle_id)
            .field("requirement_name", &self.requirement_name)
            .field("file_name", &self.file_name)
            .field("mime_type", &self.mime_type)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// `POST /api/district/workspace/numbers/registrations/documents`: the
/// document stored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct RegistrationDocumentResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The document.
    pub document: UploadedRegistrationDocument,
}

/// A document as the upload route answers it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct UploadedRegistrationDocument {
    /// The document's id.
    pub id: String,
    /// The requirement it meets.
    pub requirement_name: String,
    /// Its type, as the route read it from the contents.
    pub mime_type: String,
    /// Its size in bytes.
    pub size_bytes: u64,
    /// When it was first recorded.
    pub created_at: String,
}

/// `DELETE /api/district/workspace/numbers/registrations/documents`: the
/// document removed from a draft filing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct RegistrationDocumentRemovalResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The document removed.
    pub document_id: String,
}

/// `POST /api/district/workspace/numbers/registrations/submit`: the filing
/// filed with the carrier.
///
/// A refusal is a 422 with a code: `validation_failed`, with `missingFields`
/// (end-user details not given) or `missingRequirements` (documents not
/// uploaded), and nothing sent; or `carrier_rejected`, with `reasons`, which
/// the filing's [`rejection_reasons`](NumberRegistration::rejection_reasons)
/// also hold once the list is read again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct NumberRegistrationSubmitResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The filing.
    pub registration: SubmittedRegistration,
}

/// A filing as the submit route answers it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SubmittedRegistration {
    /// The filing's id.
    pub id: String,
    /// Its status at the carrier now: a `numberRegistrationStatus` value.
    pub status: String,
    /// When it was filed.
    pub submitted_at: String,
}

/// The end-user details a filing is submitted with, by the field names its
/// regulation's end-user requirements list. Sent as an object even when empty.
pub type EndUserAttributes = BTreeMap<String, String>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
        serde_json::from_value(value).expect("decodes")
    }

    /// No regulation is an answer of its own, `null`, not a missing key.
    #[test]
    fn a_country_with_no_regulation_answers_null() {
        let none: NumberRequirementsResponse = decode(json!({
            "success": true, "country": "CA", "numberType": "local",
            "purchasable": true, "requirements": null
        }));
        assert_eq!(none.requirements, None);
        let some: NumberRequirementsResponse = decode(json!({
            "success": true, "country": "DE", "numberType": "mobile", "purchasable": false,
            "requirements": {
                "isoCountry": "DE", "numberType": "mobile", "endUserType": "business",
                "regulationSid": "RN1", "friendlyName": "Germany mobile",
                "requirements": [
                    {"kind": "end_user", "name": "Business", "requirementName": "business_info",
                     "type": "business", "fields": ["business_name"]},
                    {"kind": "supporting_document", "name": "Proof of address",
                     "requirementName": "proof_of_address", "type": "document", "fields": [],
                     "acceptedDocuments": [{"name": "Utility bill", "type": "utility_bill",
                                            "fields": ["address_sids"]}]}
                ]
            }
        }));
        let regulation = some.requirements.expect("a regulation");
        assert!(!regulation.requirements[0].is_document());
        assert!(regulation.requirements[1].is_document());
        assert_eq!(
            regulation.requirements[1]
                .accepted_documents
                .as_ref()
                .unwrap()[0]
                .document_type,
            "utility_bill"
        );
    }

    /// The draft leaves out what was not chosen, so the route's defaults
    /// apply.
    #[test]
    fn a_draft_leaves_out_what_was_not_chosen() {
        let draft = NumberRegistrationDraft {
            iso_country: "DE".to_owned(),
            ..NumberRegistrationDraft::default()
        };
        assert_eq!(
            serde_json::to_value(&draft).unwrap(),
            json!({"isoCountry": "DE"})
        );
    }

    /// An upload's bytes never reach a log.
    #[test]
    fn an_uploads_bytes_are_not_printed() {
        let upload = RegistrationDocumentUpload {
            bundle_id: "BU1".to_owned(),
            requirement_name: "proof".to_owned(),
            file_name: "passport.pdf".to_owned(),
            mime_type: "application/pdf".to_owned(),
            bytes: b"%PDF-secret".to_vec(),
        };
        let printed = format!("{upload:?}");
        assert!(printed.contains("bytes: 11"), "{printed}");
        assert!(!printed.contains("secret"), "{printed}");
    }
}
