// {{{ Crate docs
//! MozLogJSON `Drain` for `slog-rs`
//!
//! ```
//! #[macro_use]
//! extern crate slog;
//! extern crate slog_mozlog_json;
//!
//! use slog::Drain;
//! use std::sync::Mutex;
//!
//! fn main() {
//!     let root = slog::Logger::root(
//!         Mutex::new(slog_mozlog_json::MozLogJson::default(std::io::stderr())).map(slog::Fuse),
//!         o!("version" => env!("CARGO_PKG_VERSION"))
//!     );
//! }
//! ```
//!
//! If the OS Environment variable "MOZLOG_GCP" is present and set to "true",
//! MozLog will output a Google Cloud Platform logging compliant JSON string.
// }}}

// {{{ Imports & meta
use std::{cell::RefCell, env, fmt, fmt::Write, io, process, result, str::FromStr};

use serde::ser::SerializeMap;
use slog::{FnValue, Key, OwnedKVList, Record, SendSyncRefUnwindSafeKV, KV};

use crate::util::{level_to_gcp_severity, level_to_severity};

// }}}

// {{{ Serialize
thread_local! {
    static TL_BUF: RefCell<String> = RefCell::new(String::with_capacity(128))
}

/// `slog::Serializer` adapter for `serde::Serializer`
///
/// Newtype to wrap serde Serializer, so that `Serialize` can be implemented
/// for it
struct SerdeSerializer<S: serde::Serializer> {
    /// Current state of map serializing: `serde::Serializer::MapState`
    ser_map: S::SerializeMap,
}

impl<S: serde::Serializer> SerdeSerializer<S> {
    /// Start serializing map of values
    fn start(ser: S, len: Option<usize>) -> result::Result<Self, slog::Error> {
        let ser_map = ser
            .serialize_map(len)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "serde serialization error"))?;
        Ok(SerdeSerializer { ser_map })
    }

    /// Finish serialization, and return the serializer
    fn end(self) -> result::Result<S::Ok, S::Error> {
        self.ser_map.end()
    }
}

macro_rules! impl_m(
    ($s:expr, $key:expr, $val:expr) => ({
        let k_s:  &str = $key.as_ref();
        $s.ser_map.serialize_entry(k_s, $val)
             .map_err(|_| io::Error::new(io::ErrorKind::Other, "serde serialization error"))?;
        Ok(())
    });
);

impl<S> slog::Serializer for SerdeSerializer<S>
where
    S: serde::Serializer,
{
    fn emit_bool(&mut self, key: Key, val: bool) -> slog::Result {
        impl_m!(self, key, &val)
    }

    fn emit_unit(&mut self, key: Key) -> slog::Result {
        impl_m!(self, key, &())
    }

    fn emit_char(&mut self, key: Key, val: char) -> slog::Result {
        impl_m!(self, key, &val)
    }

    fn emit_none(&mut self, key: Key) -> slog::Result {
        let val: Option<()> = None;
        impl_m!(self, key, &val)
    }
    fn emit_u8(&mut self, key: Key, val: u8) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_i8(&mut self, key: Key, val: i8) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_u16(&mut self, key: Key, val: u16) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_i16(&mut self, key: Key, val: i16) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_usize(&mut self, key: Key, val: usize) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_isize(&mut self, key: Key, val: isize) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_u32(&mut self, key: Key, val: u32) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_i32(&mut self, key: Key, val: i32) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_f32(&mut self, key: Key, val: f32) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_u64(&mut self, key: Key, val: u64) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_i64(&mut self, key: Key, val: i64) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_f64(&mut self, key: Key, val: f64) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_str(&mut self, key: Key, val: &str) -> slog::Result {
        impl_m!(self, key, &val)
    }
    fn emit_arguments(&mut self, key: Key, val: &fmt::Arguments) -> slog::Result {
        TL_BUF.with(|buf| {
            let mut buf = buf.borrow_mut();

            buf.write_fmt(*val).unwrap();

            let res = { || impl_m!(self, key, &*buf) }();
            buf.clear();
            res
        })
    }

    #[cfg(feature = "nested-values")]
    fn emit_serde(&mut self, key: Key, value: &slog::SerdeValue) -> slog::Result {
        impl_m!(self, key, value.as_serde())
    }
}
// }}}

