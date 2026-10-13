//! Inference — the servers that run models for Centinel.
//!
//! Centinel runs no model itself. Embedding, reranking and transcription are each a
//! request to a server the operator runs, or one they pay for, and what comes back is
//! stored here. The tuning that makes a model fast on one machine — GPU layers, physical
//! batch, flash attention, threads — belongs to the server, where it is set once for that
//! machine, and never to this binary's flags.
//!
//! ## Three requests, two specs
//!
//! ```text
//!   embed       POST {base_url}/embeddings             OpenAI
//!   transcribe  POST {base_url}/audio/transcriptions   OpenAI, verbose_json segments
//!   rerank      POST {base_url}/rerank                 Cohere/Jina — OpenAI has none
//! ```
//!
//! llama-server serves the first and the third (`--embedding`, `--reranking`),
//! whisper.cpp's server and faster-whisper serve the second, and LocalAI and vLLM serve
//! all three. OpenRouter serves embeddings. The `[inference]` block names one `base_url`
//! for the lot, and each capability may name its own.
//!
//! ## Where text goes is where the operator pointed it
//!
//! SPEC §2.1 still holds for a server on `localhost`: nothing leaves the machine. A
//! `base_url` elsewhere sends chunk text, queries and audio there, and the config file is
//! the one place that says so — nothing falls back to a server the operator did not name.
//!
//! ## A failure about the server ends a stage; one about the input skips it
//!
//! A server that is not listening, refuses the key, does not serve the path, or is still
//! answering 5xx or 429 after three attempts will treat every later request the same way.
//! Those are [`Fatal`], and a stage stops on them rather than skipping a corpus one chunk at
//! a time for hours — the next run resumes where it stopped. A request the server refuses
//! on its merits (any other 4xx) is not retried, and a timeout or a dropped connection is
//! retried and then handed back: both may be about *this* input, so the caller splits the
//! batch to find the chunk. A response that fails verification goes straight back the same
//! way — it is the caller's to check, not this module's.

use std::time::Duration;

use crate::config::InferenceConfig;

/// Attempts per request. The first is the request; the rest cover a 429 or a wobble on
/// the wire, the pause doubling each time.
const ATTEMPTS: u32 = 3;

/// The first retry pause. Doubles per attempt.
const BACKOFF: Duration = Duration::from_secs(1);

/// How long a connection may take to open. A server on this machine answers in
/// microseconds and one across a network in milliseconds; past this it is not there.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// What a request is for. Each names one model and, optionally, its own server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    Embed,
    Rerank,
    Transcribe,
}

impl Capability {
    pub const ALL: [Self; 3] = [Self::Embed, Self::Rerank, Self::Transcribe];

    /// The config table and the word a report uses.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Embed => "embed",
            Self::Rerank => "rerank",
            Self::Transcribe => "transcribe",
        }
    }

    /// The path under `base_url` this capability posts to.
    fn path(&self) -> &'static str {
        match self {
            Self::Embed => "embeddings",
            Self::Rerank => "rerank",
            Self::Transcribe => "audio/transcriptions",
        }
    }
}

/// A model on a server: where to send a request, and what to ask for.
///
/// Built from the config without reading the key, so `doctor`, a dry run and a reachability
/// check never fail for want of a secret they would not use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub capability: Capability,
    /// Without a trailing slash: `http://127.0.0.1:8080/v1`.
    pub base_url: String,
    /// The model the server is asked for, verbatim.
    pub model: String,
    /// The environment variable holding the API key, if the server wants one.
    pub api_key_env: Option<String>,
}

impl Endpoint {
    pub fn new(
        capability: Capability,
        base_url: &str,
        model: &str,
        api_key_env: Option<&str>,
    ) -> Self {
        Self {
            capability,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key_env: api_key_env.map(str::to_string),
        }
    }

    /// Where this capability's requests go.
    pub fn url(&self) -> String {
        format!("{}/{}", self.base_url, self.capability.path())
    }

    /// The same model, under another name — a `--model` typed for one run.
    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    /// `host:port`, for provenance and for messages. The path is left off: it names the
    /// API, not the machine.
    pub fn host(&self) -> String {
        match url::Url::parse(&self.base_url) {
            Ok(url) => match (url.host_str(), url.port_or_known_default()) {
                (Some(host), Some(port)) => format!("{host}:{port}"),
                (Some(host), None) => host.to_string(),
                _ => self.base_url.clone(),
            },
            Err(_) => self.base_url.clone(),
        }
    }

