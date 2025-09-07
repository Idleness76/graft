#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Start,
    End,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ChannelType {
    Message,
    Error,
    Extra,
}