// {{{ MozLogJson
/// Json `Drain`
///
/// Each record will be printed as a Json map
/// to a given `io`
pub struct MozLogJson<W: io::Write> {
    newlines: bool,
    values: Vec<OwnedKVList>,
    io: RefCell<W>,
    pretty: bool,
}

impl<W> MozLogJson<W>
where
    W: io::Write,
{
    /// New `Json` `Drain` with default key-value pairs added
    pub fn default(io: W) -> MozLogJson<W> {
        MozLogJsonBuilder::new(io).build()
    }

    /// Build custom `Json` `Drain`
    #[allow(clippy::new_ret_no_self)]
    pub fn new(io: W) -> MozLogJsonBuilder<W> {
        MozLogJsonBuilder::new(io)
    }

    fn log_placeholder_impl<F>(
        &self,
        serializer: &mut serde_json::ser::Serializer<&mut io::Cursor<Vec<u8>>, F>,
        rinfo: &Record,
    ) -> io::Result<()>
    where
        F: serde_json::ser::Formatter,
    {
        let mut serializer = SerdeSerializer::start(&mut *serializer, None)?;

        for kv in &self.values {
            kv.serialize(rinfo, &mut serializer)?;
        }

        let fields_placeholder = kv!("Fields" => "00PLACEHOLDER00");
        fields_placeholder.serialize(rinfo, &mut serializer)?;

        let res = serializer.end();

        res.map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(())
    }

    fn log_fields_impl<F>(
        &self,
        serializer: &mut serde_json::ser::Serializer<&mut io::Cursor<Vec<u8>>, F>,
        rinfo: &Record,
        logger_values: &OwnedKVList,
    ) -> io::Result<()>
    where
        F: serde_json::ser::Formatter,
    {
        let mut serializer = SerdeSerializer::start(&mut *serializer, None)?;

        let msg = kv!("msg" => format!("{}", rinfo.msg()));
        msg.serialize(rinfo, &mut serializer)?;

        logger_values.serialize(rinfo, &mut serializer)?;
        rinfo.kv().serialize(rinfo, &mut serializer)?;

        Ok(())
    }
}

impl<W> slog::Drain for MozLogJson<W>
where
    W: io::Write,
{
    type Ok = ();
    type Err = io::Error;
    fn log(&self, rinfo: &Record, logger_values: &OwnedKVList) -> io::Result<()> {
        // XXX: UGLY HACK HERE
        // First write out the structure without the Fields nested
        let mut buf = io::Cursor::new(Vec::new());
        if self.pretty {
            let mut serializer = serde_json::Serializer::pretty(&mut buf);
            self.log_placeholder_impl(&mut serializer, rinfo)?;
        } else {
            let mut serializer = serde_json::Serializer::new(&mut buf);
            self.log_placeholder_impl(&mut serializer, rinfo)?;
        };
        let payload = String::from_utf8(buf.into_inner()).unwrap();

        // XXX: UGLY HACK PART 2: Now write out just the Fields entry we replace with
        let mut buf = io::Cursor::new(Vec::new());
        if self.pretty {
            let mut serializer = serde_json::Serializer::pretty(&mut buf);
            self.log_fields_impl(&mut serializer, rinfo, logger_values)?;
        } else {
            let mut serializer = serde_json::Serializer::new(&mut buf);
            self.log_fields_impl(&mut serializer, rinfo, logger_values)?;
        };
        let fields = String::from_utf8(buf.into_inner()).unwrap();

        // And now we replace the placeholder with the contents
        let mut payload = payload.replace("\"00PLACEHOLDER00\"", fields.as_str());
        // For some reason the replace loses an end }
        payload.push('}');

        let mut io = self.io.borrow_mut();
        io.write_all(payload.as_bytes())?;
        if self.newlines {
            io.write_all(b"\n")?;
        }
        Ok(())
    }
}

