//! Typed calls for District HQ, the workspace assistant: a prompt, and the
//! confirmation of a change it proposed.
//!
//! Both are one route, `POST /api/district/hq`, and both run a model; the
//! confirmation also changes the workspace. Neither is ever repeated, even after
//! a refused access token: one call is one model run, and one confirmation is
//! one change. They are two methods with two answer types, so a change being
//! applied can never be read as a reply.
//!
//! By voice, a session's credential comes from its own route, and a change the
//! voice agent proposed is confirmed on the same route as a typed one, with
//! the proposal's id, or cancelled on a route of its own. None of the three is
//! ever repeated either: a credential starts a billed voice session, and the
//! other two are writes.

use district_l10n::Locale;
use district_model::{
    HQ_VOICE_HISTORY_MAX_CHARS, HQ_VOICE_HISTORY_MAX_TURNS, HqConfirmResponse, HqPendingWrite,
    HqPromptResponse, HqProposalCancelResponse, HqTurn, HqVoiceProposal, HqVoiceTokenResponse,
};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::client::ApiClient;
use crate::endpoints::Endpoint;
use crate::error::ApiError;
use crate::methods::confirm;
use crate::token::TokenSource;

/// The `confirm` object of a confirmation: the proposal's write and arguments,
/// borrowed so they go back exactly as they came.
///
/// A typed proposal's confirmation has no `proposalId` at all: the service
/// takes any `proposalId` it is sent, even an empty one, as a voice card's.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Confirmation<'a> {
    tool: &'a str,
    args: &'a Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    proposal_id: Option<&'a str>,
}

/// The language a voice session speaks, as the voice credential's route reads
/// it: `fr` for French, anything else English.
fn voice_locale(locale: Locale) -> &'static str {
    match locale {
        Locale::En => "en",
        Locale::FrCa => "fr",
    }
}

/// The earlier turns a voice session starts with: the last
/// [`HQ_VOICE_HISTORY_MAX_TURNS`] with any text, each trimmed and cut to
/// [`HQ_VOICE_HISTORY_MAX_CHARS`] characters. The service bounds them again; this
/// only keeps the request small.
fn voice_history(history: &[HqTurn]) -> Vec<HqTurn> {
    let kept: Vec<HqTurn> = history
        .iter()
        .filter(|turn| !turn.text.trim().is_empty())
        .map(|turn| HqTurn {
            role: turn.role,
            text: turn
                .text
                .trim()
                .chars()
                .take(HQ_VOICE_HISTORY_MAX_CHARS)
                .collect(),
        })
        .collect();
    let skip = kept.len().saturating_sub(HQ_VOICE_HISTORY_MAX_TURNS);
    kept.into_iter().skip(skip).collect()
}

impl<S: TokenSource> ApiClient<S> {
    /// Asks the assistant `prompt` about `workspace_id`, after the earlier turns
    /// in `history`, oldest first.
    ///
    /// The service keeps no conversation, so the caller holds it and sends it
    /// every time; the service reads the last six turns. An empty history is
    /// left out of the request, as the Android app leaves it out.
    ///
    /// When the answer proposes a change, nothing has been changed:
    /// [`hq_confirm`](Self::hq_confirm) applies it, and only when the member
    /// asks. Each call is a billed model run and is never repeated.
    pub async fn hq_prompt(
        &self,
        workspace_id: &str,
        prompt: &str,
        history: &[HqTurn],
    ) -> Result<HqPromptResponse, ApiError> {
        let answer: HqPromptResponse = self
            .request(Endpoint::Hq)
            .workspace(workspace_id)
            .field("prompt", prompt)
            .optional_field("history", (!history.is_empty()).then_some(history))
            .send()
            .await?;
        confirm(Endpoint::Hq, answer.success)?;
        Ok(answer)
    }

    /// Applies the change `proposal` describes, which the member has read (as
    /// [`HqPendingWrite::summary`]) and confirmed.
    ///
    /// The write and its arguments are sent back exactly as the service proposed
    /// them. The answer echoes what was applied, which
    /// [`HqConfirmResponse::is_the_proposal`] checks; a success with
    /// [`executed`](HqConfirmResponse::executed) false changed nothing. Sent
    /// once and never repeated, even after a refused access token.
    pub async fn hq_confirm(
        &self,
        workspace_id: &str,
        proposal: &HqPendingWrite,
    ) -> Result<HqConfirmResponse, ApiError> {
        let confirmation = Confirmation {
            tool: &proposal.tool,
            args: &proposal.args,
            proposal_id: None,
        };
        let applied: HqConfirmResponse = self
            .request(Endpoint::Hq)
            .workspace(workspace_id)
            .field("confirm", confirmation)
            .send()
            .await?;
        confirm(Endpoint::Hq, applied.success)?;
        Ok(applied)
    }

