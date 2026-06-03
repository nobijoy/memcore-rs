mod pii;

pub use pii::PiiRedactor;

use crate::ports::MemoryMessage;

/// Redacts message bodies when PII redaction is enabled for LLM extraction.
pub fn redact_messages_for_extraction(messages: &[MemoryMessage]) -> Vec<MemoryMessage> {
    messages
        .iter()
        .map(|message| MemoryMessage {
            role: message.role.clone(),
            content: PiiRedactor::redact_text(&message.content),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::redact_messages_for_extraction;
    use crate::ports::{MemoryMessage, MessageRole};

    #[test]
    fn redact_messages_applies_to_user_content() {
        let messages = vec![MemoryMessage {
            role: MessageRole::User,
            content: "Reach me at user@example.com".to_string(),
        }];
        let redacted = redact_messages_for_extraction(&messages);
        assert!(redacted[0].content.contains("[REDACTED_EMAIL]"));
    }
}
