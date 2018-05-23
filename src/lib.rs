extern crate chrono;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate slog;

mod drain;
mod util;

pub use drain::MozLogJson;
