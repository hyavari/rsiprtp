//! LLM client for Ollama.
//!
//! Provides chat API integration with Ollama for conversational AI.

use crate::config::LlmConfig;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Ollama chat client.
pub struct OllamaClient {
    client: Client,
    endpoint: String,
    model: String,
    system_prompt: String,
    temperature: f32,
    max_tokens: u32,
    history: Vec<Message>,
    max_history: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    options: ChatOptions,
}

#[derive(Debug, Serialize)]
struct ChatOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Debug, Deserialize)]
struct NonStreamResponse {
    message: MessageContent,
}

#[derive(Debug, Deserialize)]
struct MessageContent {
    content: String,
}

impl OllamaClient {
    /// Create a new Ollama client from configuration.
    pub fn new(config: &LlmConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            endpoint: config.endpoint.clone(),
            model: config.model.clone(),
            system_prompt: config.system_prompt.clone(),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
            history: Vec::new(),
            max_history: config.max_history_messages,
        }
    }

    /// Send a chat message and get the complete response.
    ///
    /// This is the main method to use - collects the full response.
    pub async fn chat_complete(&mut self, user_input: &str) -> String {
        // Add user message to history
        self.history.push(Message {
            role: "user".to_string(),
            content: user_input.to_string(),
        });

        // Trim history if needed
        self.trim_history();

        // Build messages with system prompt
        let mut messages = vec![Message {
            role: "system".to_string(),
            content: self.system_prompt.clone(),
        }];
        messages.extend(self.history.clone());

        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            stream: false, // Non-streaming for simplicity
            options: ChatOptions {
                temperature: self.temperature,
                num_predict: self.max_tokens,
            },
        };

        let response = match self
            .client
            .post(format!("{}/api/chat", self.endpoint))
            .json(&request)
            .send()
            .await
        {
            Ok(resp) => {
                if !resp.status().is_success() {
                    tracing::error!("Ollama request failed: {}", resp.status());
                    return "I'm having trouble thinking right now.".to_string();
                }
                match resp.json::<NonStreamResponse>().await {
                    Ok(data) => data.message.content,
                    Err(e) => {
                        tracing::error!("Failed to parse response: {}", e);
                        "I'm having trouble understanding the response.".to_string()
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to send request to Ollama: {}", e);
                "I'm having trouble connecting right now.".to_string()
            }
        };

        // Add assistant response to history
        if !response.is_empty() {
            self.history.push(Message {
                role: "assistant".to_string(),
                content: response.clone(),
            });
            self.trim_history();
        }

        response
    }

    /// Trim conversation history to stay within limits.
    fn trim_history(&mut self) {
        if self.history.len() > self.max_history {
            // Remove oldest messages, keeping pairs (user + assistant)
            let to_remove = self.history.len() - self.max_history;
            let to_remove = to_remove.div_ceil(2) * 2; // Round up to even number
            let to_remove = to_remove.min(self.history.len());
            self.history.drain(0..to_remove);
        }
    }

    /// Add an assistant response to history (for streaming case).
    pub fn add_assistant_response(&mut self, response: &str) {
        self.history.push(Message {
            role: "assistant".to_string(),
            content: response.to_string(),
        });
        self.trim_history();
    }

    /// Clear conversation history.
    #[allow(dead_code)] // API for future use
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Check if Ollama is reachable.
    #[allow(dead_code)] // API for startup health check
    pub async fn health_check(&self) -> bool {
        match self
            .client
            .get(format!("{}/api/tags", self.endpoint))
            .send()
            .await
        {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim_history() {
        let config = LlmConfig {
            max_history_messages: 4,
            ..Default::default()
        };
        let mut client = OllamaClient::new(&config);

        // Add 6 messages
        for i in 0..6 {
            client.history.push(Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
                content: format!("Message {}", i),
            });
        }

        client.trim_history();

        // Should have trimmed to 4 messages
        assert!(client.history.len() <= 4);
    }
}
