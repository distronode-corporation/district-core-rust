//! Typed calls for a phone number's regulatory registration: what a country's
//! regulator asks for, the workspace's filings, the documents on a draft, and
//! filing it with the carrier. The same six calls, with the same shapes, as the
//! Android app's Registrations tab.
//!
//! The workspace is a query parameter on every one of these routes, the writes
//! included. A viewer may read the requirements, and nothing else here. None
//! of the writes is repeated after a refused token; filing above all, which
//! files a regulated application in the business's name.

use district_model::{
    EndUserAttributes, NumberRegistrationCreateResponse, NumberRegistrationDraft,
    NumberRegistrationSubmitResponse, NumberRegistrationsResponse, NumberRequirementsResponse,
    RegistrationDocumentRemovalResponse, RegistrationDocumentResponse, RegistrationDocumentUpload,
};

use crate::client::ApiClient;
use crate::endpoints::Endpoint;
use crate::error::ApiError;
use crate::methods::confirm;
use crate::token::TokenSource;

impl<S: TokenSource> ApiClient<S> {
    /// What `country`'s regulator asks for, for `number_type` and
    /// `end_user_type`; either left out when `None`, so the route's defaults
    /// (`local`, `business`) apply. An answer whose
    /// [`requirements`](NumberRequirementsResponse::requirements) is `None`
    /// says no registration is needed; a failed lookup is an error.
    pub async fn number_requirements(
        &self,
        workspace_id: &str,
        country: &str,
        number_type: Option<&str>,
        end_user_type: Option<&str>,
    ) -> Result<NumberRequirementsResponse, ApiError> {
        let found: NumberRequirementsResponse = self
            .request(Endpoint::NumberRequirements)
            .workspace(workspace_id)
            .query("country", country)
            .query_opt("type", number_type)
            .query_opt("endUserType", end_user_type)
            .send()
            .await?;
        confirm(Endpoint::NumberRequirements, found.success)?;
        Ok(found)
    }

    /// `workspace_id`'s filings, newest first, and the countries open to it.
    /// A filing under review is read again from the carrier as this is asked.
    pub async fn number_registrations(
        &self,
        workspace_id: &str,
    ) -> Result<NumberRegistrationsResponse, ApiError> {
        let list: NumberRegistrationsResponse = self
            .request(Endpoint::NumberRegistrations)
            .workspace(workspace_id)
            .send()
            .await?;
        confirm(Endpoint::NumberRegistrations, list.success)?;
        Ok(list)
    }

    /// Starts the draft filing `draft`. Nothing reaches the carrier. A filing
    /// the workspace already has for the country and number type is a 409, and
    /// so is one for a combination that needs no registration. Sent once,
    /// never repeated.
    pub async fn create_number_registration(
        &self,
        workspace_id: &str,
        draft: &NumberRegistrationDraft,
    ) -> Result<NumberRegistrationCreateResponse, ApiError> {
        let created: NumberRegistrationCreateResponse = self
            .request(Endpoint::NumberRegistrationCreate)
            .workspace(workspace_id)
            .json(draft)
            .send()
            .await?;
        confirm(Endpoint::NumberRegistrationCreate, created.success)?;
        Ok(created)
    }

    /// Puts `upload` on its draft filing, replacing the document its
    /// requirement had. The route refuses a file over 10 MiB (413) and one whose
    /// contents are not a PDF, PNG or JPEG (415), and a filing that is no longer
    /// a draft (409). Sent once, never repeated.
    pub async fn upload_registration_document(
        &self,
        workspace_id: &str,
        upload: &RegistrationDocumentUpload,
    ) -> Result<RegistrationDocumentResponse, ApiError> {
        let stored: RegistrationDocumentResponse = self
            .request(Endpoint::RegistrationDocumentUpload)
            .workspace(workspace_id)
            .form_field("bundleId", upload.bundle_id.as_str())
            .form_field("requirementName", upload.requirement_name.as_str())
            .file(
                upload.file_name.clone(),
                upload.mime_type.clone(),
                upload.bytes.clone(),
            )
            .send()
            .await?;
        confirm(Endpoint::RegistrationDocumentUpload, stored.success)?;
        Ok(stored)
    }

    /// Takes document `document_id` off draft filing `bundle_id`. Sent once,
    /// never repeated.
    pub async fn delete_registration_document(
        &self,
        workspace_id: &str,
        bundle_id: &str,
        document_id: &str,
    ) -> Result<RegistrationDocumentRemovalResponse, ApiError> {
        let removed: RegistrationDocumentRemovalResponse = self
            .request(Endpoint::RegistrationDocumentDelete)
            .workspace(workspace_id)
            .field("bundleId", bundle_id)
            .field("documentId", document_id)
            .send()
            .await?;
        confirm(Endpoint::RegistrationDocumentDelete, removed.success)?;
        Ok(removed)
    }

    /// Files draft filing `bundle_id` with the carrier, with
    /// `end_user_attributes` (sent as an object even when empty). It files a
    /// regulated application in the business's name, so it is sent once,
    /// never repeated.
    ///
    /// A refusal is an [`ApiError::Envelope`] with a 422: `validation_failed`
    /// when details or documents are missing (nothing was sent), or
    /// `carrier_rejected` when the carrier refused it, whose reasons the
    /// filing holds when the list is read again.
    pub async fn submit_number_registration(
        &self,
        workspace_id: &str,
        bundle_id: &str,
        end_user_attributes: &EndUserAttributes,
    ) -> Result<NumberRegistrationSubmitResponse, ApiError> {
        let filed: NumberRegistrationSubmitResponse = self
            .request(Endpoint::NumberRegistrationSubmit)
            .workspace(workspace_id)
            .field("bundleId", bundle_id)
            .field("endUserAttributes", end_user_attributes)
            .send()
            .await?;
        confirm(Endpoint::NumberRegistrationSubmit, filed.success)?;
        Ok(filed)
    }
}
