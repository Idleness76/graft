#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Start,
    End,
    Other(String),
}
