//! Typed calls for supervising a live call: listening in, a private note to
//! the AI (a whisper), and taking the call over by voice.
//!
//! The service finds the media server that holds a call's room by asking for
//! the room, never from the workspace's region, and answers with that server;
//! the whisper it delivers itself. Joining the room is the call engine's
//! work, not this crate's.

use district_model::{
    CallMonitorTokenResponse, CallTakeoverResponse, CallWhisperResponse, LiveCallId,
};

use crate::client::ApiClient;
use crate::endpoints::Endpoint;
use crate::error::ApiError;
use crate::methods::confirm;
use crate::token::TokenSource;

impl<S: TokenSource> ApiClient<S> {
    /// A listen-only credential for the live call `call_id`'s room, and the
    /// media server that holds it.
    ///
    /// The service works out the workspace from the call and checks the
    /// member belongs to it, so no workspace is sent; every role may listen.
    /// The credential publishes nothing. A call that started before calls had
    /// rooms is refused (`call_predates_feature`), and one the service cannot
    /// find is [`ApiError::NotFound`].
    ///
    /// It stores nothing and spends nothing (it signs a short-lived
    /// credential), so a refused access token is refreshed and the request
    /// sent once more, like a read. The answer prints no secret in `Debug`.
    pub async fn call_monitor_token(
        &self,
        call_id: &LiveCallId,
    ) -> Result<CallMonitorTokenResponse, ApiError> {
        let credential: CallMonitorTokenResponse = self
            .request(Endpoint::CallRoomToken)
            .field("roomName", call_id.as_str())
            // Required by the route and ignored: the service makes the
            // participant's identity from the session.
            .field("identity", self.platform().wire())
            .send()
            .await?;
        confirm(Endpoint::CallRoomToken, credential.success)?;
        Ok(credential)
    }

    /// Sends `whisper`, one private note, to the AI on the live call
    /// `call_id` of `workspace_id`. The caller never hears it.
    ///
    /// A viewer is refused. A call that is not in progress any more is
    /// refused (`call_not_in_progress`), and so is a note sent while the
    /// service could not reach the call's media server
    /// (`service_unavailable`): either way nothing was delivered, and the note
    /// should be kept. Sent once and never repeated automatically: the AI
    /// would act on it twice.
    pub async fn call_whisper(
        &self,
        workspace_id: &str,
        call_id: &str,
        whisper: &str,
    ) -> Result<CallWhisperResponse, ApiError> {
        let answer: CallWhisperResponse = self
            .request(Endpoint::CallWhisper)
            .workspace(workspace_id)
            .field("callId", call_id)
            .field("whisper", whisper)
            .send()
            .await?;
        confirm(Endpoint::CallWhisper, answer.success)?;
        Ok(answer)
    }

    /// A microphone-only credential that takes the live call `call_id` of
    /// `workspace_id` over from the AI, and the media server that holds its
    /// room.
    ///
    /// Joining with it is what makes the AI announce the handover and step
    /// back; leaving the room hands the call back. A viewer is refused; so is
    /// a call that is not in progress or whose room has gone
    /// (`call_not_in_progress`, a 409), and one the service could not look for
    /// (`service_unavailable`). Never repeated automatically. The answer
    /// prints no secret in `Debug`.
    pub async fn call_takeover(
        &self,
        workspace_id: &str,
        call_id: &str,
    ) -> Result<CallTakeoverResponse, ApiError> {
        let grant: CallTakeoverResponse = self
            .request(Endpoint::CallTakeover)
            .path_param("callId", call_id)
            .workspace(workspace_id)
            .send()
            .await?;
        confirm(Endpoint::CallTakeover, grant.success)?;
        Ok(grant)
    }
}
