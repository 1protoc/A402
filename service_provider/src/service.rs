//! Synthetic business logic the SP "executes" before producing `σ̂_S` and
//! encrypting the result. Kept deliberately trivial: a swappable
//! `weather()` function. Real deployments would proxy to an upstream API
//! here — the atomicity guarantee doesn't care what's inside this function
//! as long as the plaintext bytes are deterministic given the request.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct WeatherResult {
    pub endpoint: &'static str,
    pub location: &'static str,
    pub temperature: u32,
    pub conditions: &'static str,
    pub request_number: u64,
}

pub fn execute_weather(request_number: u64) -> Vec<u8> {
    let result = WeatherResult {
        endpoint: "/weather",
        location: "San Francisco",
        temperature: 72 + (request_number % 3) as u32,
        conditions: "clear",
        request_number,
    };
    serde_json::to_vec(&result).expect("serialize weather result")
}
