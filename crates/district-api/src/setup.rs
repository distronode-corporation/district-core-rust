//! Typed calls for the setup wizard: the step saves, the website lookup, the
//! test call, the call forwarding check, and the connections read its later
//! steps make.
//!
//! Every `setup/*` route is the workspace owner's alone: anyone else gets
//! [`ApiError::Forbidden`], and a workspace from before the wizard a 409 with the
//! code `setup_not_available`. Both keep the service's answer. Buying a number,
//! the plan and a calendar provider's sign-in are not here: they charge a card or
//! need a browser, and stay on the web.
//!
//! None of the writes is ever repeated. The test call and the forwarding check
//! each place a real call, and the service limits both per day.

use district_l10n::Locale;
use district_model::{
    ConnectionsResponse, ForwardingCheckMode, ForwardingCheckResponse, SetupLookupResponse,
    SetupResponse, SetupStep, SetupStepState, SetupTestCallResponse,
};
use serde_json::{Map, Value};

use crate::client::ApiClient;
use crate::endpoints::Endpoint;
use crate::error::ApiError;
use crate::methods::confirm;
use crate::token::TokenSource;

impl<S: TokenSource> ApiClient<S> {
    /// Saves step states and business facts sections of `workspace_id`, and
    /// answers the wizard's state after the write.
    ///
    /// Only `steps` go inside `setupProgress` (the service strips anything else
    /// there), and a step left out keeps its state. Each section in
    /// `business_facts` replaces the stored one whole, a `null` section clears
    /// it, and a section left out is kept. With neither, only the workspace is
    /// sent.
    pub async fn save_setup(
        &self,
        workspace_id: &str,
        steps: &[(SetupStep, SetupStepState)],
        business_facts: Option<&Map<String, Value>>,
    ) -> Result<SetupResponse, ApiError> {
        let progress = (!steps.is_empty()).then(|| {
            let states: Map<String, Value> = steps
                .iter()
                .map(|(step, state)| (step.as_str().to_owned(), Value::from(state.as_str())))
                .collect();
            serde_json::json!({ "steps": states })
        });
        self.request(Endpoint::SetupSave)
            .workspace(workspace_id)
            .optional_field("setupProgress", progress)
            .optional_field("businessFacts", business_facts)
            .send()
            .await
    }

    /// A draft of the business's facts from the page at `website_url`, written
    /// in `locale`. Writes nothing. A page the service cannot read is a 422 with
    /// the code `website_unreadable`.
    pub async fn look_up_website(
        &self,
        workspace_id: &str,
        website_url: &str,
        locale: Locale,
    ) -> Result<SetupLookupResponse, ApiError> {
        self.request(Endpoint::SetupLookup)
            .workspace(workspace_id)
            .field("locale", lookup_locale(locale))
            .field("websiteUrl", website_url)
            .send()
            .await
    }

    /// Has the receptionist call `phone`, the owner's own. The answer names the
    /// call. The service records the owner's consent in its own words.
    pub async fn place_test_call(
        &self,
        workspace_id: &str,
        phone: &str,
    ) -> Result<SetupTestCallResponse, ApiError> {
        let placed: SetupTestCallResponse = self
            .request(Endpoint::SetupTestCall)
            .workspace(workspace_id)
            .field("phone", phone)
            .send()
            .await?;
        confirm(Endpoint::SetupTestCall, placed.success)?;
        Ok(placed)
    }

    /// The latest call forwarding check of `workspace_id`'s business line.
    pub async fn forwarding_check(
        &self,
        workspace_id: &str,
    ) -> Result<ForwardingCheckResponse, ApiError> {
        self.request(Endpoint::ForwardingCheck)
            .workspace(workspace_id)
            .send()
            .await
    }

    /// Starts a check that `business_line` forwards `mode` calls to the District
    /// number. Answers the pending check; poll
    /// [`forwarding_check`](Self::forwarding_check) while it is pending.
    pub async fn start_forwarding_check(
        &self,
        workspace_id: &str,
        business_line: &str,
        mode: ForwardingCheckMode,
    ) -> Result<ForwardingCheckResponse, ApiError> {
        self.request(Endpoint::ForwardingCheckStart)
            .workspace(workspace_id)
            .field("businessLine", business_line)
            .field("mode", mode)
            .send()
            .await
    }

    /// The providers `workspace_id` is connected to, and the calendar the
    /// receptionist books into. Every member may read it; a viewer gets the
    /// rows without the calendar.
    pub async fn connections(&self, workspace_id: &str) -> Result<ConnectionsResponse, ApiError> {
        self.request(Endpoint::Connections)
            .workspace(workspace_id)
            .send()
            .await
    }
}

/// The lookup's `locale`: the route knows `en` and `fr`.
fn lookup_locale(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "en",
        Locale::FrCa => "fr",
    }
}
