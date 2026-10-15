//! Typed calls for the account's own settings: the language it speaks.

use district_model::{LanguageSave, SettingsSaveResponse};

use crate::client::ApiClient;
use crate::endpoints::Endpoint;
use crate::error::ApiError;
use crate::methods::confirm;
use crate::token::TokenSource;

impl<S: TokenSource> ApiClient<S> {
    /// Tells the account which language the app shows: `locale` is
    /// [`SETTINGS_LANGUAGE_FRENCH`](district_model::SETTINGS_LANGUAGE_FRENCH) or
    /// [`SETTINGS_LANGUAGE_ENGLISH`](district_model::SETTINGS_LANGUAGE_ENGLISH). The
    /// rest of the account's settings are kept. Never repeated after a refused
    /// access token.
    pub async fn save_language(&self, locale: &str) -> Result<SettingsSaveResponse, ApiError> {
        let answer: SettingsSaveResponse = self
            .request(Endpoint::SaveSettings)
            .json(&LanguageSave::new(locale))
            .send()
            .await?;
        confirm(Endpoint::SaveSettings, answer.success)?;
        Ok(answer)
    }
}