    /// The API key, or `None` when the server wants none. A variable that is named and
    /// empty is an error naming it, the way a missing binary names its install.
    pub fn key(&self) -> anyhow::Result<Option<String>> {
        let Some(var) = &self.api_key_env else {
            return Ok(None);
        };
        let key = std::env::var(var).unwrap_or_default().trim().to_string();
        anyhow::ensure!(
            !key.is_empty(),
            "[inference] names ${var} for {}, and it is not set",
            self.base_url
        );
        Ok(Some(key))
    }

    /// The model as the spend ledger files it.
    ///
    /// OpenRouter's models keep the `openrouter/` prefix the ledger has always used, so a
    /// month of history stays one series and [`crate::spend::PRICES`] prices it. Any
    /// other server's model is filed under its own name.
    pub fn ledger_model(&self) -> String {
        if self.host().starts_with("openrouter.ai") {
            format!("openrouter/{}", self.model)
        } else {
            self.model.clone()
        }
    }

    /// Whether anything is listening, by opening a TCP connection and closing it.
    ///
    /// A connection and nothing more, because the three specs share no cheap read:
    /// whisper.cpp's server has no `/models`, and a real request costs a model call.
    /// What it proves is that a stage will reach a server — the question `run` asks
    /// before an embed stage and `doctor` asks of every capability.
    pub async fn reachable(&self) -> Result<(), String> {
        let url = url::Url::parse(&self.base_url)
            .map_err(|e| format!("`{}` is not a URL: {e}", self.base_url))?;
        let host = url
            .host_str()
            .ok_or_else(|| format!("`{}` names no host", self.base_url))?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let port = url
            .port_or_known_default()
            .ok_or_else(|| format!("`{}` names no port", self.base_url))?;
        match tokio::time::timeout(
            CONNECT_TIMEOUT,
            tokio::net::TcpStream::connect((host.as_str(), port)),
        )
        .await
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(format!(
                "nothing is answering at {} ({e}) — start a server for {} there, or point \
                 [inference] at one",
                self.base_url, self.model
            )),
            Err(_) => Err(format!(
                "{} did not answer within {}s",
                self.base_url,
                CONNECT_TIMEOUT.as_secs()
            )),
        }
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}", self.model, self.base_url)
    }
}

impl InferenceConfig {
    /// The endpoint for one capability: its own server if its table names one, else the
    /// shared `base_url`, and the same for the key.
    pub fn endpoint(&self, capability: Capability) -> Endpoint {
        let (model, base_url, api_key_env) = match capability {
            Capability::Embed => (
                &self.embed.model,
                &self.embed.base_url,
                &self.embed.api_key_env,
            ),
            Capability::Rerank => (
                &self.rerank.model,
                &self.rerank.base_url,
                &self.rerank.api_key_env,
            ),
            Capability::Transcribe => (
                &self.transcribe.model,
                &self.transcribe.base_url,
                &self.transcribe.api_key_env,
            ),
        };
        Endpoint::new(
            capability,
            base_url.as_deref().unwrap_or(&self.base_url),
            model,
            api_key_env.as_deref().or(self.api_key_env.as_deref()),
        )
    }

    /// Every environment variable the config names for a key, once each — what a
    /// background service has to be handed, since it inherits no shell.
    pub fn key_vars(&self) -> Vec<String> {
        let mut vars: Vec<String> = Capability::ALL
            .into_iter()
            .filter_map(|capability| self.endpoint(capability).api_key_env)
            .collect();
        vars.sort();
        vars.dedup();
        vars
    }
}

/// A failure every later request would repeat: nothing listening, or the key refused.
///
/// The one failure that must stop a run rather than skip a chunk — see the module docs.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Fatal(pub(crate) String);

/// Whether an error anywhere in `err`'s chain is [`Fatal`].
pub fn is_fatal(err: &anyhow::Error) -> bool {
    err.chain()
        .any(|cause| cause.downcast_ref::<Fatal>().is_some())
}

