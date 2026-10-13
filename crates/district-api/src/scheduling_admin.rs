//! The booking scheduler's administration: one typed method for every
//! operation of the service's catalog, the upload, and the scheduler sign-in a
//! calendar connection starts with.
//!
//! The service owns the catalog (the scheduler path, the method, the parameters'
//! schema and what the answer may carry), so this client sends an operation's
//! name and its parameters, never a path, and holds a second copy of none of
//! it. What it owns is the envelope, which the catalog does not describe and
//! every operation shares: `{ok: true, data}`, or `{ok: false, failure,
//! status}` with a 200 when the scheduler refused. That refusal is an answer
//! ([`SchedulingAdminReply::Refused`]), not an [`ApiError`]: reading it as the
//! caller's type would either fail, blaming the contract for an outage, or,
//! for a type whose every field may be missing, succeed and show an outage as
//! data.
//!
//! Nothing here retries. The service already makes one fresh attempt when the
//! scheduler refuses the member's key; a loop here would multiply that, and 41
//! of the 64 operations are writes.

use district_model::{
    SchedulingAdminFailure, SchedulingAdminReply, SchedulingAdminRequest, SchedulingSecret,
};
use serde_json::Value;

use crate::client::ApiClient;
use crate::endpoints::Endpoint;
use crate::error::ApiError;
use crate::token::TokenSource;

impl<S: TokenSource> ApiClient<S> {
    /// Runs `request` for `workspace_id` as the signed-in member, and reads the
    /// answer as the type its operation answers.
    ///
    /// The route's own refusals are errors as anywhere else: a workspace
    /// without booking pages is an [`ApiError::Envelope`] with
    /// [`CODE_SCHEDULING_NOT_READY`](district_model::CODE_SCHEDULING_NOT_READY),
    /// parameters the schema refused one with
    /// [`CODE_INVALID_PARAMS`](district_model::CODE_INVALID_PARAMS), an
    /// operation above the member's role [`ApiError::Forbidden`], and too many
    /// writes in an hour [`ApiError::RateLimited`]. Sent once, never repeated.
    pub async fn scheduling_admin(
        &self,
        workspace_id: &str,
        request: &SchedulingAdminRequest,
    ) -> Result<SchedulingAdminReply, ApiError> {
        let (endpoint, body): (Endpoint, Value) =
            if let SchedulingAdminRequest::Upload(upload) = request {
                let body = self
                    .request(Endpoint::SchedulingAdminUpload)
                    .workspace(workspace_id)
                    .form_field("target", upload.target.as_str())
                    .file(
                        upload.file_name.clone(),
                        upload.mime_type.clone(),
                        upload.bytes.clone(),
                    )
                    .send()
                    .await?;
                (Endpoint::SchedulingAdminUpload, body)
            } else {
                let op = request.op().map_or("", |op| op.as_str());
                let body = self
                    .request(Endpoint::SchedulingAdmin)
                    .workspace(workspace_id)
                    .field("op", op)
                    .field("params", request.params())
                    .send()
                    .await?;
                (Endpoint::SchedulingAdmin, body)
            };
        read_reply(endpoint, request, body)
    }
}

impl<S: TokenSource> ApiClient<S> {
    /// A single-use link that signs the browser in to `workspace_id`'s
    /// scheduler and lands on `next`, a path inside the scheduler: how a member
    /// reaches a calendar provider's consent screen. Read from the route's
    /// redirect and never followed here, so the credential in it is spent by the
    /// browser it is opened in and nothing else. Open it at once and keep it
    /// nowhere; its `Debug` output is redacted.
    ///
    /// A workspace whose booking pages are not ready is [`ApiError::Conflict`];
    /// a `next` into the scheduler's retired console is a 410.
    pub async fn scheduling_sign_in(
        &self,
        workspace_id: &str,
        next: &str,
    ) -> Result<SchedulingSecret, ApiError> {
        self.request(Endpoint::SchedulingSignIn)
            .workspace(workspace_id)
            .query("next", next)
            .send_for_location()
            .await
            .map(SchedulingSecret::new)
    }
}

/// The envelope's flag first, then the payload: a refusal is read as one, and
/// a success's `data` as what the request's operation answers.
fn read_reply(
    endpoint: Endpoint,
    request: &SchedulingAdminRequest,
    mut body: Value,
) -> Result<SchedulingAdminReply, ApiError> {
    let decoded = match body.get("ok") {
        Some(Value::Bool(false)) => serde_json::from_value::<SchedulingAdminFailure>(body)
            .map(SchedulingAdminReply::Refused),
        Some(Value::Bool(true)) => request
            .decode(body["data"].take())
            .map(SchedulingAdminReply::Done),
        // Neither shape: a body this client cannot read, and not one a retry
        // fixes.
        _ => Err(serde::de::Error::custom("an answer with no `ok` flag")),
    };
    // Read from a value, so the parser has no position to give. Nothing of the
    // body is kept: bookings and answers to booking questions are customer data.
    decoded.map_err(|error| ApiError::Decode {
        endpoint,
        line: error.line(),
        column: error.column(),
    })
}
