use std::time::Duration;

use reqwest::Method;

use crate::client::GithubClient;
use crate::error::Result;
use crate::rest::repo_path;

const LOG_LIMIT: usize = 5 * 1024 * 1024;
const LOG_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobLog {
    pub text: String,
    pub truncated: bool,
}

struct TailBuffer {
    limit: usize,
    bytes: Vec<u8>,
    truncated: bool,
}

impl TailBuffer {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            bytes: Vec::new(),
            truncated: false,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
        if self.bytes.len() > self.limit * 2 {
            self.trim();
        }
    }

    fn trim(&mut self) {
        if self.bytes.len() > self.limit {
            let excess = self.bytes.len() - self.limit;
            self.bytes.drain(..excess);
            self.truncated = true;
        }
    }

    fn into_log(mut self) -> JobLog {
        self.trim();
        let start = if self.truncated {
            self.first_whole_line()
        } else {
            0
        };
        JobLog {
            text: String::from_utf8_lossy(&self.bytes[start..]).into_owned(),
            truncated: self.truncated,
        }
    }

    fn first_whole_line(&self) -> usize {
        match self.bytes.iter().position(|byte| *byte == b'\n') {
            Some(newline) if newline + 1 < self.bytes.len() => newline + 1,
            _ => self
                .bytes
                .iter()
                .take_while(|byte| (**byte & 0b1100_0000) == 0b1000_0000)
                .count(),
        }
    }
}

impl GithubClient {
    pub async fn job_log(&self, repo: &str, job_id: u64) -> Result<JobLog> {
        let repo = repo_path(repo)?;
        let url = self.rest_url_for(&format!("/repos/{repo}/actions/jobs/{job_id}/logs"), &[])?;
        let response = self
            .rest_request(Method::GET, url)
            .timeout(LOG_TIMEOUT)
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        let mut response = self.check_status(response)?;
        let mut tail = TailBuffer::new(LOG_LIMIT);
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(reqwest::Error::without_url)?
        {
            tail.push(&chunk);
        }
        Ok(tail.into_log())
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    use super::*;
    use crate::test_server::read_request;

    const TOKEN: &str = "test-token-not-real";

    fn log_of(chunks: &[&[u8]], limit: usize) -> JobLog {
        let mut tail = TailBuffer::new(limit);
        for chunk in chunks {
            tail.push(chunk);
        }
        tail.into_log()
    }

    fn tail_of(chunks: &[&[u8]], limit: usize) -> String {
        log_of(chunks, limit).text
    }

    #[test]
    fn log_under_the_limit_is_returned_whole() {
        assert_eq!(
            tail_of(&[b"one\ntwo\n", b"three\n"], 100),
            "one\ntwo\nthree\n"
        );
    }

    #[test]
    fn log_over_the_limit_keeps_only_whole_trailing_lines() {
        let text = tail_of(
            &[b"first line\nsecond line\n", b"third line\nfourth line\n"],
            30,
        );
        assert_eq!(
            text,
            "fourth line\n"
                .replace("fourth", "third line\nfourth")
                .as_str()
        );
    }

    #[test]
    fn truncation_across_many_chunks_still_ends_with_the_last_line() {
        let chunk = b"0123456789\n";
        let chunks: Vec<&[u8]> = (0..50).map(|_| chunk.as_slice()).collect();
        let mut tail = TailBuffer::new(25);
        for chunk in &chunks {
            tail.push(chunk);
        }
        assert!(tail.truncated);
        assert_eq!(tail.into_log().text, "0123456789\n0123456789\n");
    }

    #[test]
    fn truncated_flag_reflects_whether_bytes_were_dropped() {
        assert!(
            !log_of(
                &[b"one
two
"],
                100
            )
            .truncated
        );
        assert!(
            log_of(
                &[b"one
two
three
"],
                10
            )
            .truncated
        );
    }

    #[test]
    fn single_line_longer_than_the_limit_keeps_its_last_bytes() {
        let log = log_of(
            &[b"abcdefghijklmnopqrstuvwxyz
"],
            10,
        );
        assert!(log.truncated);
        assert_eq!(
            log.text,
            "rstuvwxyz
"
        );

        let unterminated = log_of(&[b"abcdefghijklmnopqrstuvwxyz"], 10);
        assert_eq!(unterminated.text, "qrstuvwxyz");
    }

    #[test]
    fn cut_inside_a_multibyte_char_drops_the_partial_char() {
        let log = log_of(&["\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}".as_bytes()], 9);
        assert!(log.truncated);
        assert_eq!(log.text, "\u{e9}\u{e9}\u{e9}\u{e9}");
    }

    #[test]
    fn invalid_utf8_is_replaced_instead_of_failing() {
        let text = tail_of(&[b"ok \xff\xfe line\n"], 100);
        assert!(text.starts_with("ok "));
        assert!(text.ends_with(" line\n"));
    }

    async fn respond_once(listener: TcpListener, response: String) -> String {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
        request
    }

    fn has_authorization(request: &str) -> bool {
        request
            .lines()
            .any(|line| line.to_ascii_lowercase().starts_with("authorization:"))
    }

    #[tokio::test]
    async fn authorization_is_not_forwarded_to_the_storage_host_on_redirect() {
        let storage = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let storage_port = storage.local_addr().unwrap().port();
        let api = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_port = api.local_addr().unwrap().port();

        let log = "2026-11-01T20:16:00Z ##[error]Process completed with exit code 1.\n";
        let storage_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{log}",
            log.len()
        );
        let api_response = format!(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{storage_port}/signed/log?sig=abc\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let storage_task = tokio::spawn(respond_once(storage, storage_response));
        let api_task = tokio::spawn(respond_once(api, api_response));

        let mut client = GithubClient::new("github.com", TOKEN.to_string());
        client.rest_url = format!("http://127.0.0.1:{api_port}");
        let text = client
            .job_log("orbita-health/atlas", 7300000002)
            .await
            .unwrap();

        let api_request = api_task.await.unwrap();
        let storage_request = storage_task.await.unwrap();
        assert_eq!(text.text, log);
        assert!(!text.truncated);
        assert!(
            api_request.starts_with("GET /repos/orbita-health/atlas/actions/jobs/7300000002/logs ")
        );
        assert!(has_authorization(&api_request));
        assert!(storage_request.starts_with("GET /signed/log?sig=abc "));
        assert!(
            !has_authorization(&storage_request),
            "Authorization leaked to the redirect target: {storage_request}"
        );
    }

    #[tokio::test]
    async fn storage_failure_does_not_leak_the_signed_url_in_the_error() {
        use crate::test_server::{json_response, serve};

        let storage_port = serve(|_| json_response("403 Forbidden", "{}")).await;
        let api_port = serve(move |_| {
            format!(
                "HTTP/1.1 302 Found
Location: http://127.0.0.1:{storage_port}/signed/log?sig=secret-signature
Content-Length: 0
Connection: close

"
            )
        })
        .await;

        let mut client = GithubClient::new("github.com", TOKEN.to_string());
        client.rest_url = format!("http://127.0.0.1:{api_port}");
        let error = client
            .job_log("orbita-health/atlas", 7300000002)
            .await
            .unwrap_err();

        let message = format!("{error} {error:?}");
        assert!(message.contains("403"), "{message}");
        assert!(!message.contains("secret-signature"), "{message}");
        assert!(!message.contains("/signed/"), "{message}");
    }
}