/// A connection to one endpoint: the key read once, the client built once.
///
/// Cheap to build — the expensive part is on the far end — and the one place an HTTP
/// request to a model server is made, so retries, the key and the fatal rule are written
/// once for all three capabilities.
pub struct Client {
    pub endpoint: Endpoint,
    http: reqwest::Client,
    key: Option<String>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The key stays out of Debug the way it stays out of the config file.
        f.debug_struct("Client")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Reads the key and builds the client. `timeout` bounds one request end to end.
    pub fn new(endpoint: Endpoint, timeout: Duration) -> anyhow::Result<Self> {
        let key = endpoint.key()?;
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(timeout)
            .user_agent(concat!("centinel/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| anyhow::anyhow!("building the HTTP client: {e}"))?;
        Ok(Self {
            endpoint,
            http,
            key,
        })
    }

    /// Posts a JSON body, retried through transient failures, and returns the response body.
    pub async fn post_json(&self, body: &serde_json::Value) -> anyhow::Result<Vec<u8>> {
        let body = serde_json::to_vec(body)?;
        self.retried(|| {
            let request = self
                .http
                .post(self.endpoint.url())
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.clone());
            async move { Ok::<_, anyhow::Error>(request) }
        })
        .await
    }

    /// Posts a multipart form built fresh for each attempt — a form's file part is a stream
    /// and cannot be sent twice.
    pub async fn post_form<F, Fut>(&self, form: F) -> anyhow::Result<Vec<u8>>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<reqwest::multipart::Form>>,
    {
        self.retried(|| {
            let form = form();
            async move {
                let form = form.await?;
                Ok::<_, anyhow::Error>(self.http.post(self.endpoint.url()).multipart(form))
            }
        })
        .await
    }

    /// One request, attempted up to [`ATTEMPTS`] times. `request` builds it afresh each time.
    async fn retried<F, Fut>(&self, request: F) -> anyhow::Result<Vec<u8>>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<reqwest::RequestBuilder>>,
    {
        let mut last = None;
        for attempt in 0..ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(BACKOFF * 2u32.pow(attempt - 1)).await;
            }
            match self.send(request().await?).await {
                Ok(bytes) => return Ok(bytes),
                Err(failure @ (Attempt::Fatal(_) | Attempt::Refused(_))) => {
                    return Err(failure.into_error());
                }
                Err(failure) => {
                    tracing::debug!(attempt, error = %failure.message(), "inference request failed");
                    last = Some(failure);
                }
            }
        }
        Err(match last.expect("ATTEMPTS is nonzero") {
            // Three answers of "not now" in seven seconds is a server that is down or
            // overloaded, and every later request would get the same.
            Attempt::Unavailable(line) => Fatal(format!(
                "{line} — still failing after {ATTEMPTS} attempts; stopping here, and the next \
                 run resumes"
            ))
            .into(),
            other => other.into_error().context(format!(
                "{} did not take the request after {ATTEMPTS} attempts",
                self.endpoint.base_url
            )),
        })
    }

    /// One HTTP exchange: the raw response body, or which kind of no.
    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Vec<u8>, Attempt> {
        let request = match &self.key {
            Some(key) => request.bearer_auth(key),
            None => request,
        };
        let response = request.send().await.map_err(|e| {
            let line = format!("reaching {}: {e}", self.endpoint.url());
            if e.is_connect() {
                Attempt::Fatal(format!(
                    "{line} — is the {} server running?",
                    self.endpoint.capability.name()
                ))
            } else {
                Attempt::Transient(line)
            }
        })?;

        let status = response.status();
        let bytes = response.bytes().await.map_err(|e| {
            Attempt::Transient(format!("reading {}'s response: {e}", self.endpoint.host()))
        })?;
        if status.is_success() {
            return Ok(bytes.to_vec());
        }
        // The body is the server's own account of what was wrong, so it rides along —
        // truncated, because an HTML error page would otherwise be the whole message.
        let detail: String = String::from_utf8_lossy(&bytes).chars().take(300).collect();
        let line = format!("{} answered {status}: {detail}", self.endpoint.url());
        Err(match status {
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                let hint = match &self.endpoint.api_key_env {
                    Some(var) => format!("check ${var}"),
                    None => "set api_key_env in [inference]".to_string(),
                };
                Attempt::Fatal(format!("{line} — {hint}"))
            }
            // The server does not serve this path: whisper.cpp's server answers at
            // `/inference` unless told otherwise, and llama-server answers `/rerank` only
            // with `--reranking`.
            reqwest::StatusCode::NOT_FOUND => Attempt::Fatal(format!(
                "{line} — the server at {} does not serve `/{}`",
                self.endpoint.base_url,
                self.endpoint.capability.path()
            )),
            reqwest::StatusCode::TOO_MANY_REQUESTS => Attempt::Unavailable(line),
            status if status.is_server_error() => Attempt::Unavailable(line),
            // Any other 4xx is the server refusing *this* request — an input too long, a
            // body it cannot parse. Sent again it is refused again.
            _ => Attempt::Refused(line),
        })
    }
}

