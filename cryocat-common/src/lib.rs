
use anyhow::{Ok, Result};
use serde::{Deserialize, Serialize};
use webrtc::{ice_transport::ice_candidate::RTCIceCandidateInit, peer_connection::sdp::session_description::RTCSessionDescription};

pub type Id = String;
pub type Offer = bool;

#[derive(Serialize, Deserialize)]
pub enum Packet {
    Start(Id), RequestOffer, Offer(RTCSessionDescription), Answer(RTCSessionDescription), Candidate(RTCIceCandidateInit),
}

impl Packet {
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json<'a>(json: &'a str) -> Result<Self> {
        Ok(serde_json::from_str(&json)?)
    }
}
