/// Rolling conversation state — last 4 exchanges.
///
/// In-memory only. Nothing persists between sessions.
pub struct ConversationContext {
    exchanges: std::collections::VecDeque<(String, String)>,
}

impl ConversationContext {
    const MAX_EXCHANGES: usize = 4;

    pub fn new() -> Self {
        Self {
            exchanges: std::collections::VecDeque::with_capacity(Self::MAX_EXCHANGES),
        }
    }

    /// Add an exchange. Drops the oldest if the window is full.
    pub fn push(&mut self, input: String, response: String) {
        if self.exchanges.len() == Self::MAX_EXCHANGES {
            self.exchanges.pop_front();
        }
        self.exchanges.push_back((input, response));
    }

    /// Number of exchanges currently stored.
    pub fn len(&self) -> usize {
        self.exchanges.len()
    }

    /// True if no exchanges have been stored.
    pub fn is_empty(&self) -> bool {
        self.exchanges.is_empty()
    }

    /// Renders the rolling context into a ChatML-formatted prompt prefix.
    ///
    /// Suitable for prepending to the next model prompt.
    pub fn as_prompt_prefix(&self) -> String {
        self.exchanges
            .iter()
            .map(|(i, r)| {
                format!(
                    "<|im_start|>user\n{i}\n<|im_end|>\n<|im_start|>assistant\n{r}\n<|im_end|>\n"
                )
            })
            .collect()
    }
}

impl Default for ConversationContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_empty() {
        let ctx = ConversationContext::new();
        assert_eq!(ctx.len(), 0);
        assert!(ctx.is_empty());
    }

    #[test]
    fn push_and_len() {
        let mut ctx = ConversationContext::new();
        ctx.push("hello".into(), "hi".into());
        assert_eq!(ctx.len(), 1);
        assert!(!ctx.is_empty());
    }

    #[test]
    fn window_caps_at_four() {
        let mut ctx = ConversationContext::new();
        for i in 0..6 {
            ctx.push(format!("q{i}"), format!("a{i}"));
        }
        assert_eq!(ctx.len(), 4);
    }

    #[test]
    fn oldest_dropped_when_full() {
        let mut ctx = ConversationContext::new();
        for i in 0..5 {
            ctx.push(format!("q{i}"), format!("a{i}"));
        }
        // q0 should be gone, q1-q4 present
        let prefix = ctx.as_prompt_prefix();
        assert!(!prefix.contains("q0"));
        assert!(prefix.contains("q1"));
        assert!(prefix.contains("q4"));
    }

    #[test]
    fn prompt_prefix_uses_chatml() {
        let mut ctx = ConversationContext::new();
        ctx.push("test input".into(), "test response".into());
        let prefix = ctx.as_prompt_prefix();
        assert!(prefix.contains("<|im_start|>user"));
        assert!(prefix.contains("test input"));
        assert!(prefix.contains("<|im_start|>assistant"));
        assert!(prefix.contains("test response"));
    }
}
