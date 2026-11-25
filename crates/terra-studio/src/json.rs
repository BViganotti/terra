//! JSON reading and writing with hard limits.
//!
//! The Studio accepts small configuration documents and emits larger result
//! documents. The writer serializes any [`Json`] value; the parser enforces a
//! maximum input size, nesting depth, container length and string length so
//! malformed requests fail with an error instead of exhausting memory.

/// Implementation is introduced with the Studio milestone.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// JSON null.
    Null,
    /// Boolean.
    Bool(bool),
    /// Number, stored as the original decimal representation.
    Number(f64),
    /// String.
    String(String),
    /// Array.
    Array(Vec<Json>),
    /// Object with insertion order preserved.
    Object(Vec<(String, Json)>),
}
