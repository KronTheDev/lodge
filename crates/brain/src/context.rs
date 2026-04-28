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

    pub fn push(&mut self, input: String, response: String) {
        if self.exchanges.len() == Self::MAX_EXCHANGES {
            self.exchanges.pop_front();
        }
        self.exchanges.push_back((input, response));
    }

    /// Renders the rolling context into a prompt prefix for the model.
    pub fn as_prompt_prefix(&self) -> String {
        self.exchanges
            .iter()
            .map(|(i, r)| format!("User: {i}\nAssistant: {r}\n"))
            .collect()
    }
}

impl Default for ConversationContext {
    fn default() -> Self {
        Self::new()
    }
}
