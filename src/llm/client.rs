use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Request payload for an LLM prompt.
#[derive(Clone, Debug)]
pub struct LlmRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub max_tokens: u32,
    pub temperature: f32,
    /// Optional base64-encoded images for multimodal prompts.
    pub images: Vec<String>,
}

/// Response from an LLM provider.
#[derive(Clone, Debug)]
pub struct LlmResponse {
    pub text: String,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Errors that can occur during LLM requests.
#[derive(Debug, Clone)]
pub enum LlmError {
    Network(String),
    Parse(String),
    Api(String),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::Network(msg) => write!(f, "Network error: {msg}"),
            LlmError::Parse(msg) => write!(f, "Parse error: {msg}"),
            LlmError::Api(msg) => write!(f, "API error: {msg}"),
        }
    }
}

impl std::error::Error for LlmError {}

impl From<reqwest::Error> for LlmError {
    fn from(err: reqwest::Error) -> Self {
        LlmError::Network(err.to_string())
    }
}

/// Trait for LLM clients.
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn prompt(&self, request: LlmRequest) -> Result<LlmResponse, LlmError>;
}

// ─── Ollama Client ───

pub struct OllamaClient {
    client: Client,
    api_url: String,
    model: String,
}

impl OllamaClient {
    pub fn new(api_url: String, model: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client with default settings should always build");
        Self {
            client,
            api_url,
            model,
        }
    }
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn prompt(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        #[derive(Serialize)]
        struct Request {
            model: String,
            prompt: String,
            system: String,
            images: Vec<String>,
            stream: bool,
            options: Options,
        }

        #[derive(Serialize)]
        struct Options {
            temperature: f32,
            num_predict: i32,
        }

        #[derive(Deserialize)]
        struct Response {
            response: String,
            #[allow(dead_code)]
            done: bool,
        }

        let req = Request {
            model: self.model.clone(),
            prompt: request.user_prompt,
            system: request.system_prompt,
            images: request.images,
            stream: false,
            options: Options {
                temperature: request.temperature,
                num_predict: request.max_tokens as i32,
            },
        };

        let res = self
            .client
            .post(&self.api_url)
            .json(&req)
            .send()
            .await?;

        let status = res.status();
        if !status.is_success() {
            let text = res.text().await.unwrap_or_default();
            return Err(LlmError::Api(format!("Ollama API error: {status} - {text}")));
        }

        let data: Response = res.json().await.map_err(|e| LlmError::Parse(e.to_string()))?;

        Ok(LlmResponse {
            text: data.response,
            reasoning: None,
            tool_calls: Vec::new(),
        })
    }
}

// ─── OpenAI-Compatible Client ───

pub struct OpenAiClient {
    client: Client,
    api_url: String,
    api_key: Option<String>,
    model: String,
}

impl OpenAiClient {
    pub fn new(api_url: String, api_key: Option<String>, model: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client with default settings should always build");
        Self {
            client,
            api_url,
            api_key,
            model,
        }
    }
}

#[async_trait]
impl LlmClient for OpenAiClient {
    async fn prompt(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        #[derive(Serialize)]
        struct ApiRequest {
            model: String,
            messages: Vec<Message>,
            max_tokens: u32,
            temperature: f32,
        }

        #[derive(Serialize)]
        struct Message {
            role: String,
            content: Vec<Content>,
        }

        #[derive(Serialize)]
        #[serde(tag = "type")]
        enum Content {
            #[serde(rename = "text")]
            Text { text: String },
            #[serde(rename = "image_url")]
            Image { image_url: ImageUrl },
        }

        #[derive(Serialize)]
        struct ImageUrl {
            url: String,
        }

        let mut user_content = Vec::new();
        user_content.push(Content::Text {
            text: request.user_prompt,
        });
        for img in &request.images {
            user_content.push(Content::Image {
                image_url: ImageUrl {
                    url: format!("data:image/png;base64,{img}"),
                },
            });
        }

        let messages = vec![
            Message {
                role: "system".into(),
                content: vec![Content::Text {
                    text: request.system_prompt,
                }],
            },
            Message {
                role: "user".into(),
                content: user_content,
            },
        ];

        let req = ApiRequest {
            model: self.model.clone(),
            messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
        };

        let mut builder = self.client.post(&self.api_url);
        if let Some(ref key) = self.api_key {
            builder = builder.header("Authorization", format!("Bearer {key}"));
        }

        let res = builder.json(&req).send().await?;
        let status = res.status();
        if !status.is_success() {
            let text = res.text().await.unwrap_or_default();
            return Err(LlmError::Api(format!("OpenAI API error: {status} - {text}")));
        }

        let json: serde_json::Value = res.json().await.map_err(|e| LlmError::Parse(e.to_string()))?;
        let text = json["choices"]
            .get(0)
            .and_then(|c| c["message"]["content"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(LlmResponse {
            text,
            reasoning: None,
            tool_calls: Vec::new(),
        })
    }
}

// ─── Demo Client ───

pub struct DemoClient;

impl DemoClient {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LlmClient for DemoClient {
    async fn prompt(&self, _request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let actions = ["hover", "move_forward", "ascend", "rotate_cw", "move_left"];
        let reasonings = [
            "Maintaining stable position for observation",
            "Moving toward target waypoint",
            "Gaining altitude for better visibility",
            "Adjusting heading to align with flight path",
            "Avoiding potential obstacle on right side",
        ];
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let idx = (seed % actions.len() as u64) as usize;
        let json = serde_json::json!({
            "action": actions[idx],
            "reasoning": reasonings[idx],
            "confidence": 0.7 + (seed % 3) as f32 / 10.0,
        })
        .to_string();

        Ok(LlmResponse {
            text: json,
            reasoning: None,
            tool_calls: Vec::new(),
        })
    }
}

/// Factory to create the appropriate client for a provider.
pub fn create_client(
    provider: &str,
    model: String,
    api_url: String,
    api_key: Option<String>,
) -> std::sync::Arc<dyn LlmClient> {
    match provider {
        "demo" => std::sync::Arc::new(DemoClient::new()),
        "ollama" => std::sync::Arc::new(OllamaClient::new(api_url, model)),
        _ => std::sync::Arc::new(OpenAiClient::new(api_url, api_key, model)),
    }
}
