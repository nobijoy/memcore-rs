use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    Profile,
    Preference,
    Project,
    Conversation,
    Task,
    Entity,
    Skill,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    UserMessage,
    AssistantMessage,
    ApiImport,
    Manual,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactOperation {
    Add,
    Update,
    Delete,
    NoOp,
    Archive,
    Summarize,
}
