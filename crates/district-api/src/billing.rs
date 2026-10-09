//! Typed calls for billing: the workspace's plan and the account's
//! subscriptions and invoices, read only, and the hand-off that signs a browser
//! in to the service's checkout or billing page.
//!
//! Nothing here changes a plan, cancels a subscription or touches a card. The
//! service has routes for those; this client does not call them. A purchase
//! happens on the service's own checkout page, which the hand-off opens.

use district_model::{AccountBillingResponse, BillingHandOffResponse, WorkspaceBillingResponse};

use crate::client::ApiClient;
use crate::endpoints::Endpoint;
use crate::error::ApiError;
use crate::methods::confirm;
use crate::token::TokenSource;

impl<S: TokenSource> ApiClient<S> {
    /// `workspace_id`'s plan, from the service's own records.
    pub async fn workspace_billing(
        &self,
        workspace_id: &str,
    ) -> Result<WorkspaceBillingResponse, ApiError> {
        let plan: WorkspaceBillingResponse = self
            .request(Endpoint::WorkspaceBilling)
            .workspace(workspace_id)
            .send()
            .await?;
        confirm(Endpoint::WorkspaceBilling, plan.success)?;
        Ok(plan)
    }

    /// The signed-in account's subscriptions and invoices, from the payment
    /// processor. Scoped to the account, so it names no workspace.
    ///
    /// This answer has no `success` flag to check: an empty body is
    /// [`ApiError::Decode`], and an unreachable payment processor is a success
    /// with [`billing_unavailable`](AccountBillingResponse::billing_unavailable)
    /// set, which is to be said rather than shown as an account without billing.
    pub async fn account_billing(&self) -> Result<AccountBillingResponse, ApiError> {
        self.request(Endpoint::StripeBilling).send().await
    }

    /// A link that signs a browser in to the checkout or the billing page,
    /// landing on `next`.
    ///
    /// `next` must be exactly what the service admits: `/checkout`, with only
    /// `tier`, `term`, `promo` and `reason`, each at most once and each in the
    /// exact shape the checkout page reads, or `/dashboard/district/billing`
    /// with no query. Anything else is a 400 with
    /// [`CODE_INVALID_NEXT`](district_model::CODE_INVALID_NEXT), never a
    /// fallback, so build it from `district_core`'s billing destinations rather
    /// than by hand.
    ///
    /// `workspace_id` is the workspace open in the app, if any; the service
    /// records it with the code and authorises nothing by it, and without one
    /// the key is left out. `nonce` binds the link to one browser, as for
    /// [`scheduling_hand_off`](Self::scheduling_hand_off), with the same two
    /// refusals.
    ///
    /// The link is a one-time credential, good for one sign-in within a
    /// minute: open it at once and never log, store or cache it. Sent once,
    /// never repeated.
    pub async fn billing_hand_off(
        &self,
        workspace_id: Option<&str>,
        next: &str,
        nonce: Option<&str>,
    ) -> Result<BillingHandOffResponse, ApiError> {
        self.request(Endpoint::BillingHandOff)
            .field("next", next)
            .optional_field("workspaceId", workspace_id)
            .optional_field("nonce", nonce)
            .send()
            .await
    }
}