    /// The credential for a voice session in `workspace_id`, speaking `locale`,
    /// after the earlier turns in `history`, oldest first (the last few are
    /// sent, see [`HQ_VOICE_HISTORY_MAX_TURNS`]; none is left out of the
    /// request).
    ///
    /// The service refuses a viewer. The answer carries the media token and the
    /// room's passphrase and prints neither in `Debug`; a room without a
    /// passphrase must not be joined
    /// ([`HqVoiceTokenResponse::is_joinable`]). Each call starts a billed voice
    /// session and is never repeated, even after a refused access token.
    pub async fn hq_voice_token(
        &self,
        workspace_id: &str,
        locale: Locale,
        history: &[HqTurn],
    ) -> Result<HqVoiceTokenResponse, ApiError> {
        let history = voice_history(history);
        let credential: HqVoiceTokenResponse = self
            .request(Endpoint::HqVoiceToken)
            .workspace(workspace_id)
            .field("locale", voice_locale(locale))
            .optional_field("history", (!history.is_empty()).then_some(&history))
            .send()
            .await?;
        confirm(Endpoint::HqVoiceToken, credential.success)?;
        Ok(credential)
    }

    /// Applies the change the voice agent proposed in `proposal`, which the
    /// member read and clicked Confirm on.
    ///
    /// The write and its arguments go back exactly as the agent sent them, with
    /// the proposal's id; the service runs its own stored copy, and only if
    /// they match. The answer is the same as a typed confirmation's
    /// ([`HqConfirmResponse::is_the_voice_proposal`] checks it). A proposal
    /// already resolved, by voice or another click, is
    /// [`ApiError::Conflict`] with the code `already_resolved`; one too old to
    /// confirm is the code `expired`. Sent once and never repeated.
    pub async fn hq_confirm_voice(
        &self,
        workspace_id: &str,
        proposal: &HqVoiceProposal,
    ) -> Result<HqConfirmResponse, ApiError> {
        let confirmation = Confirmation {
            tool: &proposal.tool,
            args: &proposal.args,
            proposal_id: Some(&proposal.proposal_id),
        };
        let applied: HqConfirmResponse = self
            .request(Endpoint::Hq)
            .workspace(workspace_id)
            .field("confirm", confirmation)
            .send()
            .await?;
        confirm(Endpoint::Hq, applied.success)?;
        Ok(applied)
    }

    /// Cancels the change the voice agent proposed as `proposal_id`. The same
    /// refusals as [`hq_confirm_voice`](Self::hq_confirm_voice). Sent once and
    /// never repeated.
    pub async fn hq_proposal_cancel(
        &self,
        workspace_id: &str,
        proposal_id: &str,
    ) -> Result<HqProposalCancelResponse, ApiError> {
        let cancelled: HqProposalCancelResponse = self
            .request(Endpoint::HqProposalCancel)
            .workspace(workspace_id)
            .field("proposalId", proposal_id)
            .send()
            .await?;
        confirm(Endpoint::HqProposalCancel, cancelled.success)?;
        Ok(cancelled)
    }
}

#[cfg(test)]
mod tests {
    use district_model::HqRole;

    use super::*;

    fn turn(role: HqRole, text: &str) -> HqTurn {
        HqTurn {
            role,
            text: text.to_owned(),
        }
    }

    #[test]
    fn a_voice_session_starts_with_the_last_turns_trimmed_and_cut() {
        let mut history: Vec<HqTurn> = (0..8)
            .map(|n| turn(HqRole::User, &format!(" turn {n} ")))
            .collect();
        history.push(turn(HqRole::Model, "   "));
        history.push(turn(HqRole::Model, &"\u{e9}".repeat(600)));
        let sent = voice_history(&history);
        assert_eq!(sent.len(), HQ_VOICE_HISTORY_MAX_TURNS);
        assert_eq!(sent[0].text, "turn 3");
        assert_eq!(sent[4].text, "turn 7");
        assert_eq!(sent[5].role, HqRole::Model);
        assert_eq!(sent[5].text.chars().count(), HQ_VOICE_HISTORY_MAX_CHARS);
        assert!(voice_history(&[]).is_empty());
    }

    #[test]
    fn the_session_speaks_french_only_when_the_app_does() {
        assert_eq!(voice_locale(Locale::FrCa), "fr");
        assert_eq!(voice_locale(Locale::En), "en");
    }
}