/// How one attempt failed, as far as trying again is concerned. Private: what leaves this
/// module is an `anyhow::Error`, [`Fatal`] or not.
enum Attempt {
    /// Every later request would fail the same way. Not retried.
    Fatal(String),
    /// The server is down or overloaded: retried, and [`Fatal`] if it stays that way.
    Unavailable(String),
    /// A timeout or a dropped connection — the server, or this input. Retried, then handed
    /// back for the caller to split.
    Transient(String),
    /// The server refused this request on its merits. Not retried; the next may succeed.
    Refused(String),
}

impl Attempt {
    fn message(&self) -> &str {
        match self {
            Self::Fatal(m) | Self::Unavailable(m) | Self::Transient(m) | Self::Refused(m) => m,
        }
    }

    fn into_error(self) -> anyhow::Error {
        match self {
            Self::Fatal(m) => Fatal(m).into(),
            other => anyhow::anyhow!(other.message().to_string()),
        }
    }
}

/// A test server speaking the three specs, in the raw-socket style of
/// `workspace::fake_jev`.
#[cfg(test)]
pub(crate) mod fake;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capability_with_no_table_of_its_own_uses_the_shared_server() {
        let config = crate::config::Config::parse(
            "[inference]\nbase_url = \"http://gpu:8080/v1/\"\napi_key_env = \"K\"\n\
             [inference.transcribe]\nmodel = \"whisper-1\"\nbase_url = \"http://ears:9000/v1\"\n",
        )
        .unwrap()
        .inference;

        let embed = config.endpoint(Capability::Embed);
        assert_eq!(
            embed.base_url, "http://gpu:8080/v1",
            "trailing slash dropped"
        );
        assert_eq!(embed.url(), "http://gpu:8080/v1/embeddings");
        assert_eq!(embed.api_key_env.as_deref(), Some("K"));

        let transcribe = config.endpoint(Capability::Transcribe);
        assert_eq!(transcribe.url(), "http://ears:9000/v1/audio/transcriptions");
        assert_eq!(transcribe.model, "whisper-1");
        assert_eq!(
            transcribe.api_key_env.as_deref(),
            Some("K"),
            "the shared key applies unless the table names its own"
        );
    }

    #[test]
    fn the_ledger_keeps_openrouter_models_under_their_old_names() {
        let routed = Endpoint::new(
            Capability::Embed,
            "https://openrouter.ai/api/v1",
            "qwen/qwen3-embedding-8b",
            None,
        );
        assert_eq!(routed.ledger_model(), "openrouter/qwen/qwen3-embedding-8b");
        assert_eq!(routed.host(), "openrouter.ai:443");

        let local = Endpoint::new(
            Capability::Embed,
            "http://127.0.0.1:8080/v1",
            "qwen3-embedding-4b",
            None,
        );
        assert_eq!(local.ledger_model(), "qwen3-embedding-4b");
    }

    #[test]
    fn a_named_key_that_is_not_set_names_the_variable() {
        let endpoint = Endpoint::new(
            Capability::Embed,
            "http://127.0.0.1:1/v1",
            "m",
            Some("CENTINEL_TEST_KEY_THAT_IS_NEVER_SET"),
        );
        let err = endpoint.key().unwrap_err().to_string();
        assert!(
            err.contains("$CENTINEL_TEST_KEY_THAT_IS_NEVER_SET"),
            "{err}"
        );
    }

    /// The fatal rule, end to end: a port nothing listens on is refused once and ends the
    /// stage, rather than costing three backoffs per chunk.
    #[tokio::test]
    async fn nothing_listening_is_fatal_and_named() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let endpoint = Endpoint::new(
            Capability::Embed,
            &format!("http://127.0.0.1:{port}/v1"),
            "m",
            None,
        );
        assert!(endpoint.reachable().await.is_err());

        let client = Client::new(endpoint, Duration::from_secs(5)).unwrap();
        let started = std::time::Instant::now();
        let err = client.post_json(&serde_json::json!({})).await.unwrap_err();
        assert!(is_fatal(&err), "{err:#}");
        assert!(
            started.elapsed() < BACKOFF,
            "a fatal failure is not retried"
        );
        assert!(format!("{err:#}").contains("embed server"), "{err:#}");
    }

    #[test]
    fn only_a_fatal_error_is_fatal() {
        let fatal: anyhow::Error = Fatal("401".into()).into();
        assert!(is_fatal(&fatal));
        assert!(is_fatal(&fatal.context("wrapped")));
        assert!(!is_fatal(&anyhow::anyhow!("answered 429")));
    }
}
