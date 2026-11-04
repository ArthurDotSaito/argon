mod sse;

use std::fmt;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures::{Stream, StreamExt, stream};
use reqwest::header::HeaderValue;
use reqwest::redirect::Policy;
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde_json::Value;

pub(crate) use sse::SseParser;

const USERNAME: &str = "opencode";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const ERROR_BODY_LIMIT: usize = 300;
const DIRECTORY_HEADER: &str = "X-Opencode-Directory";

pub(crate) fn basic_auth(password: &str) -> String {
    let credentials = format!("{USERNAME}:{password}");
    format!("Basic {}", STANDARD.encode(credentials))
}

pub(crate) struct LocalApi {
    base: String,
    auth: HeaderValue,
    directory: String,
    directory_header: Option<HeaderValue>,
    client: Client,
}

impl fmt::Debug for LocalApi {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalApi")
            .field("base", &self.base)
            .field("auth", &"<redacted>")
            .field("directory", &self.directory)
            .finish_non_exhaustive()
    }
}

impl LocalApi {
    pub fn new(base: String, password: &str, directory: String) -> Result<LocalApi, String> {
        let client = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .build()
            .map_err(|error| format!("failed to build the local HTTP client: {error}"))?;
        let mut auth = HeaderValue::from_str(&basic_auth(password))
            .map_err(|_| "invalid local server password".to_owned())?;
        auth.set_sensitive(true);
        let directory_header = directory
            .is_ascii()
            .then(|| HeaderValue::from_str(&directory).ok())
            .flatten();
        Ok(LocalApi {
            base: base.trim_end_matches('/').to_owned(),
            auth,
            directory,
            directory_header,
            client,
        })
    }

    pub async fn get(&self, path: &str) -> Result<Value, String> {
        let request = self.request(Method::GET, path).timeout(REQUEST_TIMEOUT);
        read_json(request).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let request = self
            .request(Method::POST, path)
            .timeout(REQUEST_TIMEOUT)
            .json(body);
        read_json(request).await
    }

    pub async fn events(
        &self,
        path: &str,
    ) -> Result<impl Stream<Item = Value> + Send + 'static, String> {
        let request = self
            .request(Method::GET, path)
            .header(reqwest::header::ACCEPT, "text/event-stream");
        let response = send_checked(request).await?;
        let parsed = response
            .bytes_stream()
            .scan(SseParser::new(), |parser, chunk| {
                std::future::ready(chunk.ok().map(|bytes| parser.push(&bytes)))
            })
            .flat_map(stream::iter);
        Ok(parsed)
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let url = format!("{}/{}", self.base, path.trim_start_matches('/'));
        let request = self
            .client
            .request(method, url)
            .header(reqwest::header::AUTHORIZATION, self.auth.clone());
        match &self.directory_header {
            Some(directory) => request.header(DIRECTORY_HEADER, directory.clone()),
            None => request,
        }
    }
}

async fn send_checked(request: RequestBuilder) -> Result<Response, String> {
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let excerpt: String = body.chars().take(ERROR_BODY_LIMIT).collect();
    Err(format!("HTTP {}: {excerpt}", status.as_u16()))
}

async fn read_json(request: RequestBuilder) -> Result<Value, String> {
    let response = send_checked(request).await?;
    if response.status() == StatusCode::NO_CONTENT {
        return Ok(Value::Null);
    }
    let body = response.text().await.map_err(|error| error.to_string())?;
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&body).map_err(|error| format!("invalid JSON response: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(directory: &str) -> LocalApi {
        LocalApi::new(
            "http://127.0.0.1:1/".to_owned(),
            "secret",
            directory.to_owned(),
        )
        .unwrap()
    }

    #[test]
    fn ascii_directories_are_sent_as_a_header() {
        let api = api(r"C:\proj\wt");
        assert_eq!(api.base, "http://127.0.0.1:1");
        assert_eq!(api.directory_header.unwrap(), r"C:\proj\wt");
    }

    #[test]
    fn non_ascii_directories_skip_the_header() {
        assert!(api(r"C:\Usuários\wt").directory_header.is_none());
    }

    #[test]
    fn debug_output_never_contains_the_credentials() {
        let api = api(r"C:\proj\wt");
        assert!(api.auth.is_sensitive());
        let printed = format!("{api:?}");
        assert!(!printed.contains(&basic_auth("secret")));
        assert!(!printed.contains("secret"));
    }
}