// }}}

// {{{ MozLogJsonBuilder
/// Json `Drain` builder
///
/// Create with `Json::new`.
pub struct MozLogJsonBuilder<W: io::Write> {
    newlines: bool,
    values: Vec<OwnedKVList>,
    io: W,
    pretty: bool,
    logger_name: Option<String>,
    msg_type: Option<String>,
    hostname: Option<String>,
    gcp: bool,
}

impl<W> MozLogJsonBuilder<W>
where
    W: io::Write,
{
    fn new(io: W) -> Self {
        MozLogJsonBuilder {
            newlines: true,
            values: vec![],
            io,
            pretty: false,
            logger_name: None,
            msg_type: None,
            hostname: None,
            gcp: bool::from_str(&env::var("MOZLOG_GCP").unwrap_or("false".to_owned()))
                .unwrap_or(false),
        }
    }

    /// Build `Json` `Drain`
    ///
    /// This consumes the builder.
    pub fn build(mut self) -> MozLogJson<W> {
        let mut values: Vec<OwnedKVList> = vec![];
        if let Some(ref logger_name) = self.logger_name {
            values.push(o!("Logger" => logger_name.to_owned()).into());
        }
        if let Some(ref msg_type) = self.msg_type {
            values.push(o!("Type" => msg_type.to_owned()).into());
        }
        if let Some(ref hostname) = self.hostname {
            values.push(o!("Hostname" => hostname.to_owned()).into());
        }
        values.push(
            o!(
            "Timestamp" => FnValue(|_ : &Record| {
                let now = chrono::Utc::now();
                let nsec: i64 = now.timestamp() * 1_000_000_000;
                nsec + (now.timestamp_subsec_nanos() as i64)
            }),
            "Pid" => process::id(),
            )
            .into(),
        );
        if self.gcp {
            values.push(
                o!(
                    "severity" => FnValue(|record : &Record| level_to_gcp_severity(record.level())),
                    // TODO: add additional components? https://cloud.google.com/logging/docs/structured-logging#special-payload-fields
                )
                .into(),
            );
        } else {
            values.push(
                o!("Severity" => FnValue(|record : &Record| level_to_severity(record.level())))
                    .into(),
            )
        }
        self.values.extend(values);

        MozLogJson {
            values: self.values,
            newlines: self.newlines,
            io: RefCell::new(self.io),
            pretty: self.pretty,
        }
    }

    /// Turn on GCP support
    pub fn enable_gcp(mut self) -> Self {
        self.gcp = true;
        self
    }
    /// Set writing a newline after every log record
    pub fn set_newlines(mut self, enabled: bool) -> Self {
        self.newlines = enabled;
        self
    }

    /// Set whether or not pretty formatted logging should be used
    pub fn set_pretty(mut self, enabled: bool) -> Self {
        self.pretty = enabled;
        self
    }

    /// Add custom values to be printed with this formatter
    pub fn add_key_value<T>(mut self, value: slog::OwnedKV<T>) -> Self
    where
        T: SendSyncRefUnwindSafeKV + 'static,
    {
        self.values.push(value.into());
        self
    }

    pub fn logger_name(mut self, logger_name: String) -> Self {
        self.logger_name = Some(logger_name);
        self
    }

    pub fn msg_type(mut self, msg_type: String) -> Self {
        self.msg_type = Some(msg_type);
        self
    }

    pub fn hostname(mut self, hostname: String) -> Self {
        self.hostname = Some(hostname);
        self
    }
}
// }}}
// vim: foldmethod=marker foldmarker={{{,}}}
