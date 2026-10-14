//! Typed calls for managing the numbers a workspace holds and its carrier
//! account: configuring and releasing a number, the billed lookup, the carriers'
//! status, SIP trunks, the one-time passcode sender, and the A2P 10DLC and
//! toll-free registrations.
//!
//! Buying a number is not here: it stays on the web.
//!
//! A viewer may read the carriers' status, the SIP trunks and the passcode
//! sender; every other call refuses one. None of the writes is ever repeated,
//! and neither is the lookup, which the carrier bills for.

use district_model::{
    A2pRegistrationDraft, A2pRegistrationResponse, NumberConfigureResponse, NumberLookupResponse,
    NumberReleaseResponse, ProviderStatusResponse, SipTrunkCreateResponse, SipTrunkDraft,
    SipTrunksResponse, TollFreeVerificationDraft, TollFreeVerificationResponse,
    VerifyServiceResponse,
};

use crate::client::ApiClient;
use crate::endpoints::Endpoint;
use crate::error::ApiError;
use crate::methods::confirm;
use crate::token::TokenSource;

impl<S: TokenSource> ApiClient<S> {
    /// Points `phone_number`'s calls and messages back at `workspace_id`, and
    /// binds it again to its region's bridge. The same every time, so safe to
    /// send again; still sent once per press.
    ///
    /// A number another workspace holds is a 403 with the code
    /// `phone_number_not_found`; a number on a carrier the workspace has no
    /// credentials for is a 409.
    pub async fn configure_number(
        &self,
        workspace_id: &str,
        phone_number: &str,
    ) -> Result<NumberConfigureResponse, ApiError> {
        let done: NumberConfigureResponse = self
            .request(Endpoint::NumberConfigure)
            .workspace(workspace_id)
            .field("phoneNumber", phone_number)
            .send()
            .await?;
        confirm(Endpoint::NumberConfigure, done.success)?;
        Ok(done)
    }

    /// Gives `phone_number` back to the carrier. It cannot be had back: every
    /// call and message to it stops. Sent once, never repeated.
    ///
    /// A success may carry [`warnings`](NumberReleaseResponse::warnings): the
    /// number is gone and a step after that did not finish. A 502 means nothing
    /// was changed.
    pub async fn release_number(
        &self,
        workspace_id: &str,
        phone_number: &str,
    ) -> Result<NumberReleaseResponse, ApiError> {
        let released: NumberReleaseResponse = self
            .request(Endpoint::NumberRelease)
            .workspace(workspace_id)
            .field("phoneNumber", phone_number)
            .send()
            .await?;
        confirm(Endpoint::NumberRelease, released.success)?;
        Ok(released)
    }

    /// What the carrier knows about `phone_number`. The carrier bills every
    /// call, an invalid number included, so call it only when the member asks,
    /// and never again by itself.
    pub async fn lookup_number(
        &self,
        workspace_id: &str,
        phone_number: &str,
    ) -> Result<NumberLookupResponse, ApiError> {
        let found: NumberLookupResponse = self
            .request(Endpoint::NumberLookup)
            .workspace(workspace_id)
            .query("phoneNumber", phone_number)
            .send()
            .await?;
        confirm(Endpoint::NumberLookup, found.success)?;
        Ok(found)
    }

    /// Which carriers `workspace_id` has credentials for, and whether each
    /// accepts them. The answer has no `success` flag.
    pub async fn provider_status(
        &self,
        workspace_id: &str,
    ) -> Result<ProviderStatusResponse, ApiError> {
        self.request(Endpoint::ProviderStatus)
            .workspace(workspace_id)
            .send()
            .await
    }

    /// `workspace_id`'s SIP trunks.
    pub async fn sip_trunks(&self, workspace_id: &str) -> Result<SipTrunksResponse, ApiError> {
        let trunks: SipTrunksResponse = self
            .request(Endpoint::SipTrunks)
            .workspace(workspace_id)
            .send()
            .await?;
        confirm(Endpoint::SipTrunks, trunks.success)?;
        Ok(trunks)
    }

    /// Creates the SIP trunk `draft` on the workspace's carrier account. The
    /// carrier bills it monthly and nothing here removes it. Sent once, never
    /// repeated: a failure part of the way can leave what was made.
    pub async fn create_sip_trunk(
        &self,
        workspace_id: &str,
        draft: &SipTrunkDraft,
    ) -> Result<SipTrunkCreateResponse, ApiError> {
        let created: SipTrunkCreateResponse = self
            .request(Endpoint::SipTrunkCreate)
            .workspace(workspace_id)
            .json(draft)
            .send()
            .await?;
        confirm(Endpoint::SipTrunkCreate, created.success)?;
        Ok(created)
    }

    /// Whether `workspace_id`'s one-time passcode sender is in use.
    pub async fn verify_service(
        &self,
        workspace_id: &str,
    ) -> Result<VerifyServiceResponse, ApiError> {
        let state: VerifyServiceResponse = self
            .request(Endpoint::VerifyService)
            .workspace(workspace_id)
            .send()
            .await?;
        confirm(Endpoint::VerifyService, state.success)?;
        Ok(state)
    }

    /// Turns the one-time passcode sender on (creating one the carrier bills
    /// for) or off (it stays on the carrier account). The answer is the state
    /// stored. Sent once, never repeated.
    pub async fn set_verify_service(
        &self,
        workspace_id: &str,
        enabled: bool,
    ) -> Result<VerifyServiceResponse, ApiError> {
        let state: VerifyServiceResponse = self
            .request(Endpoint::VerifyServiceSet)
            .workspace(workspace_id)
            .field("enabled", enabled)
            .send()
            .await?;
        confirm(Endpoint::VerifyServiceSet, state.success)?;
        Ok(state)
    }

    /// Files the A2P 10DLC registration `draft`. The carrier charges for the
    /// brand and monthly for the campaign, and a repeat files them again, so it
    /// is sent once, never repeated. A refusal with the code
    /// `TRUST_BUNDLES_NOT_APPROVED` is the usual first answer: an account step
    /// is still to be done.
    pub async fn register_a2p(
        &self,
        workspace_id: &str,
        draft: &A2pRegistrationDraft,
    ) -> Result<A2pRegistrationResponse, ApiError> {
        let filed: A2pRegistrationResponse = self
            .request(Endpoint::A2pRegistration)
            .workspace(workspace_id)
            .json(draft)
            .send()
            .await?;
        confirm(Endpoint::A2pRegistration, filed.success)?;
        Ok(filed)
    }

    /// Files the toll-free verification `draft`, which a carrier reviews by
    /// hand over days. Sent once, never repeated.
    pub async fn verify_toll_free(
        &self,
        workspace_id: &str,
        draft: &TollFreeVerificationDraft,
    ) -> Result<TollFreeVerificationResponse, ApiError> {
        let filed: TollFreeVerificationResponse = self
            .request(Endpoint::TollFreeVerification)
            .workspace(workspace_id)
            .json(draft)
            .send()
            .await?;
        confirm(Endpoint::TollFreeVerification, filed.success)?;
        Ok(filed)
    }
}
