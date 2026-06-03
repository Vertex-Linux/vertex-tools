pub mod os;
pub mod packages;
pub mod tools;

#[derive(Debug, Clone)]
pub struct OsInfo {
    pub sha: String,
    pub short: String,
    pub message: String,
    pub date: String,
}

pub enum Msg {
    Log(String),
    Done(bool),
    OsInfo(OsInfo),
    OsError(String),
    ScriptReady { path: String, sha: String },
    ScriptError(String),
    SelfUpdated,
}
