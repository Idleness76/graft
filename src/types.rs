#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Start,
    End,
    Other(String),
}

impl NodeKind {
    /// Encode a NodeKind into its persisted string form.
    /// Format:
    ///   Start => "Start"
    ///   End   => "End"
    ///   Other("X") => "Other:X"
    pub fn encode(&self) -> String {
        match self {
            NodeKind::Start => "Start".to_string(),
            NodeKind::End => "End".to_string(),
            NodeKind::Other(s) => format!("Other:{s}"),
        }
    }

    /// Decode a persisted string form back into a NodeKind.
    /// Falls back to `Other(s)` if the format is unrecognized (forward-compatible).
    pub fn decode(s: &str) -> Self {
        if s == "Start" {
            NodeKind::Start
        } else if s == "End" {
            NodeKind::End
        } else if let Some(rest) = s.strip_prefix("Other:") {
            NodeKind::Other(rest.to_string())
        } else {
            NodeKind::Other(s.to_string())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ChannelType {
    Message,
    Error,
    Extra,
}
