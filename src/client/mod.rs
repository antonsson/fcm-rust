pub(crate) mod response;

use crate::client::response::{ErrorReason, FcmError, FcmResponse, RetryAfter};
use crate::{Message, MessageInternal};
use reqwest::header::RETRY_AFTER;
use reqwest::{Body, StatusCode};
use serde::Serialize;

/// An async client for sending the notification payload.
pub struct Client {
    http_client: reqwest::Client,
}

// will be used to wrap the message in a "message" field
#[derive(Serialize)]
struct MessageWrapper<'a> {
    #[serde(rename = "message")]
    message: &'a MessageInternal,
}

impl MessageWrapper<'_> {
    fn new(message: &MessageInternal) -> MessageWrapper {
        MessageWrapper { message }
    }
}

impl Client {
    /// Get a new instance of Client.
    pub fn new() -> Client {
        let http_client = reqwest::ClientBuilder::new()
            .pool_max_idle_per_host(usize::MAX)
            .build()
            .unwrap();

        Client { http_client }
    }

    pub async fn send(&self, access_token: &str, project_id: &str, message: Message) -> Result<FcmResponse, FcmError> {
        let fin = message.finalize();
        let wrapper = MessageWrapper::new(&fin);
        let payload = serde_json::to_vec(&wrapper).unwrap();

        // https://firebase.google.com/docs/reference/fcm/rest/v1/projects.messages/send
        let url = format!("https://fcm.googleapis.com/v1/projects/{}/messages:send", project_id);

        let request = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", access_token)
            .body(Body::from(payload))
            .build()?;

        let response = self.http_client.execute(request).await?;

        let response_status = response.status();

        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|ra| ra.to_str().ok())
            .and_then(|ra| ra.parse::<RetryAfter>().ok());

        match response_status {
            StatusCode::OK => {
                let fcm_response: FcmResponse = response.json().await.unwrap();

                match fcm_response.error {
                    Some(ErrorReason::Unavailable) => Err(FcmError::ServerError(retry_after)),
                    Some(ErrorReason::InternalServerError) => Err(FcmError::ServerError(retry_after)),
                    _ => Ok(fcm_response),
                }
            }
            StatusCode::UNAUTHORIZED => Err(FcmError::Unauthorized),
            StatusCode::BAD_REQUEST => {
                let body = response.text().await.unwrap();
                Err(FcmError::InvalidMessage(format!("Bad Request ({body}")))
            }
            status if status.is_server_error() => Err(FcmError::ServerError(retry_after)),
            _ => Err(FcmError::InvalidMessage("Unknown Error".to_string())),
        }
    }
}
