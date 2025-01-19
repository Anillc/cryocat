
use anyhow::{Ok, Result};
use serde::{Deserialize, Serialize};

pub type Id = String;
pub type Offer = bool;

#[derive(Serialize, Deserialize)]
pub enum Packet {
    Start(Id), Description(Offer, String), Candidate(String),
}

impl Packet {
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json<'a>(json: &'a str) -> Result<Self> {
        Ok(serde_json::from_str(&json)?)
    }
}
