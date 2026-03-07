use log::{error, info, warn};

pub async fn summarize_diff(diff: &str, model: &str) -> Option<String> {
    use genai::Client;
    use genai::chat::{ChatMessage, ChatRequest};

    const MAX_DIFF_CHARS: usize = 8000;
    let diff = if diff.len() > MAX_DIFF_CHARS {
        let boundary = (0..=MAX_DIFF_CHARS).rev().find(|&i| diff.is_char_boundary(i)).unwrap_or(0);
        warn!("diff truncated to {boundary} bytes for LLM summarization");
        &diff[..boundary]
    } else {
        diff
    };

    let prompt = format!(
        "Summarize the following git diff in one concise sentence, \
         focusing on what changed and why it matters. \
         Reply with only the sentence, no preamble.\n\n{diff}"
    );

    let req = ChatRequest::new(vec![ChatMessage::user(prompt)]);
    let client = Client::default();
    info!("requesting diff summary from model '{model}'");
    match client.exec_chat(model, req, None).await {
        Ok(response) => {
            let summary = response.content_text_into_string();
            if summary.is_none() {
                warn!("LLM returned an empty response");
            }
            summary
        }
        Err(e) => {
            error!("LLM call failed: {e}");
            None
        }
    }
}

pub async fn llm_recap(content: &str, period: &str, model: &str) -> Option<String> {
    use genai::Client;
    use genai::chat::{ChatMessage, ChatRequest};

    const MAX_CHARS: usize = 40_000;
    let content = if content.len() > MAX_CHARS {
        let boundary = (0..=MAX_CHARS).rev().find(|&i| content.is_char_boundary(i)).unwrap_or(0);
        warn!("recap content truncated to {boundary} bytes");
        &content[..boundary]
    } else {
        content
    };

    let prompt = format!(
        "The following is a log of all git commits I made this {period}, \
         including their descriptions. Write a concise but thorough recap of \
         what I worked on: key themes, notable achievements, and any recurring \
         projects or areas of focus. Use markdown with bullet points.\n\n{content}"
    );

    let req = ChatRequest::new(vec![ChatMessage::user(prompt)]);
    match Client::default().exec_chat(model, req, None).await {
        Ok(r) => {
            let text = r.content_text_into_string();
            if text.is_none() {
                warn!("LLM returned empty recap");
            }
            text
        }
        Err(e) => {
            error!("LLM recap call failed: {e}");
            None
        }
    }
}
