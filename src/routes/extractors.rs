//! Drop-in replacements for axum's `Json` and `Path` extractors whose
//! rejections are converted into `AppError`, so malformed bodies, missing
//! `Content-Type` and non-numeric path ids also produce JSON error responses.

use crate::error::AppError;
use axum::extract::{FromRequest, FromRequestParts};

#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(AppError))]
pub struct Json<T>(pub T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(AppError))]
pub struct Path<T>(pub T);
