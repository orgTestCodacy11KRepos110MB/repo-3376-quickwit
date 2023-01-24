// Copyright (C) 2022 Quickwit, Inc.
//
// Quickwit is offered under the AGPL v3.0 and as commercial software.
// For commercial licensing, contact us at hello@quickwit.io.
//
// AGPL:
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <http://www.gnu.org/licenses/>.

use reqwest::StatusCode;
use serde::Deserialize;
use thiserror::Error;

pub static DEFAULT_ADDRESS: &str = "http://127.0.0.1:7280";
pub static DEFAULT_CONTENT_TYPE: &str = "application/json";

#[derive(Error, Debug)]
pub enum Error {
    // Error returned by reqwest lib.
    #[error("Reqwest client lib error: {0}")]
    Client(#[from] reqwest::Error),
    // Internal error returned by quickwit client lib.
    #[error("Internal Quickwit client error: {0}")]
    Internal(String),
    // IO Error returned by tokio lib.
    #[error("IO error: {0}")]
    Io(#[from] tokio::io::Error),
    // Error returned by url lib put in a string.
    #[error("Url parsing error: {0}")]
    UrlParse(String),
    // Json serialization/deserialization error.
    #[error("Serde JSON error: {0}")]
    Json(#[from] serde_json::error::Error),
    // Internal error returned by quickwit client lib.
    #[error("HTTP error status: {0}")]
    Http(String), // <- TODO: merge this error with ApiError.
    // Error returns by Quickwit.
    #[error("Api error: {0}")]
    Api(#[from] ApiError),
}

impl Error {
    pub fn status_code(&self) -> Option<StatusCode> {
        match &self {
            Error::Client(err) => err.status(),
            _ => None,
        }
    }
}

#[derive(Error, Debug, Deserialize)]
pub struct ApiError {
    pub error: String,
}

// Implement `Display` for `ApiError`.
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "(error={})", self.error)
    }
}
